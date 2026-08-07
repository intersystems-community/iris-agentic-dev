class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.0.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.0.0/iris-agentic-dev-macos-arm64"
      sha256 "83b71c7e92acb64adc6e853362c6fdadd1472a3d43cfa1f0a7c58934d02b5473"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.0.0/iris-agentic-dev-macos-x86_64"
      sha256 "e7a4fbaebb810fbf6713034bc5d086ac9ab799f846726937bd53e12e058ab3b5"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.0.0/iris-agentic-dev-linux-x86_64"
      sha256 "b9d45e5459e00198faffe599c1477c2c89049d3014b428d341d87391bd6b23da"
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
