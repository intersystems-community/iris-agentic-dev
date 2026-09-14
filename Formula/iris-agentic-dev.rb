class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.4.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.2/iris-agentic-dev-macos-arm64"
      sha256 "7c2bbdad1e224c29977956adecf57ce263d1fe6b7005002ef6ec4702d22a3d7f"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.2/iris-agentic-dev-macos-x86_64"
      sha256 "d378c234d863d8234ea7d1f00f9614ddd3f100903c0a577ede5157dcb50e2593"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.2/iris-agentic-dev-linux-aarch64"
      sha256 "675f4c0716dba92cce1118dd720770ffb35110e95666411076ffd6250d1b3d7f"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.4.2/iris-agentic-dev-linux-x86_64"
      sha256 "4742bfc92239bd68bcd8b5c63ccde3cc125ae2bc901d527d14c3df04bfbc3803"
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
