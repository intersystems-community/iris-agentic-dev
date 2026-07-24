const GITHUB_BASE =
  "https://github.com/intersystems-community/iris-agentic-dev/releases/download";

// Maps process.platform + process.arch to the binary name segment used in GitHub releases.
// Returns null for unsupported combinations (e.g. linux/arm64).
export function getBinaryName(platform: string, arch: string): string | null {
  if (platform === "darwin" && arch === "arm64")
    return "iris-agentic-dev-macos-arm64";
  if (platform === "darwin" && arch === "x64")
    return "iris-agentic-dev-macos-x86_64";
  if (platform === "linux" && arch === "x64")
    return "iris-agentic-dev-linux-x86_64";
  if (platform === "win32" && arch === "x64")
    return "iris-agentic-dev-windows-x86_64.exe";
  return null;
}

export function getDownloadUrl(
  version: string,
  platform: string,
  arch: string
): string | null {
  const name = getBinaryName(platform, arch);
  if (!name) return null;
  return `${GITHUB_BASE}/v${version}/${name}`;
}
