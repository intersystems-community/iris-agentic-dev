class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.1/iris-agentic-dev-macos-arm64"
      sha256 "0b07ff81bd594e318506a13bafc0fde2d473eec08682beb4f762bc096ef7aa72"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.1/iris-agentic-dev-macos-x86_64"
      sha256 "98f5c500a16c6f57125b195a6fee78cd2380f77a40ad7572902b10d8f137a55a"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.1/iris-agentic-dev-linux-x86_64"
      sha256 "eacfbafed86ff723ebbcd99fff2797c19304957eb93fd8b4948220aa72dddf3d"
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
