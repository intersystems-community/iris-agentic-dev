class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.2/iris-agentic-dev-macos-arm64"
      sha256 "38699a8b2e6eaf40e9d4a4dec2c47a90ca6f9d74f7f371c242c75dc2b6813c17"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.2/iris-agentic-dev-macos-x86_64"
      sha256 "7a14de201320e569324af404ac7ee8cc91b4725dae489f1ec4893ef634f72d5c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.2/iris-agentic-dev-linux-x86_64"
      sha256 "33925fa1b504969e8d8ec174141199887b2c56ffbea7b7f6f85a4d791ca77bd3"
    end
  end

  def install
    bin_name = "iris-agentic-dev-macos-arm64"
    bin_name = "iris-agentic-dev-macos-x86_64" if Hardware::CPU.intel? && OS.mac?
    bin_name = "iris-agentic-dev-linux-x86_64" if OS.linux?
    bin.install bin_name => "iris-agentic-dev"
  end

  test do
    assert_match "iris-agentic-dev #{version}", shell_output("#{bin}/iris-agentic-dev --version")
  end
end
