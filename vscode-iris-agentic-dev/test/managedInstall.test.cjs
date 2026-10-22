"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const os = require("node:os");
const fs = require("node:fs");

// ---------------------------------------------------------------------------
// managedInstall.cjs is bundled with --external:vscode, so it does a bare
// require("vscode") at runtime. Redirect that to the checked-in stub before
// loading the bundle. Resolution has to be rewritten rather than the module
// aliased at bundle time: tests mutate this object to simulate settings and
// download failures, and an inlined copy would be a different instance.
//
// The stub previously lived in node_modules/vscode/, created by hand. npm ci
// wipes it, so on CI the require failed and this whole file was skipped — it
// only ever passed on machines where someone had made the stub locally.
// ---------------------------------------------------------------------------
const Module = require("node:module");
const STUB_PATH = require.resolve("./stubs/vscode.cjs");
const WHICH_STUB_PATH = require.resolve("./stubs/which.cjs");
const _origResolve = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === "vscode") return STUB_PATH;
  // `which` is also --external. Tier 2 of resolveServerBinary is a PATH
  // lookup, so with the real module these tests pass or fail depending on
  // whether the developer happens to have iris-agentic-dev installed — an
  // early return from tier 2 never reaches the cache and download logic
  // tiers 3+ exercise. Stubbed so PATH state is explicit per test.
  if (request === "which") return WHICH_STUB_PATH;
  return _origResolve.call(this, request, ...rest);
};

const vscodeStub = require("vscode");
const whichStub = require("which");

// Tier 3 now repoints MCP registrations under the home directory
// (~/.claude.json, ~/.cursor/mcp.json, ...). Point HOME at an empty temp dir so
// no test reads, let alone writes, the developer's real client configs.
const FAKE_HOME = fs.mkdtempSync(path.join(os.tmpdir(), "iad-home-"));
process.env.HOME = FAKE_HOME;
process.env.USERPROFILE = FAKE_HOME;

// Build managedInstall.ts → .test-out/managedInstall.cjs before running tests
// (done in package.json test script)
const {
  resolveServerBinary,
  offerRegistrationUpdate,
  registrationOffer,
} = require("../.test-out/managedInstall.cjs");

// ---------------------------------------------------------------------------
// Helper: build a fake ExtensionContext pointing at a temp dir
// ---------------------------------------------------------------------------
// `version` is the MCP server binary version — the thing managedInstall
// downloads. The extension's own version is deliberately different here so a
// regression back to packageJSON.version fails these tests instead of quietly
// building a 404 URL.
function makeContext(version, tmpDir) {
  return {
    extension: {
      packageJSON: {
        version: "0.4.26",
        irisAgenticDev: { serverVersion: version },
      },
    },
    globalStorageUri: { fsPath: tmpDir },
    globalState: makeMemento(),
  };
}

/** An in-memory stand-in for ExtensionContext.globalState. */
function makeMemento() {
  const m = new Map();
  return {
    get: (k) => m.get(k),
    update: async (k, v) => {
      m.set(k, v);
    },
  };
}

// ---------------------------------------------------------------------------
// US1 tests
// ---------------------------------------------------------------------------

test("US1: version marker absent → download path triggered", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const ctx = makeContext("0.9.5", tmp);

  // Can't override process.platform in the bundle, so verify the no-marker
  // path either returns a path or null without throwing.
  let result;
  try {
    // This will attempt a real download in CI — but we accept null or a path
    // The key invariant: no throw even when download fails
    result = await resolveServerBinary(ctx);
  } catch (err) {
    assert.fail(`resolveServerBinary must not throw: ${err}`);
  }
  // Result is either a string (path) or null — never throws
  assert.ok(result === null || typeof result === "string");
  fs.rmSync(tmp, { recursive: true, force: true });
});

test("US1: version marker matches and binary exists → returns cached path without downloading", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const version = "0.9.5";

  // Determine expected binary name for current platform
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const binaryName = getBinaryName(process.platform, process.arch);
  if (!binaryName) {
    // Unsupported platform in test env — skip
    fs.rmSync(tmp, { recursive: true, force: true });
    return;
  }

  // Pre-populate cache
  const binaryDir = path.join(tmp, `iris-agentic-dev-${version}`);
  fs.mkdirSync(binaryDir, { recursive: true });
  const binaryPath = path.join(binaryDir, binaryName);
  fs.writeFileSync(binaryPath, "#!/bin/sh\necho fake", { mode: 0o755 });
  fs.writeFileSync(path.join(tmp, "iris-agentic-dev.version"), version);

  const ctx = makeContext(version, tmp);
  const result = await resolveServerBinary(ctx);

  // The caller gets the fixed-name copy, not the version directory: that is
  // the path the Output channel shows, and so the one people paste into other
  // clients' configs.
  const { stableBinaryPath } = require("../.test-out/stablePath.cjs");
  assert.equal(result, stableBinaryPath(tmp, process.platform));
  assert.equal(fs.readFileSync(result, "utf8"), fs.readFileSync(binaryPath, "utf8"));
  fs.rmSync(tmp, { recursive: true, force: true });
});

test("US1: unsupported platform/arch → returns null (no PATH, no setting)", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  // We can't override process.platform, so we test via getBinaryName returning null.
  // On supported platforms, this test validates the null-safety of the fallback chain
  // for unsupported arch. win32/arm64 is the remaining unsupported combination:
  // linux/arm64 is a published asset now, so it no longer exercises the null path.
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const name = getBinaryName("win32", "arm64");
  assert.equal(name, null);
  fs.rmSync(tmp, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// US2 tests
// ---------------------------------------------------------------------------

test("US2: serverPath set to existing executable → returns it immediately", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  // Create a fake executable
  const fakeBin = path.join(tmp, "fake-iris-dev");
  fs.writeFileSync(fakeBin, "#!/bin/sh\necho ok", { mode: 0o755 });

  // Override vscode config stub to return the path
  const origGetConfig = vscodeStub.workspace.getConfiguration;
  vscodeStub.workspace.getConfiguration = () => ({
    get: (key) => (key === "serverPath" ? fakeBin : ""),
  });

  const ctx = makeContext("0.9.5", tmp);
  const result = await resolveServerBinary(ctx);

  vscodeStub.workspace.getConfiguration = origGetConfig;
  fs.rmSync(tmp, { recursive: true, force: true });

  assert.equal(result, fakeBin);
});

test("US2: serverPath set to non-existent path → returns null with error", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const badPath = path.join(tmp, "does-not-exist");

  const origGetConfig = vscodeStub.workspace.getConfiguration;
  vscodeStub.workspace.getConfiguration = () => ({
    get: (key) => (key === "serverPath" ? badPath : ""),
  });

  const ctx = makeContext("0.9.5", tmp);
  const result = await resolveServerBinary(ctx);

  vscodeStub.workspace.getConfiguration = origGetConfig;
  fs.rmSync(tmp, { recursive: true, force: true });

  assert.equal(result, null);
});

// ---------------------------------------------------------------------------
// US3 tests
// ---------------------------------------------------------------------------

test("US3: version marker mismatch → download triggered (no throw)", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  // Write an old version marker
  fs.writeFileSync(path.join(tmp, "iris-agentic-dev.version"), "0.0.0");

  const ctx = makeContext("0.9.5", tmp);
  let result;
  try {
    result = await resolveServerBinary(ctx);
  } catch (err) {
    assert.fail(`must not throw on version mismatch: ${err}`);
  }
  // Either downloaded successfully or fell back — never throws
  assert.ok(result === null || typeof result === "string");
  fs.rmSync(tmp, { recursive: true, force: true });
});

test("US3: download fails and stale binary exists → returns stale path", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const version = "0.9.5";

  const { getBinaryName } = require("../.test-out/platform.cjs");
  const binaryName = getBinaryName(process.platform, process.arch);
  if (!binaryName) {
    fs.rmSync(tmp, { recursive: true, force: true });
    return;
  }

  // Pre-populate a stale binary with old version marker
  const binaryDir = path.join(tmp, `iris-agentic-dev-${version}`);
  fs.mkdirSync(binaryDir, { recursive: true });
  const binaryPath = path.join(binaryDir, binaryName);
  fs.writeFileSync(binaryPath, "#!/bin/sh\necho stale", { mode: 0o755 });
  fs.writeFileSync(path.join(tmp, "iris-agentic-dev.version"), "0.0.0");

  // Make withProgress simulate a download failure
  const origWithProgress = vscodeStub.window.withProgress;
  vscodeStub.window.withProgress = async () => {
    throw new Error("simulated network failure");
  };

  const ctx = makeContext(version, tmp);
  const result = await resolveServerBinary(ctx);

  vscodeStub.window.withProgress = origWithProgress;

  // The stale binary is served through the fixed-name copy, like any other.
  const { stableBinaryPath } = require("../.test-out/stablePath.cjs");
  assert.equal(result, stableBinaryPath(tmp, process.platform));
  assert.match(fs.readFileSync(result, "utf8"), /stale/);
  fs.rmSync(tmp, { recursive: true, force: true });
});

test("US3: download fails and no cache → returns null", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));

  const origWithProgress = vscodeStub.window.withProgress;
  vscodeStub.window.withProgress = async () => {
    throw new Error("simulated network failure");
  };

  const ctx = makeContext("0.9.5", tmp);
  const result = await resolveServerBinary(ctx);

  vscodeStub.window.withProgress = origWithProgress;
  fs.rmSync(tmp, { recursive: true, force: true });

  assert.equal(result, null);
});

test("US3: on win32, getBinaryName returns .exe extension", () => {
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const name = getBinaryName("win32", "x64");
  assert.ok(name !== null && name.endsWith(".exe"));
});

// --- Tier 2: PATH lookup ----------------------------------------------------

test("tier 2: a binary on PATH is used as-is, without downloading", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const onPath = path.join(tmp, "iris-agentic-dev");
  fs.writeFileSync(onPath, "#!/bin/sh\necho ok", { mode: 0o755 });
  whichStub.__setFound(onPath);

  // Fail loudly if tier 2 falls through to a download.
  const origWithProgress = vscodeStub.window.withProgress;
  vscodeStub.window.withProgress = async () => {
    throw new Error("must not download when a binary is on PATH");
  };

  try {
    const result = await resolveServerBinary(makeContext("0.9.5", tmp));
    assert.equal(result, onPath);
  } finally {
    vscodeStub.window.withProgress = origWithProgress;
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

// A PATH binary still wins — overriding what a developer deliberately put on
// PATH would be worse than a stale version. But a version mismatch is now
// visible instead of silent: a Homebrew install left at 0.9.4 while the
// extension expects 0.9.6 produced tool-not-found errors with nothing in the
// log to explain them.

/** Writes a fake binary whose `--version` prints the given version. */
function writeFakeBinary(dir, version) {
  const p = path.join(dir, "iris-agentic-dev");
  fs.writeFileSync(p, `#!/bin/sh\necho "iris-agentic-dev ${version}"\n`, {
    mode: 0o755,
  });
  return p;
}

test("tier 2: a PATH binary is used even when its version is stale", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const stale = writeFakeBinary(tmp, "0.0.1");
  whichStub.__setFound(stale);
  vscodeStub.__resetLog();

  try {
    const result = await resolveServerBinary(makeContext("0.9.5", tmp));
    assert.equal(result, stale, "must not override an explicit PATH install");
  } finally {
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("tier 2: a stale PATH binary warns, naming both versions", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const stale = writeFakeBinary(tmp, "0.0.1");
  whichStub.__setFound(stale);
  vscodeStub.__resetLog();

  try {
    await resolveServerBinary(makeContext("0.9.5", tmp));
    const warnings = vscodeStub.__log.warn.join("\n");
    assert.match(
      warnings,
      /0\.0\.1/,
      `the warning must name the version found; got: ${warnings}`
    );
    assert.match(
      warnings,
      /0\.9\.5/,
      `the warning must name the version expected; got: ${warnings}`
    );
  } finally {
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("tier 2: a matching PATH binary does not warn", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const current = writeFakeBinary(tmp, "0.9.5");
  whichStub.__setFound(current);
  vscodeStub.__resetLog();

  try {
    const result = await resolveServerBinary(makeContext("0.9.5", tmp));
    assert.equal(result, current);
    assert.deepEqual(
      vscodeStub.__log.warn,
      [],
      "an up-to-date PATH binary must not produce warning noise"
    );
  } finally {
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("tier 2: an unreadable --version does not break resolution", async () => {
  // Old builds without --version, wrapper scripts, and binaries that exit
  // non-zero must all still resolve. The version check is diagnostic only.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const broken = path.join(tmp, "iris-agentic-dev");
  fs.writeFileSync(broken, "#!/bin/sh\nexit 3\n", { mode: 0o755 });
  whichStub.__setFound(broken);
  vscodeStub.__resetLog();

  try {
    const result = await resolveServerBinary(makeContext("0.9.5", tmp));
    assert.equal(result, broken, "must still use the binary it found");
  } finally {
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test("tier 2: version check cannot hang on a binary that never exits", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const hangs = path.join(tmp, "iris-agentic-dev");
  fs.writeFileSync(hangs, "#!/bin/sh\nsleep 30\n", { mode: 0o755 });
  whichStub.__setFound(hangs);

  const started = Date.now();
  try {
    const result = await resolveServerBinary(makeContext("0.9.5", tmp));
    assert.equal(result, hangs);
    const elapsed = Date.now() - started;
    assert.ok(
      elapsed < 10_000,
      `resolution took ${elapsed}ms — a hung binary must not block activation`
    );
  } finally {
    whichStub.__reset();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

// --- other clients' registrations: found, then offered ----------------------
//
// A Windows user's ~/.claude.json named .../iris-agentic-dev-1.5.0/...exe,
// written once when they registered the server. The extension went on to
// download 1.6.0 and 1.6.3, and Claude Code kept launching 1.5.0. The
// extension now finds such entries, and rewrites them only if the user says
// Update. It never writes another client's config without that.

const { stableBinaryPath } = require("../.test-out/stablePath.cjs");

/** Scripted answers for showInformationMessage; records every call. */
function scriptPrompts(...answers) {
  const calls = [];
  const orig = vscodeStub.window.showInformationMessage;
  vscodeStub.window.showInformationMessage = async (message, ...items) => {
    calls.push({ message, items });
    return items.length ? answers.shift() : undefined;
  };
  return { calls, restore: () => (vscodeStub.window.showInformationMessage = orig) };
}

/** Storage holding the given versions, and a ~/.claude.json pinned to the first. */
function pinnedSetup(versions) {
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  const paths = {};
  for (const v of versions) {
    const dir = path.join(storage, `iris-agentic-dev-${v}`);
    fs.mkdirSync(dir, { recursive: true });
    paths[v] = path.join(dir, "iris-agentic-dev-macos-arm64");
    fs.writeFileSync(paths[v], `#!/bin/sh\necho "iris-agentic-dev ${v}"\n`, { mode: 0o755 });
  }
  const claudeJson = path.join(FAKE_HOME, ".claude.json");
  fs.writeFileSync(
    claudeJson,
    JSON.stringify(
      {
        mcpServers: {
          "iris-agentic-dev": { command: paths[versions[0]], args: ["mcp"] },
          mine: { command: "/opt/iad/iris-agentic-dev", args: ["mcp"] },
        },
      },
      null,
      2
    ) + "\n"
  );
  const stable = stableBinaryPath(storage, process.platform);
  return {
    storage,
    paths,
    claudeJson,
    stable,
    read: () => JSON.parse(fs.readFileSync(claudeJson, "utf8")),
    cleanup: () => {
      fs.rmSync(claudeJson, { force: true });
      fs.rmSync(storage, { recursive: true, force: true });
    },
  };
}

const ASK = /^Found 1 registration\(s\) pointing at an old iris-agentic-dev binary\. Update them to the current one\?$/;
const log = () => vscodeStub.window.createOutputChannel();

test("activation finds a pinned entry, asks, and writes nothing before the answer", async () => {
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const binaryName = getBinaryName(process.platform, process.arch);
  if (!binaryName) return;
  const env = pinnedSetup(["1.5.0"]);
  const dir = path.join(env.storage, "iris-agentic-dev-1.6.4");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, binaryName), "#!/bin/sh\necho 1.6.4\n", { mode: 0o755 });
  fs.writeFileSync(path.join(env.storage, "iris-agentic-dev.version"), "1.6.4");
  const before = fs.readFileSync(env.claudeJson, "utf8");
  const prompts = scriptPrompts(undefined);
  try {
    assert.equal(await resolveServerBinary(makeContext("1.6.4", env.storage)), env.stable);
    await registrationOffer();
    assert.equal(prompts.calls.length, 1);
    assert.match(prompts.calls[0].message, ASK);
    assert.deepEqual(prompts.calls[0].items, ["Update", "Show files", "Not now"]);
    assert.equal(fs.readFileSync(env.claudeJson, "utf8"), before, "dismissed: nothing written");
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("Update repoints the listed entry, leaves the user's own, and says to restart", async () => {
  const env = pinnedSetup(["1.5.0", "1.6.4"]);
  const prompts = scriptPrompts("Update");
  try {
    await offerRegistrationUpdate(makeContext("1.6.4", env.storage), env.storage, env.stable, "1.6.4", log());
    const reg = env.read();
    assert.equal(reg.mcpServers["iris-agentic-dev"].command, env.stable);
    assert.equal(reg.mcpServers.mine.command, "/opt/iad/iris-agentic-dev");
    assert.equal(prompts.calls.length, 2);
    assert.match(prompts.calls[1].message, /Restart/);
    assert.ok(prompts.calls[1].message.includes(env.claudeJson));
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("Show files lists each file and entry and writes nothing", async () => {
  const env = pinnedSetup(["1.5.0"]);
  const before = fs.readFileSync(env.claudeJson, "utf8");
  const prompts = scriptPrompts("Show files", "Not now");
  vscodeStub.__resetLog();
  try {
    await offerRegistrationUpdate(makeContext("1.6.4", env.storage), env.storage, env.stable, "1.6.4", log());
    const info = vscodeStub.__log.info.join("\n");
    assert.ok(info.includes(env.claudeJson), info);
    assert.ok(info.includes("mcpServers.iris-agentic-dev.command"), info);
    assert.ok(info.includes(env.paths["1.5.0"]), info);
    assert.equal(prompts.calls.length, 2, "the question is asked again after the list");
    assert.equal(fs.readFileSync(env.claudeJson, "utf8"), before);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("Not now writes nothing and does not ask again for the same version", async () => {
  const env = pinnedSetup(["1.5.0"]);
  const before = fs.readFileSync(env.claudeJson, "utf8");
  const ctx = makeContext("1.6.4", env.storage);
  const prompts = scriptPrompts("Not now");
  try {
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.4", log());
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.4", log());
    assert.equal(prompts.calls.length, 1, "asked once for 1.6.4");
    assert.equal(fs.readFileSync(env.claudeJson, "utf8"), before);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("dismissing the notification counts as Not now", async () => {
  const env = pinnedSetup(["1.5.0"]);
  const ctx = makeContext("1.6.4", env.storage);
  const prompts = scriptPrompts(undefined);
  try {
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.4", log());
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.4", log());
    assert.equal(prompts.calls.length, 1);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("a newer binary asks again after Not now", async () => {
  const env = pinnedSetup(["1.5.0"]);
  const ctx = makeContext("1.6.4", env.storage);
  const prompts = scriptPrompts("Not now", "Update");
  try {
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.4", log());
    await offerRegistrationUpdate(ctx, env.storage, env.stable, "1.6.5", log());
    assert.equal(prompts.calls.filter((c) => ASK.test(c.message)).length, 2);
    assert.equal(env.read().mcpServers["iris-agentic-dev"].command, env.stable);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("nothing pinned: no question, no write", async () => {
  const env = pinnedSetup(["1.5.0"]);
  fs.writeFileSync(env.claudeJson, '{"mcpServers":{"mine":{"command":"/opt/iad/iris-agentic-dev"}}}');
  const prompts = scriptPrompts();
  try {
    await offerRegistrationUpdate(makeContext("1.6.4", env.storage), env.storage, env.stable, "1.6.4", log());
    assert.equal(prompts.calls.length, 0);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});

test("an accepted update leaves every registration on the newest binary", async () => {
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const binaryName = getBinaryName(process.platform, process.arch);
  if (!binaryName) return;
  const env = pinnedSetup(["1.5.0"]);
  const seed = (v) => {
    const dir = path.join(env.storage, `iris-agentic-dev-${v}`);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, binaryName), `#!/bin/sh\necho "iris-agentic-dev ${v}"\n`, { mode: 0o755 });
    fs.writeFileSync(path.join(env.storage, "iris-agentic-dev.version"), v);
  };
  const prompts = scriptPrompts("Update");
  try {
    seed("1.6.4");
    assert.equal(await resolveServerBinary(makeContext("1.6.4", env.storage)), env.stable);
    await registrationOffer();
    assert.equal(env.read().mcpServers["iris-agentic-dev"].command, env.stable);
    assert.match(fs.readFileSync(env.stable, "utf8"), /1\.6\.4/);

    // The next update changes the binary behind the path, not the path, and
    // there is nothing left to ask about.
    const asked = prompts.calls.length;
    seed("1.6.5");
    assert.equal(await resolveServerBinary(makeContext("1.6.5", env.storage)), env.stable);
    await registrationOffer();
    assert.equal(prompts.calls.length, asked);
    assert.equal(env.read().mcpServers["iris-agentic-dev"].command, env.stable);
    assert.match(fs.readFileSync(env.stable, "utf8"), /1\.6\.5/);
  } finally {
    prompts.restore();
    env.cleanup();
  }
});
