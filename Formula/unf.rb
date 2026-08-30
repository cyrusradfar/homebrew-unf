class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.1/unf-v0.21.1-aarch64-apple-darwin.tar.gz"
      sha256 "f3ef5fd52bccdbd66f342a960c2050bb906e63e0d5322299b134a82e9abc8b66"
    else
      url "https://downloads.unfudged.io/releases/v0.21.1/unf-v0.21.1-x86_64-apple-darwin.tar.gz"
      sha256 "d54467cf0ff8da432727ecd61ba9358978329493a59a947ec1aa22554fb45a4f"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.1/unf-v0.21.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "f50a3ee3f49fd2fee1a044e7bdfdcdb315c9d079b0481eae813906df5ca9c063"
    else
      url "https://downloads.unfudged.io/releases/v0.21.1/unf-v0.21.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "3e5cedbbc22f3a0917a8f743c346fda6a3612adf652850051c2c09b43a34e70b"
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
