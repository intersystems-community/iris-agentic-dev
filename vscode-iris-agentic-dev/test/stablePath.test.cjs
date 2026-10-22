"use strict";

// The extension keeps each downloaded binary in a version-named directory,
// iris-agentic-dev-<version>/. A client registered by hand with that path —
// Claude Code's ~/.claude.json, Cursor's mcp.json — keeps launching that
// version after the extension has downloaded newer ones, because nothing ever
// rewrites the registration. A Windows user found Claude Code still on 1.5.0
// while globalStorage held 1.5.0, 1.6.0 and 1.6.3.
//
// The fix: a fixed-name copy at bin/iris-agentic-dev(.exe), refreshed on each
// update, plus a one-time repoint of registrations pinned to a version
// directory inside the extension's own storage. These tests cover both.

const { test } = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const os = require("node:os");
const fs = require("node:fs");

const {
  installStableBinary,
  stableBinaryPath,
  isManagedVersionedPath,
  migrateRegistrations,
  registrationFiles,
} = require("../.test-out/stablePath.cjs");

function tmpdir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "iad-stable-"));
}

/** Writes a fake versioned binary the way managedInstall lays it out. */
function seedVersion(storage, version, platform = process.platform) {
  const name =
    platform === "win32"
      ? "iris-agentic-dev-windows-x86_64.exe"
      : "iris-agentic-dev-macos-arm64";
  const dir = path.join(storage, `iris-agentic-dev-${version}`);
  fs.mkdirSync(dir, { recursive: true });
  const p = path.join(dir, name);
  fs.writeFileSync(p, `#!/bin/sh\necho "iris-agentic-dev ${version}"\n`, {
    mode: 0o755,
  });
  return p;
}

/**
 * fs operations with Windows lock semantics for the paths in `locked`: a
 * running .exe cannot be replaced or deleted, but it can be renamed, and the
 * lock follows the file to its new name.
 */
function windowsLockOps(locked) {
  const denied = (code, p) =>
    Object.assign(new Error(`${code}: operation not permitted, '${p}'`), {
      code,
    });
  return {
    rename: async (from, to) => {
      if (locked.has(to)) throw denied("EPERM", to);
      await fs.promises.rename(from, to);
      if (locked.has(from)) {
        locked.delete(from);
        locked.add(to);
      }
    },
    unlink: async (p) => {
      if (locked.has(p)) throw denied("EBUSY", p);
      await fs.promises.unlink(p);
    },
  };
}

// --- the fixed-name copy ----------------------------------------------------

test("stable path is version-free and gets .exe on Windows", () => {
  const storage = "/s";
  assert.equal(
    stableBinaryPath(storage, "darwin"),
    path.join(storage, "bin", "iris-agentic-dev")
  );
  assert.equal(
    stableBinaryPath(storage, "win32"),
    path.join(storage, "bin", "iris-agentic-dev.exe")
  );
});

test("an update replaces the fixed-name copy with the newest binary", async () => {
  const storage = tmpdir();
  try {
    const v163 = seedVersion(storage, "1.6.3");
    const v164 = seedVersion(storage, "1.6.4");

    const first = await installStableBinary(v163, storage, "1.6.3", process.platform);
    assert.equal(first, stableBinaryPath(storage, process.platform));
    assert.match(fs.readFileSync(first, "utf8"), /1\.6\.3/);

    const second = await installStableBinary(v164, storage, "1.6.4", process.platform);
    assert.equal(second, first, "the path must not change across updates");
    assert.match(fs.readFileSync(second, "utf8"), /1\.6\.4/);
    if (process.platform !== "win32") {
      assert.ok(fs.statSync(second).mode & 0o100, "copy must be executable");
    }
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

test("Windows: a running fixed-name .exe is moved aside, not overwritten", async () => {
  const storage = tmpdir();
  try {
    const v163 = seedVersion(storage, "1.6.3", "win32");
    const v164 = seedVersion(storage, "1.6.4", "win32");
    const stable = await installStableBinary(v163, storage, "1.6.3", "win32");

    // Claude Code is running the 1.6.3 copy.
    const locked = new Set([stable]);
    const ops = windowsLockOps(locked);

    const result = await installStableBinary(v164, storage, "1.6.4", "win32", ops);
    assert.equal(result, stable);
    assert.match(fs.readFileSync(stable, "utf8"), /1\.6\.4/);

    // The running copy still exists under another name, still locked.
    const binDir = path.dirname(stable);
    const aside = fs
      .readdirSync(binDir)
      .filter((f) => f.startsWith("iris-agentic-dev.exe.old"));
    assert.equal(aside.length, 1, `expected one moved-aside copy, got ${aside}`);
    assert.match(fs.readFileSync(path.join(binDir, aside[0]), "utf8"), /1\.6\.3/);
    assert.ok(locked.has(path.join(binDir, aside[0])));

    // Next activation, still running: cleanup fails quietly.
    assert.equal(
      await installStableBinary(v164, storage, "1.6.4", "win32", ops),
      stable
    );
    // Once the old process exits, the next activation removes it.
    locked.clear();
    await installStableBinary(v164, storage, "1.6.4", "win32", ops);
    assert.deepEqual(
      fs.readdirSync(binDir).filter((f) => f.includes(".old")),
      []
    );
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

test("Windows: when even the rename is refused, it returns null and retries next time", async () => {
  const storage = tmpdir();
  try {
    const v163 = seedVersion(storage, "1.6.3", "win32");
    const v164 = seedVersion(storage, "1.6.4", "win32");
    const stable = await installStableBinary(v163, storage, "1.6.3", "win32");

    const refuseAll = {
      rename: async (_from, to) => {
        throw Object.assign(new Error(`EPERM '${to}'`), { code: "EPERM" });
      },
      unlink: async (p) => fs.promises.unlink(p),
    };
    const result = await installStableBinary(v164, storage, "1.6.4", "win32", refuseAll);
    assert.equal(result, null);
    assert.match(fs.readFileSync(stable, "utf8"), /1\.6\.3/, "old copy untouched");
    assert.deepEqual(
      fs.readdirSync(path.dirname(stable)).filter((f) => f.includes(".new")),
      [],
      "no half-written temp file left behind"
    );

    // Lock gone: the next activation finishes the job.
    assert.equal(await installStableBinary(v164, storage, "1.6.4", "win32"), stable);
    assert.match(fs.readFileSync(stable, "utf8"), /1\.6\.4/);
  } finally {
    fs.rmSync(storage, { recursive: true, force: true });
  }
});

// --- which registrations belong to the extension ----------------------------

test("a version directory inside globalStorage is managed", () => {
  const storage =
    "C:\\Users\\doc\\AppData\\Roaming\\Code\\User\\globalStorage\\intersystems-community.iris-agentic-dev";
  const pinned = `${storage}\\iris-agentic-dev-1.5.0\\iris-agentic-dev-windows-x86_64.exe`;
  assert.equal(isManagedVersionedPath(pinned, storage, "win32"), true);
  // Windows paths compare without case, and JSON configs often use forward slashes.
  assert.equal(
    isManagedVersionedPath(pinned.toLowerCase().replace(/\\/g, "/"), storage, "win32"),
    true
  );
  const posix = "/home/doc/.config/Code/User/globalStorage/intersystems-community.iris-agentic-dev";
  assert.equal(
    isManagedVersionedPath(`${posix}/iris-agentic-dev-1.6.0/iris-agentic-dev-linux-x86_64`, posix, "linux"),
    true
  );
});

test("paths the user chose are not managed", () => {
  const storage = "/home/doc/.config/Code/User/globalStorage/intersystems-community.iris-agentic-dev";
  for (const p of [
    "/usr/local/bin/iris-agentic-dev",
    "iris-agentic-dev",
    "/home/doc/bin/iris-agentic-dev-1.5.0/iris-agentic-dev-linux-x86_64",
    // A sibling directory sharing the prefix is not inside storage.
    "/home/doc/.config/Code/User/globalStorage/intersystems-community.iris-agentic-dev-fork/iris-agentic-dev-1.5.0/x",
    // Another editor's storage (Cursor) belongs to that editor's extension.
    "/home/doc/.config/Cursor/User/globalStorage/intersystems-community.iris-agentic-dev/iris-agentic-dev-1.5.0/x",
    // The stable path itself needs no repointing.
    `${storage}/bin/iris-agentic-dev`,
    `${storage}/somewhere-else/iris-agentic-dev`,
  ]) {
    assert.equal(isManagedVersionedPath(p, storage, "linux"), false, p);
  }
});

// --- repointing registrations -----------------------------------------------

function claudeJson(storage, platform) {
  const sep = platform === "win32" ? "\\" : "/";
  const pinned = (v) =>
    `${storage}${sep}iris-agentic-dev-${v}${sep}iris-agentic-dev-windows-x86_64.exe`;
  return {
    numStartups: 42,
    mcpServers: {
      "iris-agentic-dev": { type: "stdio", command: pinned("1.5.0"), args: ["mcp"], env: {} },
      mine: { command: "C:\\tools\\iris-agentic-dev.exe", args: ["mcp"] },
    },
    projects: {
      "C:/work/docs": {
        mcpServers: {
          iad: { command: "cmd", args: ["/c", pinned("1.6.0"), "mcp"] },
        },
      },
    },
  };
}

test("a pinned 1.5.0 entry inside globalStorage is repointed; the user's own is not", async () => {
  const dir = tmpdir();
  try {
    const storage = "C:\\Users\\doc\\AppData\\Roaming\\Code\\User\\globalStorage\\intersystems-community.iris-agentic-dev";
    const stable = `${storage}\\bin\\iris-agentic-dev.exe`;
    const file = path.join(dir, ".claude.json");
    fs.writeFileSync(file, JSON.stringify(claudeJson(storage, "win32"), null, 2) + "\n");

    const results = await migrateRegistrations([file], storage, stable, "win32");
    assert.deepEqual(results, [{ file, rewritten: 2 }]);

    const after = JSON.parse(fs.readFileSync(file, "utf8"));
    assert.equal(after.mcpServers["iris-agentic-dev"].command, stable);
    assert.deepEqual(after.projects["C:/work/docs"].mcpServers.iad.args, ["/c", stable, "mcp"]);
    assert.equal(after.mcpServers.mine.command, "C:\\tools\\iris-agentic-dev.exe");
    assert.equal(after.numStartups, 42, "unrelated keys survive");
    assert.ok(fs.readFileSync(file, "utf8").endsWith("}\n"), "trailing newline kept");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a file with only user-authored entries is left byte-for-byte alone", async () => {
  const dir = tmpdir();
  try {
    const storage = path.join(dir, "storage");
    const file = path.join(dir, "mcp.json");
    const text = '{"mcpServers":{"iad":{"command":"/opt/iad/iris-agentic-dev","args":["mcp"]}}}';
    fs.writeFileSync(file, text);
    const before = fs.statSync(file).mtimeMs;

    const results = await migrateRegistrations(
      [file],
      storage,
      path.join(storage, "bin", "iris-agentic-dev"),
      process.platform
    );
    assert.deepEqual(results, []);
    assert.equal(fs.readFileSync(file, "utf8"), text);
    assert.equal(fs.statSync(file).mtimeMs, before);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("missing files and files that are not plain JSON are skipped, never rewritten", async () => {
  const dir = tmpdir();
  try {
    const storage = path.join(dir, "storage");
    const pinned = path.join(storage, "iris-agentic-dev-1.5.0", "iris-agentic-dev-macos-arm64");
    // VS Code's mcp.json allows comments; JSON.parse does not. Leave it alone
    // rather than risk writing back a file with the comments stripped.
    const jsonc = path.join(dir, "mcp.json");
    const text = `{\n  // mine\n  "servers": {"iad": {"command": ${JSON.stringify(pinned)}}}\n}\n`;
    fs.writeFileSync(jsonc, text);

    const results = await migrateRegistrations(
      [path.join(dir, "absent.json"), jsonc],
      storage,
      path.join(storage, "bin", "iris-agentic-dev"),
      process.platform
    );
    assert.deepEqual(results, []);
    assert.equal(fs.readFileSync(jsonc, "utf8"), text);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("registrationFiles covers Claude Code, Claude Desktop, Cursor and VS Code", () => {
  const files = registrationFiles({
    home: "/h",
    appData: undefined,
    platform: "darwin",
    userDir: "/h/Library/Application Support/Code/User",
    workspaceDirs: ["/w"],
  });
  for (const f of [
    path.join("/h", ".claude.json"),
    path.join("/h", "Library", "Application Support", "Claude", "claude_desktop_config.json"),
    path.join("/h", ".cursor", "mcp.json"),
    path.join("/h/Library/Application Support/Code/User", "mcp.json"),
    path.join("/w", ".mcp.json"),
    path.join("/w", ".vscode", "mcp.json"),
    path.join("/w", ".cursor", "mcp.json"),
  ]) {
    assert.ok(files.includes(f), `missing ${f}`);
  }
  const win = registrationFiles({
    home: "C:\\Users\\doc",
    appData: "C:\\Users\\doc\\AppData\\Roaming",
    platform: "win32",
    userDir: "C:\\Users\\doc\\AppData\\Roaming\\Code\\User",
    workspaceDirs: [],
  });
  assert.ok(win.includes(path.join("C:\\Users\\doc", ".claude.json")));
  assert.ok(
    win.includes(path.join("C:\\Users\\doc\\AppData\\Roaming", "Claude", "claude_desktop_config.json"))
  );
});
