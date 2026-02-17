class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://github.com/cyrusradfar/unfudged"
  version "0.16.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.16.2/unf-v0.16.2-aarch64-apple-darwin.tar.gz"
      sha256 "928d9920ed3781ec971ef150e0efe900708930867cfcd26caa9a502c4d8b0ef5"
    else
      url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.16.2/unf-v0.16.2-x86_64-apple-darwin.tar.gz"
      sha256 "ec264d46c5abbbbbd1a1e278f2b3240279235f665d86db56ad19b2824377d846"
    end
  end

  on_linux do
    url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.16.2/unf-v0.16.2-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "7c3601180c43d403a5cdfada061fd39d6c40d0732d06d78ba4dbea6f61398791"
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
