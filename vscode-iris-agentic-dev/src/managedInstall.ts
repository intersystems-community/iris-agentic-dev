import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import which from "which";
import { downloadBinary } from "./download";
import { getBinaryName, getDownloadUrl, getServerVersion } from "./platform";

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
    try {
      const found = which.sync(name);
      log.info(`iris-agentic-dev: using PATH binary: ${found}`);
      return found;
    } catch {
      /* try next */
    }
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
