class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.0"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.0/unf-v0.18.0-aarch64-apple-darwin.tar.gz"
      sha256 "89ceb83ff2d0dc86ac2b9e0eb7f7595a96f0c412b93bf2b9310dc21f811a15de"
    else
      url "https://downloads.unfudged.io/releases/v0.18.0/unf-v0.18.0-x86_64-apple-darwin.tar.gz"
      sha256 "a94402a769de05780b6da5fd910ff2276f21a5502afb3be1bd732bf6fbe2aacf"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.0/unf-v0.18.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "fbd1765b9890d64c858bd964c8960007d6f6d94dc9c270db60afb1f89f21e5f7"
    else
      url "https://downloads.unfudged.io/releases/v0.18.0/unf-v0.18.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "3f6425c2a4d15f06b4b8081402277230defa835d18d7ced50d26b8251eddd33a"
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
