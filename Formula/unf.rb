class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.0/unf-v0.21.0-aarch64-apple-darwin.tar.gz"
      sha256 "22edb6df94541e4775e5df39276d48855bb16d0c941ead4198ec1368eb5fae19"
    else
      url "https://downloads.unfudged.io/releases/v0.21.0/unf-v0.21.0-x86_64-apple-darwin.tar.gz"
      sha256 "34e262ead4dffa77d3c5a35d209fdf8ac09321a0903b66d373cc6e949fecdf98"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.0/unf-v0.21.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "4617450feeb3101f1a664b287c6ec20bf772099bef3d4971f95d167d02fbcbbc"
    else
      url "https://downloads.unfudged.io/releases/v0.21.0/unf-v0.21.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "dcd93881834a5611b3df8b4c9b056628daf58a3da5da8f272dc40df6a011fc16"
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
