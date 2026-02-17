class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://github.com/cyrusradfar/unfudged"
  version "0.14.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/cyrusradfar/unfudged/releases/download/v0.14.0/unf-v0.14.0-aarch64-apple-darwin.tar.gz"
      sha256 "55ff6432d04aad444e24379ad4aa265db3cb65e34da43c9e49450defc055cfef"
    else
      url "https://github.com/cyrusradfar/unfudged/releases/download/v0.14.0/unf-v0.14.0-x86_64-apple-darwin.tar.gz"
      sha256 "944a5aae290c1130623ad3bda18acba34bf07834beaa7e6090848f0f850ecc18"
    end
  end

  on_linux do
    url "https://github.com/cyrusradfar/unfudged/releases/download/v0.14.0/unf-v0.14.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "dc4214eec175b3d3d982240a646cc8030d08c5171d4e22c837b2dc5e12ea77fd"
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
