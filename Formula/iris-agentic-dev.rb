class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.5/iris-agentic-dev-macos-arm64"
      sha256 "be1535dff59193b52938640d96c6bf65b5ff731388647011e7db8753457ff792"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.5/iris-agentic-dev-macos-x86_64"
      sha256 "f8eefe14312d63fd67a301aec209809371d0fe0ca2804a9c37cbe0824267fd4f"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.5/iris-agentic-dev-linux-x86_64"
      sha256 "94e9ed8e433369b4acb740182b1a835fb800e9e6e6dd96c09fbef339690458c5"
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
