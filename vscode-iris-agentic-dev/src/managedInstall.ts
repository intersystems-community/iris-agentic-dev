import { execFile } from "child_process";
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import which from "which";
import { downloadBinary } from "./download";
import { getBinaryName, getDownloadUrl, getServerVersion } from "./platform";

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
      return binaryPath;
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
    return binaryPath;
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
    return binaryPath;
  } catch {
    log.error(
      `iris-agentic-dev: download failed and no cached binary available. ` +
        "Download manually from https://github.com/intersystems-community/iris-agentic-dev/releases " +
        "and set iris-agentic-dev.serverPath."
    );
    return null;
  }
}
