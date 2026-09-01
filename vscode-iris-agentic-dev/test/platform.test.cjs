"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");

// We test the compiled JS output since esbuild bundles to CJS.
// Build platform.ts → .test-out/platform.cjs before running these tests.
const { getBinaryName, getDownloadUrl } = require("../.test-out/platform.cjs");

// --- getBinaryName ---

test("darwin/arm64 → macos-arm64", () => {
  assert.equal(getBinaryName("darwin", "arm64"), "iris-agentic-dev-macos-arm64");
});

test("darwin/x64 → macos-x86_64", () => {
  assert.equal(getBinaryName("darwin", "x64"), "iris-agentic-dev-macos-x86_64");
});

test("linux/x64 → linux-x86_64", () => {
  assert.equal(getBinaryName("linux", "x64"), "iris-agentic-dev-linux-x86_64");
});

test("win32/x64 → windows-x86_64.exe", () => {
  assert.equal(
    getBinaryName("win32", "x64"),
    "iris-agentic-dev-windows-x86_64.exe"
  );
});

test("linux/arm64 → linux-aarch64", () => {
  assert.equal(
    getBinaryName("linux", "arm64"),
    "iris-agentic-dev-linux-aarch64"
  );
});

test("win32/arm64 → null (unsupported)", () => {
  assert.equal(getBinaryName("win32", "arm64"), null);
});

// The release matrix publishes exactly these five assets. A name that matches
// none of them is a 404 at auto-install time, which is the failure this module
// exists to prevent — and the reason the linux/arm64 tests above went stale
// without anything catching it.
test("every non-null binary name is a published release asset", () => {
  const published = new Set([
    "iris-agentic-dev-linux-x86_64",
    "iris-agentic-dev-linux-aarch64",
    "iris-agentic-dev-macos-arm64",
    "iris-agentic-dev-macos-x86_64",
    "iris-agentic-dev-windows-x86_64.exe",
  ]);
  for (const platform of ["darwin", "linux", "win32"]) {
    for (const arch of ["x64", "arm64"]) {
      const name = getBinaryName(platform, arch);
      if (name !== null) {
        assert.ok(
          published.has(name),
          `${platform}/${arch} → ${name} is not a published asset`
        );
      }
    }
  }
});

// --- getDownloadUrl ---

test("getDownloadUrl darwin/arm64 builds correct URL", () => {
  const url = getDownloadUrl("0.9.5", "darwin", "arm64");
  assert.equal(
    url,
    "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v0.9.5/iris-agentic-dev-macos-arm64"
  );
});

test("getDownloadUrl win32/x64 includes .exe", () => {
  const url = getDownloadUrl("0.9.5", "win32", "x64");
  assert.equal(
    url,
    "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v0.9.5/iris-agentic-dev-windows-x86_64.exe"
  );
});

test("getDownloadUrl linux/arm64 builds correct URL", () => {
  const url = getDownloadUrl("0.9.5", "linux", "arm64");
  assert.equal(
    url,
    "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v0.9.5/iris-agentic-dev-linux-aarch64"
  );
});

test("getDownloadUrl unsupported platform → null", () => {
  assert.equal(getDownloadUrl("0.9.5", "win32", "arm64"), null);
});
