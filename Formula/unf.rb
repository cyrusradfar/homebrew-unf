class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-aarch64-apple-darwin.tar.gz"
      sha256 "e4dd0592118c3c6edb20099b654ebab1f76159965ab39b7087325d1413773c85"
    else
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-x86_64-apple-darwin.tar.gz"
      sha256 "54514e1310b78029c1970e7bc9c55a00f6e99b00db14a8f15993786335649e42"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "8d16eea0b98546727787d56f30bf01921c2c4ea1f0ff40e85cad03142630b2b8"
    else
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "fdf79b1c6613d9b1307422f9cd988686c425579d4f5f772f5e54d6eee4516695"
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
