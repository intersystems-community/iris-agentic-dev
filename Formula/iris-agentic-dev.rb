class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.0/iris-agentic-dev-macos-arm64"
      sha256 "84049e73ab8ef42b76beb840f61b0ad7ac261df1faf8536d64c896bbd4c57f05"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.0/iris-agentic-dev-macos-x86_64"
      sha256 "abb9a8d2f4ba638887184ea7bac709072a08c3322bdac68026a98cb2cd8d11a5"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.0/iris-agentic-dev-linux-x86_64"
      sha256 "0359fbfdc6b3b33f842a5d511b326f0a719ea3422ac0870405a7427d1f456a07"
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
