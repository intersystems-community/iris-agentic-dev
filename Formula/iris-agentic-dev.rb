class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.7"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.7/iris-agentic-dev-macos-arm64"
      sha256 "691fb91585464d760207b9d15cc02394667d742997340428f2744acbcffa5a6d"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.7/iris-agentic-dev-macos-x86_64"
      sha256 "45ad93b6c580670e8b5acee9a714af7c34f8deabf04c2146efc9126c70d64548"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.7/iris-agentic-dev-linux-x86_64"
      sha256 "55aa1c2e6c74884d007ec73ee0ba4b3bb59fe1622170265ecfbd3df4027440fa"
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
