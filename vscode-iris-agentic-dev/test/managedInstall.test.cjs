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

// Build managedInstall.ts → .test-out/managedInstall.cjs before running tests
// (done in package.json test script)
const { resolveServerBinary } = require("../.test-out/managedInstall.cjs");

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

  assert.equal(result, binaryPath);
  fs.rmSync(tmp, { recursive: true, force: true });
});

test("US1: unsupported platform/arch → returns null (no PATH, no setting)", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "iad-test-"));
  // We can't override process.platform, so we test via getBinaryName returning null.
  // On supported platforms, this test validates the null-safety of the fallback chain
  // for unsupported arch. We test getBinaryName(platform, 'arm64') on linux directly.
  const { getBinaryName } = require("../.test-out/platform.cjs");
  const name = getBinaryName("linux", "arm64");
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
  fs.rmSync(tmp, { recursive: true, force: true });

  assert.equal(result, binaryPath);
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
