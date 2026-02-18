class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.4"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.4/unf-v0.17.4-aarch64-apple-darwin.tar.gz"
      sha256 "368accb9ea3ba89a88be188657ee5aa6abd415b04d6db60eaaaaec8550d7071f"
    else
      url "https://downloads.unfudged.io/releases/v0.17.4/unf-v0.17.4-x86_64-apple-darwin.tar.gz"
      sha256 "ed28193bdedcd7afe5e6d548225428896530683c09a37e889a0897622f3fbef4"
    end
  end

  on_linux do
    url "https://downloads.unfudged.io/releases/v0.17.4/unf-v0.17.4-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "a76db23c300715baa554621b3673c0ee849184cc0bed56b03197968730c8d53c"
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
