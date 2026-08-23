class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.4/iris-agentic-dev-macos-arm64"
      sha256 "9bde2d3baf6b70df812e54f4c9f2fc056bea0f5bf34764d0a0a1cdd593da3654"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.4/iris-agentic-dev-macos-x86_64"
      sha256 "84634100a66dd5f01b1f4c0387560bbd7973fad171e4793be7dbaa981aec09bf"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.4/iris-agentic-dev-linux-x86_64"
      sha256 "478ad5fe0d5e3ef6bba463b08fbfeddea6b0bc7e248a642e0e2979d294fd924d"
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
