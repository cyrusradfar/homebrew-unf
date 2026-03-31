class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.1"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.1/unf-v0.18.1-aarch64-apple-darwin.tar.gz"
      sha256 "c0f0bd35ee1f42642cb6978d97975f0af743d492c4a4d96bef40a97217b3c022"
    else
      url "https://downloads.unfudged.io/releases/v0.18.1/unf-v0.18.1-x86_64-apple-darwin.tar.gz"
      sha256 "4030da0159a87789a857bbc37724ff66be8bac8251788fbccfe5054e74d47a87"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.1/unf-v0.18.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "646ab546e0ca27c38136f74b473e43eca6c2e48756e28a0ac0956cf65a9440e1"
    else
      url "https://downloads.unfudged.io/releases/v0.18.1/unf-v0.18.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "3ee8024cc37e312d77740df7daf746dfe62943c6118f3fcc4968be7ae67a1a52"
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
