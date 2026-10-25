"use strict";

// The extension runs the server binary it downloads, so it trusts only a
// SHA-256 pinned inside the extension itself. The release workflow builds the
// binaries first and stamps their hashes into package.json before the VSIX is
// packaged (scripts/stamp-asset-hashes.cjs). These tests cover the reader, the
// stamper, and the wiring between the workflow and both.

const { test } = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync, spawnSync } = require("node:child_process");

const pkg = require("../package.json");
const { getAssetSha256, getBinaryName } = require("../.test-out/platform.cjs");

const STAMP = path.join(__dirname, "..", "scripts", "stamp-asset-hashes.cjs");
const RELEASE_YML = path.join(__dirname, "..", "..", ".github", "workflows", "release.yml");

const ASSETS = [
  "iris-agentic-dev-linux-x86_64",
  "iris-agentic-dev-linux-aarch64",
  "iris-agentic-dev-macos-arm64",
  "iris-agentic-dev-macos-x86_64",
  "iris-agentic-dev-windows-x86_64.exe",
];
const sha = (s) => crypto.createHash("sha256").update(s).digest("hex");

test("the extension can install exactly the assets the release publishes", () => {
  const names = [
    getBinaryName("linux", "x64"),
    getBinaryName("linux", "arm64"),
    getBinaryName("darwin", "arm64"),
    getBinaryName("darwin", "x64"),
    getBinaryName("win32", "x64"),
  ];
  assert.deepEqual([...names].sort(), [...ASSETS].sort());
});

test("getAssetSha256 returns the pinned hash for a platform, lower-cased", () => {
  const h = "AB".repeat(32);
  const json = { irisAgenticDev: { assetSha256: { "iris-agentic-dev-macos-arm64": h } } };
  assert.equal(getAssetSha256(json, "iris-agentic-dev-macos-arm64"), h.toLowerCase());
});

test("getAssetSha256 returns null when there is no usable pin", () => {
  const bad = "z".repeat(64);
  assert.equal(getAssetSha256({}, "x"), null);
  assert.equal(getAssetSha256({ irisAgenticDev: {} }, "x"), null);
  assert.equal(getAssetSha256({ irisAgenticDev: { assetSha256: {} } }, "x"), null);
  assert.equal(getAssetSha256({ irisAgenticDev: { assetSha256: { x: bad } } }, "x"), null);
  assert.equal(getAssetSha256({ irisAgenticDev: { assetSha256: { x: "ab" } } }, "x"), null);
  assert.equal(getAssetSha256({ irisAgenticDev: { assetSha256: { x: 7 } } }, "x"), null);
  // A pin for another platform is not a pin for this one.
  assert.equal(
    getAssetSha256({ irisAgenticDev: { assetSha256: { y: sha("y") } } }, "x"),
    null
  );
});

test("the committed package.json declares the pin table (empty until a release stamps it)", () => {
  assert.equal(typeof pkg.irisAgenticDev.assetSha256, "object");
  for (const [name, h] of Object.entries(pkg.irisAgenticDev.assetSha256)) {
    assert.ok(ASSETS.includes(name), `unknown asset ${name}`);
    assert.match(h, /^[0-9a-f]{64}$/);
  }
});

function stageAssets(dir, names = ASSETS) {
  for (const n of names) fs.writeFileSync(path.join(dir, n), `binary for ${n}`);
}

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "iad-stamp-"));
  const assets = path.join(root, "assets");
  fs.mkdirSync(assets);
  const pkgPath = path.join(root, "package.json");
  fs.writeFileSync(
    pkgPath,
    JSON.stringify({ name: "x", irisAgenticDev: { serverVersion: "1.2.3", assetSha256: {} } }, null, 2) + "\n"
  );
  return { root, assets, pkgPath };
}

test("the stamper writes one SHA-256 per release asset and keeps the rest of package.json", () => {
  const { root, assets, pkgPath } = fixture();
  try {
    stageAssets(assets);
    execFileSync(process.execPath, [STAMP, assets, pkgPath]);
    const out = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
    assert.equal(out.name, "x");
    assert.equal(out.irisAgenticDev.serverVersion, "1.2.3");
    assert.deepEqual(Object.keys(out.irisAgenticDev.assetSha256).sort(), [...ASSETS].sort());
    for (const n of ASSETS) {
      assert.equal(out.irisAgenticDev.assetSha256[n], sha(`binary for ${n}`));
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("the stamper fails, and writes nothing, when an asset is missing", () => {
  const { root, assets, pkgPath } = fixture();
  try {
    stageAssets(assets, ASSETS.slice(1));
    const before = fs.readFileSync(pkgPath, "utf8");
    const r = spawnSync(process.execPath, [STAMP, assets, pkgPath], { encoding: "utf8" });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, new RegExp(ASSETS[0]));
    assert.equal(fs.readFileSync(pkgPath, "utf8"), before);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("the stamper fails when an asset is empty", () => {
  const { root, assets, pkgPath } = fixture();
  try {
    stageAssets(assets);
    fs.writeFileSync(path.join(assets, ASSETS[2]), "");
    const r = spawnSync(process.execPath, [STAMP, assets, pkgPath], { encoding: "utf8" });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /empty/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test("release.yml stamps the hashes from the built binaries before it packages the VSIX", () => {
  const yml = fs.readFileSync(RELEASE_YML, "utf8");
  const job = yml.slice(yml.indexOf("  build-vsix:"), yml.indexOf("\n  sbom:"));
  assert.match(job, /needs:\s*\[?\s*build\b/, "build-vsix must wait for the binaries");
  const download = job.indexOf("actions/download-artifact");
  const stamp = job.indexOf("stamp-asset-hashes.cjs");
  const pkgStep = job.indexOf("npm run package");
  assert.ok(download !== -1 && stamp !== -1 && pkgStep !== -1, "download, stamp and package steps must all exist");
  assert.ok(download < stamp && stamp < pkgStep, "order must be download, stamp, package");
  // npm test (which asserts the stamped table) must run after the stamp too.
  assert.ok(job.indexOf("npm test") > stamp, "tests must see the stamped package.json");
});
