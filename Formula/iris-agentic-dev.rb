class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.4.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.1/iris-agentic-dev-macos-arm64"
      sha256 "c2b7a9b3963cc947ad2405c86cdac665edf4ce3106353046409a3cd36299ca88"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.1/iris-agentic-dev-macos-x86_64"
      sha256 "3c7c55e7ded893ec9542a4ecac5c940a60ba9ac3f5dd1e4017bcdc4cede0b364"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.1/iris-agentic-dev-linux-aarch64"
      sha256 "1ae704c4926c2f8cbb144ede87ccad697c228990df2259b724dc62339231f0f2"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.1/iris-agentic-dev-linux-x86_64"
      sha256 "eb8188cc0e53313fb87a900ca5f7b82dc3acccbc19d82804d725c259f1775270"
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
