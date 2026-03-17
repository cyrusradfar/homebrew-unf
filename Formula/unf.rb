class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.8"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.8/unf-v0.17.8-aarch64-apple-darwin.tar.gz"
      sha256 "bec7415a208a61179ef4f08e9a35bc8688afa18de51d250b8d24852cadc9df70"
    else
      url "https://downloads.unfudged.io/releases/v0.17.8/unf-v0.17.8-x86_64-apple-darwin.tar.gz"
      sha256 "61731271c6da6f31d9b7b63fba9de9bf45cbf2a7b09d8a2f21fc3291179ec086"
    end
  end

  on_linux do
    url "https://downloads.unfudged.io/releases/v0.17.8/unf-v0.17.8-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "476d377d8d6c9d0d90475130d0e79079392cc6e9e0f20809d816ced1b58825d2"
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
