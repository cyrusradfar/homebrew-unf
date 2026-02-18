class UnfStaging < Formula
  desc "Filesystem flight recorder — staging build for pre-release testing"
  homepage "https://github.com/cyrusradfar/unfudged"
  version "0.16.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/staging-v0.16.2/unf-v0.16.2-aarch64-apple-darwin.tar.gz"
      sha256 "4957ad5052561e810da6124bbbf9aed2082ddf107ad633ed1b5134a78ff4680f"
    else
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/staging-v0.16.2/unf-v0.16.2-x86_64-apple-darwin.tar.gz"
      sha256 "7d6c9b061921f6fc8ebc18517ecee2f58822aa016f1f535c8bc5269c3353e5a8"
    end
  end

  on_linux do
    url "https://github.com/cyrusradfar/homebrew-unf/releases/download/staging-v0.16.2/unf-v0.16.2-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "bb7419514e1aec5b77b0f12abbb204205276cc13e151f00506c072e648e58059"
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