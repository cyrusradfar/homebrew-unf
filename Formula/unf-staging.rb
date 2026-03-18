class UnfStaging < Formula
  desc "Filesystem flight recorder — staging build for pre-release testing"
  homepage "https://unfudged.io"
  version "0.17.10"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/v0.17.10/unf-v0.17.10-aarch64-apple-darwin.tar.gz"
      sha256 "b9204e7b417fdb1fec993ecd930c51c31d2822f56ec6b3b85439ca9dc9c83062"
    else
      url "https://downloads.unfudged.io/staging/v0.17.10/unf-v0.17.10-x86_64-apple-darwin.tar.gz"
      sha256 "17062d04d722a302cd00b326800f35c53b52456d4e791d30dc352dbf835a3989"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/staging/v0.17.10/unf-v0.17.10-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "acdd6d59b262a6cf01fa24bd5751af58cc590e8e97c99e890ef3066ec2e46fef_ARM"
    else
      url "https://downloads.unfudged.io/staging/v0.17.10/unf-v0.17.10-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "acdd6d59b262a6cf01fa24bd5751af58cc590e8e97c99e890ef3066ec2e46fef"
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
