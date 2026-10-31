class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-aarch64-apple-darwin.tar.gz"
      sha256 "7489daa55d5524436a5e5b5ede7e5d83c416e14d9b397dd22158003dfbda4ae0"
    else
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-x86_64-apple-darwin.tar.gz"
      sha256 "cfa4781d25c3545d57b0a30829b7fb1f5170f21adc97af5def57654511d5539b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "98dd085e07b522a66bbc3e5eace011b67b9ae2666786c067c2c58211e595ef3c"
    else
      url "https://downloads.unfudged.io/releases/v0.21.4/unf-v0.21.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ba05aedcfed7f5b5761a43623e209a9ab744bca92060ada7522dbede1770090c"
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
