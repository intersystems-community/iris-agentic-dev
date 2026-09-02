class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.0/iris-agentic-dev-macos-arm64"
      sha256 "0a885e46cf7e2d1305559e0a2c515f6ae03bf83a07852addd3f7ae63c7125412"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.0/iris-agentic-dev-macos-x86_64"
      sha256 "d76c143c88d055bae79fcfe62222dd5190d078895b98a7b35e8cdeb9f7613cdf"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.0/iris-agentic-dev-linux-aarch64"
      sha256 "78e12d351016cb93e7a96761ab4ab5b019006b659540ef8c2ef3db6208405c22"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.3.0/iris-agentic-dev-linux-x86_64"
      sha256 "0c1976603dad85bd9af63599f1ab30723402031cd6f08fbbb4d1c3cb0e0e6ccf"
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
