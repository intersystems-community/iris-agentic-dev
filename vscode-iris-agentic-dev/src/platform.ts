const GITHUB_BASE = "https://github.com/intersystems-community/iris-agentic-dev/releases/download";

// The extension version (0.4.x) and the MCP server binary version (0.9.x) are
// separate sequences: the extension ships on its own cadence, the binary is
// tagged v0.9.x by the Rust release. Deriving the download URL from the
// extension's own version asks for a release tag that does not exist.
export function getServerVersion(packageJSON: {
  irisAgenticDev?: { serverVersion?: string };
}): string {
  const version = packageJSON.irisAgenticDev?.serverVersion;
  if (!version) {
    // Deliberately fatal rather than falling back to the extension version —
    // that fallback is what produced 404s on every auto-install.
    throw new Error(
      "package.json is missing irisAgenticDev.serverVersion — cannot determine " +
        "which MCP server binary to download.",
    );
  }
  return version;
}

// Maps process.platform + process.arch to the binary name segment used in GitHub releases.
export function getBinaryName(platform: string, arch: string): string | null {
  if (platform === "darwin" && arch === "arm64") return "iris-agentic-dev-macos-arm64";
  if (platform === "darwin" && arch === "x64") return "iris-agentic-dev-macos-x86_64";
  if (platform === "linux" && arch === "x64") return "iris-agentic-dev-linux-x86_64";
  if (platform === "linux" && arch === "arm64") return "iris-agentic-dev-linux-aarch64";
  if (platform === "win32" && arch === "x64") return "iris-agentic-dev-windows-x86_64.exe";
  return null;
}

export function getDownloadUrl(version: string, platform: string, arch: string): string | null {
  const name = getBinaryName(platform, arch);
  if (!name) return null;
  return `${GITHUB_BASE}/v${version}/${name}`;
}

// The SHA-256 the extension trusts for a release asset. Nothing in the
// download is trusted on its own: the release workflow builds the binaries,
// then stamps their hashes into package.json before the VSIX is packaged, so
// the hashes travel inside the extension package, not from the GitHub
// release the binary is fetched from. A checkout that was not
// stamped has no pins, and so installs nothing.
const SHA256_HEX = /^[0-9a-f]{64}$/;

export function getAssetSha256(
  packageJSON: { irisAgenticDev?: { assetSha256?: Record<string, unknown> } },
  binaryName: string
): string | null {
  const table = packageJSON.irisAgenticDev?.assetSha256;
  if (!table || typeof table !== "object") return null;
  const value = Object.prototype.hasOwnProperty.call(table, binaryName)
    ? table[binaryName]
    : undefined;
  if (typeof value !== "string") return null;
  const hex = value.toLowerCase();
  return SHA256_HEX.test(hex) ? hex : null;
}
