class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.3/unf-v0.21.3-aarch64-apple-darwin.tar.gz"
      sha256 "7e3c9f5752ee2bf29bc813243636b957bc4b275b612eb8157642a0bf0fc8efd8"
    else
      url "https://downloads.unfudged.io/releases/v0.21.3/unf-v0.21.3-x86_64-apple-darwin.tar.gz"
      sha256 "e0f860965d040dd665d7e8bb7a118c9409b9e2f0a63e8d5b5ed3266c5814c7aa"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.3/unf-v0.21.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "fa4ebd090ac2fa8f8963d333b61e10479e9270c6146d067ac7145029b62d6298"
    else
      url "https://downloads.unfudged.io/releases/v0.21.3/unf-v0.21.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "01876fa5053c15e20e565c37bc18fe84933b95c3bc0c8e968596df4feddacefb"
    end
  end

  def install
    bin.install "unf"
  end

  def caveats
    <<~EOS
      To start watching a project:
        cd /path/to/project && unf watch

      This automatically installs a LaunchAgent for auto-start on login.
      For the desktop app:
        brew install --cask cyrusradfar/unf/unfudged
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/unf --version")
  end
end
