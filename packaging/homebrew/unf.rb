class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "VERSION_PLACEHOLDER"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_ARM"
    else
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_X86"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX_ARM"
    else
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX"
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
