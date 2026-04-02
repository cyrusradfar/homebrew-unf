class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.3"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.3/unf-v0.18.3-aarch64-apple-darwin.tar.gz"
      sha256 "059fd21cb028be17706c63edfe532851d7f551ae77f3e2b431aa50f59744f025"
    else
      url "https://downloads.unfudged.io/releases/v0.18.3/unf-v0.18.3-x86_64-apple-darwin.tar.gz"
      sha256 "037f17a2f88563cc3151d5de603acbffcae847ad5e5303538f3b395b8b22978c"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.3/unf-v0.18.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "dd595fc64f59d0d37c80508b0671d0e7866209007a84393b9acf93bd6816875f"
    else
      url "https://downloads.unfudged.io/releases/v0.18.3/unf-v0.18.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "081c84a0d32abaf8347011af064ebf5582895f8394d5a5f3d405aa9e9a1dce7b"
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
