class UnfStaging < Formula
  desc "Filesystem flight recorder — staging build for pre-release testing"
  homepage "https://unfudged.io"
  version "0.17.11"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/v0.17.11/unf-v0.17.11-aarch64-apple-darwin.tar.gz"
      sha256 "9d1d152801d6538a428ee0d525aa9f869ce5471e00417b2e741d492230a34776"
    else
      url "https://downloads.unfudged.io/staging/v0.17.11/unf-v0.17.11-x86_64-apple-darwin.tar.gz"
      sha256 "b2bb30159070f057f7756fb2e75c08b4002e4ec095da0ec85c7554f6deb1dfc0"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/v0.17.11/unf-v0.17.11-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "1c932d15efa3b9f9a6562df46042a00e6bec3e2ecfaf01f0f2d110c43805953f_ARM"
    else
      url "https://downloads.unfudged.io/staging/v0.17.11/unf-v0.17.11-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1c932d15efa3b9f9a6562df46042a00e6bec3e2ecfaf01f0f2d110c43805953f"
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
