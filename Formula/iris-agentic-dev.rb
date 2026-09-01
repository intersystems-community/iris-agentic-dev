class IrisAgenticDev < Formula
  desc "MCP server connecting AI assistants to InterSystems IRIS — compile, test, debug ObjectScript without leaving the chat"
  homepage "https://github.com/intersystems-community/iris-agentic-dev"
  version "1.2.9"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.9/iris-agentic-dev-macos-arm64"
      sha256 "c67e431dd9a9c3f02c0ccf6c33da627c60ac89d206d7a10479f224a013da832c"
    end
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.9/iris-agentic-dev-macos-x86_64"
      sha256 "7904135561a0ac46aab843f740e4cfb7b0de796cee356e8967f69bd6a36a811c"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/intersystems-community/iris-agentic-dev/releases/download/v1.2.9/iris-agentic-dev-linux-x86_64"
      sha256 "2a1b9e9245d189f3ec12b9c4b90badaa153540aab270a5553367a0dc79ad5eb3"
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
