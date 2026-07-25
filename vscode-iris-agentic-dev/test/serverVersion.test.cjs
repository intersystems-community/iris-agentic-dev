"use strict";

// The extension version (0.4.x) and the MCP server binary version (0.9.x) are
// separate sequences. managedInstall.ts used to build its download URL from
// packageJSON.version, so it asked GitHub for a release tag like v0.4.25 —
// which does not exist, because binary releases are tagged v0.9.x. Anyone
// without a binary already on PATH got a 404 on first activation, which is
// exactly the auto-install path the extension advertises.
//
// The binary version is now declared explicitly. These tests keep it honest.

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const pkg = require("../package.json");
const { getServerVersion } = require("../.test-out/platform.cjs");

const REPO_ROOT = path.join(__dirname, "..", "..");

function cargoWorkspaceVersion() {
  const toml = fs.readFileSync(path.join(REPO_ROOT, "Cargo.toml"), "utf8");
  // [workspace.package] version = "0.9.5"
  const m = toml.match(/^version\s*=\s*"([^"]+)"/m);
  assert.ok(m, "could not find a version in Cargo.toml");
  return m[1];
}

test("package.json declares an explicit server binary version", () => {
  assert.ok(
    pkg.irisAgenticDev && typeof pkg.irisAgenticDev.serverVersion === "string",
    "package.json must declare irisAgenticDev.serverVersion"
  );
  assert.match(pkg.irisAgenticDev.serverVersion, /^\d+\.\d+\.\d+$/);
});

test("server binary version matches the Rust workspace version", () => {
  // If these drift, the extension downloads a release tag that either does not
  // exist yet or is not the binary this checkout builds.
  assert.equal(
    pkg.irisAgenticDev.serverVersion,
    cargoWorkspaceVersion(),
    "irisAgenticDev.serverVersion must match Cargo.toml — bump both together"
  );
});

test("server binary version is not the extension version", () => {
  assert.notEqual(
    pkg.irisAgenticDev.serverVersion,
    pkg.version,
    "the two version sequences are distinct; using the extension version " +
      "produces a 404 download URL"
  );
});

test("getServerVersion reads the declared binary version, not the extension version", () => {
  assert.equal(getServerVersion(pkg), pkg.irisAgenticDev.serverVersion);
  assert.notEqual(getServerVersion(pkg), pkg.version);
});

test("getServerVersion throws when the binary version is missing", () => {
  // Silently falling back to packageJSON.version is what caused the bug.
  assert.throws(() => getServerVersion({ version: "0.4.25" }), /serverVersion/);
  assert.throws(
    () => getServerVersion({ version: "0.4.25", irisAgenticDev: {} }),
    /serverVersion/
  );
});

test("the declared binary version resolves to a real download URL shape", () => {
  const { getDownloadUrl } = require("../.test-out/platform.cjs");
  const url = getDownloadUrl(pkg.irisAgenticDev.serverVersion, "darwin", "arm64");
  assert.equal(
    url,
    "https://github.com/intersystems-community/iris-agentic-dev/releases/download/" +
      `v${cargoWorkspaceVersion()}/iris-agentic-dev-macos-arm64`
  );
});
