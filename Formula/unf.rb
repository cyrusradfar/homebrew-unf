class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.3"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.3/unf-v0.17.3-aarch64-apple-darwin.tar.gz"
      sha256 "52c46aea74498084aeaeec24daef85f71e3c5fc11ecf284273c9c577be2de706"
    else
      url "https://downloads.unfudged.io/releases/v0.17.3/unf-v0.17.3-x86_64-apple-darwin.tar.gz"
      sha256 "f85f7eb878651dc2571cd91d54623f63b1635fd6e133e24b0c27e5690f1098e8"
    end
  end

  on_linux do
    url "https://downloads.unfudged.io/releases/v0.17.3/unf-v0.17.3-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "5cee3aeabb56e9472e89c81980622374b1eb77f22277e4fa84bafc8785aae8d5"
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
