class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.2/unf-v0.21.2-aarch64-apple-darwin.tar.gz"
      sha256 "27dc5cb5d91c323cefbf30b256c30a9f6ba78adcfb2270fb2c5ebbcc02a711a3"
    else
      url "https://downloads.unfudged.io/releases/v0.21.2/unf-v0.21.2-x86_64-apple-darwin.tar.gz"
      sha256 "61642e56f066e9313f43f8471af33092ed2bc1f8099021419178b62892659135"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.2/unf-v0.21.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e52865e686616b7b12cfe1b8e9edd413ad279d2538d4638fc5b5663e4b4d1327"
    else
      url "https://downloads.unfudged.io/releases/v0.21.2/unf-v0.21.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0f85d579d4829e3d551c3b798602a0ab79d1f4a657c3a1cc49074a820ed62880"
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
