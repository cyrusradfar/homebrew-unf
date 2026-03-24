class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.12"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.12/unf-v0.17.12-aarch64-apple-darwin.tar.gz"
      sha256 "5132dcc465ac69f53fe034a03aeb54dba9935c96fb0ed8a08f6571e30278eaf6"
    else
      url "https://downloads.unfudged.io/releases/v0.17.12/unf-v0.17.12-x86_64-apple-darwin.tar.gz"
      sha256 "dc24f402325626bbd016176de7aacb75f52027b1c4e8f6845c1784291ac41573"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.12/unf-v0.17.12-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d854ce5ab402b764a974928f5b67e89007d5cbbcc291e436cadc7c6af584bdb8_ARM"
    else
      url "https://downloads.unfudged.io/releases/v0.17.12/unf-v0.17.12-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "d854ce5ab402b764a974928f5b67e89007d5cbbcc291e436cadc7c6af584bdb8"
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
