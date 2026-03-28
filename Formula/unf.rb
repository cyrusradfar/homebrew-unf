class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.16"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-apple-darwin.tar.gz"
      sha256 "e565ca47a663808eec2262bcaf6d6e3b5931e6cc0d9acca350b372dd6c1281b4"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-apple-darwin.tar.gz"
      sha256 "b854ec244700bbc634dd32c26b0e89e18e6ccec368c7f4b65ede3461c30fafc9"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "9ad083d8dca8a449d86ac1ecdbf374f3946458566ba02fc9dd3a0db30eae2f0c"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a434988350fcc5575c59dc945bd4eee12645f13d7c4fb6fefbbcb844c03dce3c"
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
