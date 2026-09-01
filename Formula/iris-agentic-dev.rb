class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.8"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.8/iris-agentic-dev-macos-arm64"
      sha256 "8c7ba1c271e14369036b750c54e02893bee184ea972e0b6400c06067c39049b9"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.8/iris-agentic-dev-macos-x86_64"
      sha256 "2e81f6ac0f0ff94ce853cc73de08a6f4e0e43ff8d347055f603a08cf8aea8570"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.8/iris-agentic-dev-linux-x86_64"
      sha256 "ea7d78202b4d2d7949baf0dd9923949b9f2b62b312ebad185df5e263a4851d39"
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
