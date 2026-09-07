class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.4.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.0/iris-agentic-dev-macos-arm64"
      sha256 "3cba6e83e18a15936a83dfe6eeaf790d7eca64e0f49bc01d5b9d384cb54f80d6"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.0/iris-agentic-dev-macos-x86_64"
      sha256 "885279ff7424c783fc74b5a062c2e39b1412c5036e0cee0430d7e55a6eb58a7b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.0/iris-agentic-dev-linux-aarch64"
      sha256 "ea294bb96a0dda63c9ad05494365bff32272747c7b0095b690e1bd9ad4548b90"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.0/iris-agentic-dev-linux-x86_64"
      sha256 "e48f17f1e9d61b48cfc8e38761652710c23cb42d8ca454b8e45a7175f747dc9a"
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
