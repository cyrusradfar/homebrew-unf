class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://github.com/cyrusradfar/unfudged"
  version "0.14.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.14.0/unf-v0.14.0-aarch64-apple-darwin.tar.gz"
      sha256 "1c623f912334f20f7ab94ee699f356a7faddcacda302cd4be8ca67a45691e4bd"
    else
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.14.0/unf-v0.14.0-x86_64-apple-darwin.tar.gz"
      sha256 "48930d800e034c075a7ea332c77bb4517699268f9b539e2b797b5f505ff2de21"
    end
  end

  on_linux do
    url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.14.0/unf-v0.14.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "e4a5a96b16cb582b0fd8a12c72714c2572011abb83423b19068695f04bf5b0cd"
  end

  def install
    bin.install "unf"
  end

  def caveats
    <<~EOS
      To start watching a project:
        cd /path/to/project && unf watch

      This automatically installs a LaunchAgent for auto-start on login.
      For the desktop app, download UNFUDGED from:
        https://github.com/cyrusradfar/unfudged/releases
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/unf --version")
  end
end
