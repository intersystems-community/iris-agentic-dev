import { execFile } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import which from "which";
import { downloadBinary } from "./download";
import { getBinaryName, getDownloadUrl, getServerVersion } from "./platform";
import {
  applyRegistrations,
  installStableBinary,
  PinnedEntry,
  registrationFiles,
  scanRegistrations,
} from "./stablePath";

// `iris-agentic-dev --version` prints "iris-agentic-dev 0.9.6".
const VERSION_OUTPUT = /(\d+\.\d+\.\d+)/;

// Diagnostic only, so it must never delay activation for long. A binary that
// hangs on --version (or is not our binary at all) gets given up on.
const VERSION_TIMEOUT_MS = 3000;

/**
 * Reads the version a binary reports, or null if it cannot be determined.
 *
 * Never throws and never rejects: old builds predate --version, wrappers may
 * exit non-zero, and none of that should stop a binary from being used.
 */
function readBinaryVersion(binaryPath: string): Promise<string | null> {
  return new Promise((resolve) => {
    execFile(
      binaryPath,
      ["--version"],
      { timeout: VERSION_TIMEOUT_MS, windowsHide: true },
      (err, stdout, stderr) => {
        if (err && !stdout && !stderr) {
          resolve(null);
          return;
        }
        const match = VERSION_OUTPUT.exec(`${stdout}\n${stderr}`);
        resolve(match ? match[1] : null);
      }
    );
  });
}

/**
 * The server version this extension expects, or null if it cannot be read.
 *
 * Tier 2 only uses this to decide whether to warn, so a missing declaration
 * must not stop a PATH binary from being used — tier 3 is where that is fatal.
 */
function expectedServerVersion(
  context: vscode.ExtensionContext
): string | null {
  try {
    return getServerVersion(context.extension.packageJSON);
  } catch {
    return null;
  }
}

// Prevents duplicate downloads when multiple VS Code windows activate simultaneously.
let activeResolve: Promise<string | null> | undefined;

export function resolveServerBinary(
  context: vscode.ExtensionContext
): Promise<string | null> {
  if (activeResolve) return activeResolve;
  activeResolve = _resolve(context).finally(() => {
    activeResolve = undefined;
  });
  return activeResolve;
}

async function _resolve(
  context: vscode.ExtensionContext
): Promise<string | null> {
  const log = vscode.window.createOutputChannel("iris-agentic-dev", {
    log: true,
  });

  // Tier 1: explicit serverPath setting
  const cfg = vscode.workspace.getConfiguration("iris-agentic-dev");
  const override = cfg.get<string>("serverPath");
  if (override && override.trim()) {
    try {
      fs.accessSync(override, fs.constants.X_OK);
      log.info(`iris-agentic-dev: using serverPath binary: ${override}`);
      return override;
    } catch {
      log.error(
        `iris-agentic-dev: serverPath "${override}" is not executable or does not exist. ` +
          "Fix iris-agentic-dev.serverPath in settings or clear it to use auto-install."
      );
      return null;
    }
  }

  // Tier 2: PATH lookup
  for (const name of ["iris-agentic-dev", "iris-agentic-dev.exe"]) {
    let found: string;
    try {
      found = which.sync(name);
    } catch {
      continue;
    }
    log.info(`iris-agentic-dev: using PATH binary: ${found}`);

    // A PATH binary always wins — overriding what someone deliberately
    // installed would be worse than running a stale version. But say so when
    // it is stale: a Homebrew or ~/.local/bin copy left behind while the
    // extension moved on shows up as tools missing or misbehaving, with
    // nothing connecting that to the version.
    const expected = expectedServerVersion(context);
    if (expected) {
      const actual = await readBinaryVersion(found);
      if (actual && actual !== expected) {
        log.warn(
          `iris-agentic-dev: PATH binary at ${found} reports v${actual}, but this ` +
            `extension expects v${expected}. Tools added or changed since v${actual} ` +
            "will be missing or behave differently. Upgrade it (`brew upgrade " +
            "iris-agentic-dev`), or clear it from PATH and unset " +
            "iris-agentic-dev.serverPath to let the extension manage the binary."
        );
      }
    }
    return found;
  }

  // Tier 3: managed download
  const binaryName = getBinaryName(process.platform, process.arch);
  if (!binaryName) {
    log.warn(
      `iris-agentic-dev: unsupported platform ${process.platform}/${process.arch} — auto-install not available. ` +
        "Set iris-agentic-dev.serverPath to the binary location."
    );
    return null;
  }

  // The server binary version, NOT the extension version — the two sequences
  // are independent and only the former matches a GitHub release tag.
  let version: string;
  try {
    version = getServerVersion(context.extension.packageJSON);
  } catch (err) {
    log.error(
      `iris-agentic-dev: ${err instanceof Error ? err.message : String(err)} ` +
        "Set iris-agentic-dev.serverPath to a binary location as a workaround."
    );
    return null;
  }
  const storageDir = context.globalStorageUri.fsPath;
  const versionFile = path.join(storageDir, "iris-agentic-dev.version");
  const binaryDir = path.join(storageDir, `iris-agentic-dev-${version}`);
  const binaryPath = path.join(binaryDir, binaryName);

  // Check cache
  const cachedVersion = await fs.promises
    .readFile(versionFile, "utf8")
    .then((s) => s.trim())
    .catch(() => "");

  if (cachedVersion === version) {
    try {
      fs.accessSync(binaryPath, fs.constants.X_OK);
      // On Windows: clean up leftover .old binary from previous update
      if (process.platform === "win32") {
        const oldPath = binaryPath + ".old";
        await fs.promises.unlink(oldPath).catch(() => {});
      }
      log.info(`iris-agentic-dev: using cached binary v${version}: ${binaryPath}`);
      return publish(context, binaryPath, storageDir, version, log);
    } catch {
      // Binary missing despite version marker — fall through to download
    }
  }

  // Need to download
  const downloadUrl = getDownloadUrl(version, process.platform, process.arch);
  if (!downloadUrl) {
    return null;
  }

  log.info(`iris-agentic-dev: downloading v${version} from ${downloadUrl}`);

  // Windows: rename existing binary before replacing to avoid file-lock error
  if (process.platform === "win32") {
    const oldPath = binaryPath + ".old";
    await fs.promises.rename(binaryPath, oldPath).catch(() => {});
  }

  await fs.promises.mkdir(binaryDir, { recursive: true });

  let downloadError: Error | undefined;
  try {
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "iris-agentic-dev",
        cancellable: false,
      },
      async (progress) => {
        progress.report({
          message: `Downloading MCP server v${version}…`,
          increment: 0,
        });
        await downloadBinary(downloadUrl, binaryPath, (fraction) => {
          progress.report({
            message: `Downloading MCP server v${version} (${Math.round(fraction * 100)}%)`,
            increment: fraction * 100,
          });
        });
      }
    );
  } catch (err) {
    downloadError = err instanceof Error ? err : new Error(String(err));
  }

  if (!downloadError) {
    // Set executable permission on mac/linux
    if (process.platform !== "win32") {
      await fs.promises.chmod(binaryPath, 0o755);
    }
    await fs.promises.mkdir(storageDir, { recursive: true });
    await fs.promises.writeFile(versionFile, version, "utf8");
    log.info(`iris-agentic-dev: installed v${version} at ${binaryPath}`);
    return publish(context, binaryPath, storageDir, version, log);
  }

  // Download failed — fall back to stale cache if available
  log.warn(
    `iris-agentic-dev: download failed: ${downloadError.message}. Checking for stale cached binary.`
  );
  try {
    fs.accessSync(binaryPath, fs.constants.X_OK);
    log.warn(
      `iris-agentic-dev: falling back to stale binary v${cachedVersion}: ${binaryPath}`
    );
    return publish(context, binaryPath, storageDir, version, log);
  } catch {
    log.error(
      `iris-agentic-dev: download failed and no cached binary available. ` +
        "Download manually from https://github.com/intersystems-community/iris-agentic-dev/releases " +
        "and set iris-agentic-dev.serverPath."
    );
    return null;
  }
}

/**
 * Copies the managed binary to its fixed-name path and returns the path to
 * launch, falling back to the versioned path if the copy cannot be made. Then
 * offers, without waiting for an answer, to repoint other clients still
 * registered with a version directory.
 */
async function publish(
  context: vscode.ExtensionContext,
  binaryPath: string,
  storageDir: string,
  version: string,
  log: vscode.LogOutputChannel
): Promise<string> {
  const stable = await installStableBinary(
    binaryPath,
    storageDir,
    version,
    process.platform
  );
  if (!stable) {
    log.warn(
      `iris-agentic-dev: could not update ${path.join(storageDir, "bin")} ` +
        "(a running copy may be locked); will retry on next activation."
    );
    return binaryPath;
  }
  log.info(
    `iris-agentic-dev: v${version} is at ${stable}. Register other MCP clients with this path; it does not change on update.`
  );
  void offerRegistrationUpdate(context, storageDir, stable, version, log);
  return stable;
}

// globalState key: the server version for which the user answered Not now.
const DECLINED_KEY = "iris-agentic-dev.registrationUpdateDeclined";
const UPDATE = "Update";
const SHOW_FILES = "Show files";
const NOT_NOW = "Not now";

// Activation and the MCP provider both resolve the binary; one question at a time.
let offerInFlight: Promise<void> | undefined;

/** The offer currently waiting on the user, if any. For tests. */
export function registrationOffer(): Promise<void> {
  return offerInFlight ?? Promise.resolve();
}

/**
 * Claude Code, Cursor and other clients keep whatever path they were
 * registered with, so one pinned to a version directory never sees an update.
 * This finds such entries and asks before changing anything: Update rewrites
 * exactly the entries found, Show files lists them, and Not now (or closing
 * the notification) writes nothing and is not asked again until a newer
 * binary arrives. It is the only path that writes another client's config.
 */
export function offerRegistrationUpdate(
  context: vscode.ExtensionContext,
  storageDir: string,
  stable: string,
  version: string,
  log: vscode.LogOutputChannel
): Promise<void> {
  offerInFlight ??= askToRepoint(context, storageDir, stable, version, log)
    .catch((err) =>
      log.warn(`iris-agentic-dev: checking other MCP clients failed: ${err}`)
    )
    .finally(() => {
      offerInFlight = undefined;
    });
  return offerInFlight;
}

async function askToRepoint(
  context: vscode.ExtensionContext,
  storageDir: string,
  stable: string,
  version: string,
  log: vscode.LogOutputChannel
): Promise<void> {
  if (context.globalState.get<string>(DECLINED_KEY) === version) return;

  const files = registrationFiles({
    home: os.homedir(),
    appData: process.env.APPDATA,
    platform: process.platform,
    // globalStorageUri is <User>/globalStorage/<publisher.extension>.
    userDir: path.dirname(path.dirname(storageDir)),
    workspaceDirs: (vscode.workspace.workspaceFolders ?? [])
      .filter((f) => f.uri.scheme === "file")
      .map((f) => f.uri.fsPath),
  });
  const found = await scanRegistrations(files, storageDir, process.platform);
  const count = found.reduce((n, f) => n + f.entries.length, 0);
  if (count === 0) return;

  const question =
    `Found ${count} registration(s) pointing at an old iris-agentic-dev binary. ` +
    "Update them to the current one?";
  function ask(): Thenable<string | undefined> {
    return vscode.window.showInformationMessage(
      question,
      UPDATE,
      SHOW_FILES,
      NOT_NOW
    );
  }

  let answer = await ask();
  while (answer === SHOW_FILES) {
    log.info(`iris-agentic-dev: registrations that would point at ${stable}:`);
    for (const { file, entries } of found) {
      for (const e of entries) {
        log.info(`  ${file}: ${describePointer(e)} = ${e.value}`);
      }
    }
    log.show(true);
    answer = await ask();
  }
  if (answer !== UPDATE) {
    await context.globalState.update(DECLINED_KEY, version);
    return;
  }

  const moved = await applyRegistrations(found, stable);
  for (const { file, rewritten } of moved) {
    log.info(
      `iris-agentic-dev: repointed ${rewritten} registration(s) in ${file} to ${stable}`
    );
  }
  if (moved.length > 0) {
    void vscode.window.showInformationMessage(
      `iris-agentic-dev: ${moved.map((m) => m.file).join(", ")} now point(s) ` +
        `at ${stable}. Restart those MCP clients (Claude Code, Cursor) to run v${version}.`
    );
  } else {
    log.warn(
      "iris-agentic-dev: no registration was changed; the files changed since " +
        "they were checked. They will be checked again on next activation."
    );
  }
}

/** mcpServers.iad.args[1] */
function describePointer({ pointer }: PinnedEntry): string {
  let s = "";
  for (const k of pointer) {
    if (typeof k === "number") {
      s += `[${k}]`;
    } else {
      s += s ? `.${k}` : k;
    }
  }
  return s;
}
