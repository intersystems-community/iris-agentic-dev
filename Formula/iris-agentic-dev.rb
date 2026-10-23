class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.6.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.4/iris-agentic-dev-macos-arm64"
      sha256 "9dd6dd045e551532275e86223c2f669aa3e7931af1f7ac55205c5c7f2faad43d"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.4/iris-agentic-dev-macos-x86_64"
      sha256 "9b9f030dbb8be61bbb807e81aacf57c400041a0e17afa1cceede08b0656dbd85"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.4/iris-agentic-dev-linux-aarch64"
      sha256 "6859ce08869d42f706a2ad883fc862e942952758bfaa0eff7ee05128988c8e75"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.4/iris-agentic-dev-linux-x86_64"
      sha256 "2df18d2a73888d783b6099f9064f209910a832d9e5f57e677cf6842c5a26866a"
    end
  end

  def install
    bin_name = "iris-agentic-dev-macos-arm64"
    bin_name = "iris-agentic-dev-macos-x86_64" if Hardware::CPU.intel? && OS.mac?
    bin_name = "iris-agentic-dev-linux-aarch64" if OS.linux? && Hardware::CPU.arm?
    bin_name = "iris-agentic-dev-linux-x86_64" if OS.linux? && Hardware::CPU.intel?
    bin.install bin_name => "iris-agentic-dev"
  end

  test do
    assert_match "iris-agentic-dev #{version}", shell_output("#{bin}/iris-agentic-dev --version")
  end
end
