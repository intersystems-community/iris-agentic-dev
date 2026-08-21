class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.3/iris-agentic-dev-macos-arm64"
      sha256 "da2f58eef727187f2a5b698ec0748e7ced80b241f28bc040719684e01d5ed372"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.3/iris-agentic-dev-macos-x86_64"
      sha256 "814f477695d3316effa88d36a8cd92a1359a2774693ea518e576bbc36b0441d4"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.3/iris-agentic-dev-linux-x86_64"
      sha256 "42b99a3eb464279bce6358b44ae87faaba98f78785cfe887543f1579d83f5868"
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
