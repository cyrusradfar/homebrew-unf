class UnfStaging < Formula
  desc "Filesystem flight recorder — staging build for pre-release testing"
  homepage "https://unfudged.io"
  version "VERSION_PLACEHOLDER"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_ARM"
    else
      url "https://downloads.unfudged.io/staging/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_X86"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX_ARM"
    else
      url "https://downloads.unfudged.io/staging/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX"
    end
  end

  conflicts_with "unf", because: "both install an `unf` binary"

  def install
    bin.install "unf"
  end

  def caveats
    <<~EOS
      Staging build for pre-release testing.
      Conflicts with production `unf` — uninstall one before installing the other.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/unf --version")
  end
end
