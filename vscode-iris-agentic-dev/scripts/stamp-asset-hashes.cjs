#!/usr/bin/env node
"use strict";

// Usage: node scripts/stamp-asset-hashes.cjs <assets-dir> [package.json]
//
// Hashes each release binary in <assets-dir> and writes the digests to
// irisAgenticDev.assetSha256 in package.json. The release workflow runs this
// after the binaries are built and before the VSIX is packaged, so the
// extension ships with the hashes it will verify its download against.
// Any missing or empty asset is an error and package.json is left untouched.

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

// Keep in step with getBinaryName in src/platform.ts (a test compares them).
const ASSETS = [
  "iris-agentic-dev-linux-x86_64",
  "iris-agentic-dev-linux-aarch64",
  "iris-agentic-dev-macos-arm64",
  "iris-agentic-dev-macos-x86_64",
  "iris-agentic-dev-windows-x86_64.exe",
];

function main(argv) {
  const dir = argv[0];
  const pkgPath = path.resolve(argv[1] || path.join(__dirname, "..", "package.json"));
  if (!dir) {
    console.error("usage: stamp-asset-hashes.cjs <assets-dir> [package.json]");
    return 2;
  }
  const table = {};
  const problems = [];
  for (const name of ASSETS) {
    const file = path.join(dir, name);
    let bytes;
    try {
      bytes = fs.readFileSync(file);
    } catch {
      problems.push(`${name}: not found in ${dir}`);
      continue;
    }
    if (bytes.length === 0) {
      problems.push(`${name}: file is empty`);
      continue;
    }
    table[name] = crypto.createHash("sha256").update(bytes).digest("hex");
  }
  if (problems.length > 0) {
    console.error("cannot stamp asset hashes:\n  " + problems.join("\n  "));
    return 1;
  }
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  if (!pkg.irisAgenticDev) {
    console.error("package.json has no irisAgenticDev block");
    return 1;
  }
  pkg.irisAgenticDev.assetSha256 = table;
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
  for (const [name, h] of Object.entries(table)) console.log(`${h}  ${name}`);
  return 0;
}

process.exit(main(process.argv.slice(2)));
