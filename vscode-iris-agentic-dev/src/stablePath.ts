import * as fs from "fs";
import * as path from "path";

// Each downloaded binary lives in its own version directory,
// <storage>/iris-agentic-dev-<version>/. VS Code's own MCP registration asks
// for the path every time, so it always gets the newest one. A client
// registered by hand does not: Claude Code's ~/.claude.json, or Cursor's
// mcp.json, keeps the path it was given, and kept launching 1.5.0 for a user
// whose storage held 1.6.3. The fix is a version-free copy at
// <storage>/bin/iris-agentic-dev(.exe), refreshed on every update, and a
// one-time repoint of registrations that still name a version directory.

const STABLE_DIR = "bin";
const STABLE_MARKER = "iris-agentic-dev.version";
const VERSION_DIR = /^iris-agentic-dev-\d+\.\d+\.\d+$/i;

/** The fixed-name binary that other MCP clients should be registered with. */
export function stableBinaryPath(storageDir: string, platform: string): string {
  const name =
    platform === "win32" ? "iris-agentic-dev.exe" : "iris-agentic-dev";
  return path.join(storageDir, STABLE_DIR, name);
}

/** The two operations whose behaviour differs on Windows when a binary is running. */
export interface SwapOps {
  rename(from: string, to: string): Promise<void>;
  unlink(p: string): Promise<void>;
}

const realOps: SwapOps = {
  rename: (from, to) => fs.promises.rename(from, to),
  unlink: (p) => fs.promises.unlink(p),
};

/**
 * Makes the fixed-name copy match `sourcePath` (the binary for `version`) and
 * returns its path, or null if the copy could not be put in place this time.
 *
 * Windows will not replace or delete a running .exe, but it will rename one.
 * So when the plain swap is refused, the running copy is moved aside to
 * `.old-<time>` (the process keeps running from there) and the new one takes
 * its name. Moved-aside copies are deleted on a later call, once nothing has
 * them open. If even the rename is refused, the old copy stays, null is
 * returned, and the next activation tries again.
 */
export async function installStableBinary(
  sourcePath: string,
  storageDir: string,
  version: string,
  platform: string,
  ops: SwapOps = realOps,
): Promise<string | null> {
  const stable = stableBinaryPath(storageDir, platform);
  const binDir = path.dirname(stable);
  const marker = path.join(binDir, STABLE_MARKER);
  const base = path.basename(stable);
  try {
    await fs.promises.mkdir(binDir, { recursive: true });

    for (const f of await fs.promises.readdir(binDir)) {
      if (f.startsWith(`${base}.old`)) {
        await ops.unlink(path.join(binDir, f)).catch(() => {});
      }
    }

    const current = await fs.promises
      .readFile(marker, "utf8")
      .then((s) => s.trim())
      .catch(() => "");
    if (current === version && fs.existsSync(stable)) return stable;

    // A per-process temp name: two VS Code windows activating together must
    // not write into the same file.
    const tmp = `${stable}.new-${process.pid}`;
    await fs.promises.copyFile(sourcePath, tmp);
    if (platform !== "win32") await fs.promises.chmod(tmp, 0o755);
    try {
      await ops.rename(tmp, stable);
    } catch {
      try {
        await ops.rename(stable, `${stable}.old-${Date.now()}`);
        await ops.rename(tmp, stable);
      } catch {
        await ops.unlink(tmp).catch(() => {});
        return null;
      }
    }
    await fs.promises.writeFile(marker, version, "utf8");
    return stable;
  } catch {
    return null;
  }
}

/**
 * True when `command` is a binary inside one of this extension's version
 * directories, i.e. a path the extension put there. Anything else, including
 * a binary the user installed or another editor's copy, is theirs.
 */
export function isManagedVersionedPath(
  command: string,
  storageDir: string,
  platform: string,
): boolean {
  const p = platform === "win32" ? path.win32 : path.posix;
  if (!p.isAbsolute(command)) return false;
  // win32.relative compares without case, as Windows paths do.
  const rel = p.relative(storageDir, p.normalize(command));
  if (!rel || rel.startsWith("..") || p.isAbsolute(rel)) return false;
  const parts = rel.split(p.sep);
  return parts.length === 2 && VERSION_DIR.test(parts[0]);
}

/** Rewrites managed paths in every `command` and `args` entry; returns the count. */
function repoint(
  node: unknown,
  storageDir: string,
  stable: string,
  platform: string,
): number {
  if (Array.isArray(node)) {
    return node.reduce(
      (n, v) => n + repoint(v, storageDir, stable, platform),
      0,
    );
  }
  if (!node || typeof node !== "object") return 0;
  const obj = node as Record<string, unknown>;
  let n = 0;
  if (
    typeof obj.command === "string" &&
    isManagedVersionedPath(obj.command, storageDir, platform)
  ) {
    obj.command = stable;
    n++;
  }
  if (Array.isArray(obj.args)) {
    obj.args = obj.args.map((a) => {
      if (
        typeof a === "string" &&
        isManagedVersionedPath(a, storageDir, platform)
      ) {
        n++;
        return stable;
      }
      return a;
    });
  }
  for (const [k, v] of Object.entries(obj)) {
    if (k !== "args") n += repoint(v, storageDir, stable, platform);
  }
  return n;
}

/**
 * Repoints MCP registrations that name one of this extension's version
 * directories to `stablePath`. A file is rewritten only when it held such an
 * entry; a missing file, or one that is not plain JSON (VS Code's mcp.json
 * allows comments), is skipped. The write goes through a temp file and a
 * rename, and is abandoned if the file changed while it was being read.
 */
export async function migrateRegistrations(
  files: string[],
  storageDir: string,
  stablePath: string,
  platform: string,
): Promise<Array<{ file: string; rewritten: number }>> {
  const results: Array<{ file: string; rewritten: number }> = [];
  for (const file of files) {
    try {
      const before = await fs.promises.stat(file);
      const text = await fs.promises.readFile(file, "utf8");
      const data = JSON.parse(text);
      const rewritten = repoint(data, storageDir, stablePath, platform);
      if (rewritten === 0) continue;

      const indent = /^([ \t]+)"/m.exec(text)?.[1] ?? 2;
      let out = JSON.stringify(data, null, indent);
      if (text.endsWith("\n")) out += "\n";
      if (text.includes("\r\n")) out = out.replace(/\n/g, "\r\n");

      const tmp = `${file}.iad-${process.pid}.tmp`;
      try {
        await fs.promises.writeFile(tmp, out, { mode: before.mode & 0o777 });
        const now = await fs.promises.stat(file);
        if (now.mtimeMs !== before.mtimeMs || now.size !== before.size) {
          continue;
        }
        await fs.promises.rename(tmp, file);
        results.push({ file, rewritten });
      } finally {
        // Gone after a successful rename; otherwise do not leave it behind.
        await fs.promises.unlink(tmp).catch(() => {});
      }
    } catch {
      // Missing, unreadable, or not JSON: not ours to touch.
    }
  }
  return results;
}

function claudeDesktopDir(
  home: string,
  appData: string | undefined,
  platform: string,
): string {
  if (platform === "win32") {
    return path.join(
      appData ?? path.join(home, "AppData", "Roaming"),
      "Claude",
    );
  }
  if (platform === "darwin") {
    return path.join(home, "Library", "Application Support", "Claude");
  }
  return path.join(home, ".config", "Claude");
}

/** Config files where an MCP client may hold a hand-registered iris-agentic-dev. */
export function registrationFiles(opts: {
  home: string;
  appData: string | undefined;
  platform: string;
  userDir: string;
  workspaceDirs: string[];
}): string[] {
  const { home, appData, platform, userDir, workspaceDirs } = opts;
  return [
    path.join(home, ".claude.json"),
    path.join(
      claudeDesktopDir(home, appData, platform),
      "claude_desktop_config.json",
    ),
    path.join(home, ".cursor", "mcp.json"),
    path.join(home, ".codeium", "windsurf", "mcp_config.json"),
    path.join(userDir, "mcp.json"),
    ...workspaceDirs.flatMap((d) => [
      path.join(d, ".mcp.json"),
      path.join(d, ".vscode", "mcp.json"),
      path.join(d, ".cursor", "mcp.json"),
    ]),
  ];
}
