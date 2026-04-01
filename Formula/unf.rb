class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.2"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.2/unf-v0.18.2-aarch64-apple-darwin.tar.gz"
      sha256 "360524efb35a7660f0d62c600284e70ce4de7f8fd59d6624eef96d0bb66aebb5"
    else
      url "https://downloads.unfudged.io/releases/v0.18.2/unf-v0.18.2-x86_64-apple-darwin.tar.gz"
      sha256 "ea7b61d4255c79e246006adf6d09cdd7bbf23cff5a45041bade175d883f6ecc8"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.2/unf-v0.18.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "8045725c6391902cfafd87e359c04d35dfec3096d7859897db2827dda9419687"
    else
      url "https://downloads.unfudged.io/releases/v0.18.2/unf-v0.18.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0c8185f8b6fabc2d55342669bf6993ecebd0129b33a37fe34b49b14b0cc5f32b"
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
