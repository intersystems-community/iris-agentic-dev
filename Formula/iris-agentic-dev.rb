class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.3.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.2/iris-agentic-dev-macos-arm64"
      sha256 "b6dcf7811d96871b8ffe71da66005270c175143b5669426186725159844e7d32"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.2/iris-agentic-dev-macos-x86_64"
      sha256 "3108a1928f67b66aff1eaf4fc87d75b7d569d2b67e54a104fbfb88f1ff42ae5e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.2/iris-agentic-dev-linux-aarch64"
      sha256 "ab904ac5277905445baf9a16cabcb811d4f6883c561799b61f8d9bb594883a0e"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.2/iris-agentic-dev-linux-x86_64"
      sha256 "c4d3ca42c956be3ecbf4e6e03cc494d478f468819b4f7deac21a84f03c6a4ec5"
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
