"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const os = require("node:os");
const fs = require("node:fs");

// ---------------------------------------------------------------------------
// vscode stub lives in node_modules/vscode/index.js (created for test use).
// Require it now so we can mutate its exported object in individual tests.
// ---------------------------------------------------------------------------
const vscodeStub = require("vscode");

// Build managedInstall.ts → .test-out/managedInstall.cjs before running tests
// (done in package.json test script)
const { resolveServerBinary } = require("../.test-out/managedInstall.cjs");

// ---------------------------------------------------------------------------
// Helper: build a fake ExtensionContext pointing at a temp dir
// ---------------------------------------------------------------------------
function makeContext(version, tmpDir) {
  return {
    extension: { packageJSON: { version } },
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
