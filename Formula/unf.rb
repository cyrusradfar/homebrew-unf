class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.20.0/unf-v0.20.0-aarch64-apple-darwin.tar.gz"
      sha256 "b1bc694bdd9f59594e427cd82ce14e035fc6cbbde59639932f359d46520653c3"
    else
      url "https://downloads.unfudged.io/releases/v0.20.0/unf-v0.20.0-x86_64-apple-darwin.tar.gz"
      sha256 "2f2227ef3cb65151d02e98a67ae37c489925253dd7c4ba8cbc55a5bde9d5c7c4"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.20.0/unf-v0.20.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "fa8a507dacd7f9f82900dfc2b7f43ab8ea39b3589c650e9edcad18fd35e7faaf"
    else
      url "https://downloads.unfudged.io/releases/v0.20.0/unf-v0.20.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "13d213c4fcf1a7e73dcacb2b357c4f8df8404da1afa7dae3090a66512d64c1e0"
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
