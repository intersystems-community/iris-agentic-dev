class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.7.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.7.0/iris-agentic-dev-macos-arm64"
      sha256 "249eb30ac626e6ca6c34e1f2a597af593fae67889a76165d52975eb2ecedfc0e"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.7.0/iris-agentic-dev-macos-x86_64"
      sha256 "5486da9a66171554537e28b3cbb14a98c97378b963cc594449a9f42843ae0ebd"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.7.0/iris-agentic-dev-linux-aarch64"
      sha256 "850203e135989f66f9406665ab2465b6df6a6d2b5c8a2056b3922cc5d7e6e064"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.7.0/iris-agentic-dev-linux-x86_64"
      sha256 "cf1bb4ad205d2016024a29d8cbb5b75006dab4bf535a74ef775acf60a0a9e361"
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
