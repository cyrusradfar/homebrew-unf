class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.16"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-apple-darwin.tar.gz"
      sha256 "82be365a15f1f67dc6c479fcdd09c893a32b946ec617f84f5bba249afdecbb99"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-apple-darwin.tar.gz"
      sha256 "b8b686a629f165e8b440359ad2e2f0748a06759045eded5908de6e6841910b47"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "610ff901b14fc78aab85431592b1674bd173846b1b0a27c62bd8351c7961ebfe"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "2b9d7769e2a35bfa63a5d432536af442537d149df4c90c6d29ad5d832869e1b1"
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
