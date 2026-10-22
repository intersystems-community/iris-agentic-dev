class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.6.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.3/iris-agentic-dev-macos-arm64"
      sha256 "27ff3cc6ad3ee5c52f57f51dcdd913eba0163d780004922ef5d3248d81ea80e2"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.3/iris-agentic-dev-macos-x86_64"
      sha256 "9a38823a5173d794582a85f160974a5470b3a4eedb34e89f4676bedef74ce376"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.3/iris-agentic-dev-linux-aarch64"
      sha256 "53dd078da247fbb3e800207d69454175eab407b74c70c26ce28c715cca54ea8e"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.6.3/iris-agentic-dev-linux-x86_64"
      sha256 "9777f9f79b3b476297af040e0786ec69fff8f83ff4621a110d544263b560af0c"
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
