class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.11"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.11/unf-v0.17.11-aarch64-apple-darwin.tar.gz"
      sha256 "9d1d152801d6538a428ee0d525aa9f869ce5471e00417b2e741d492230a34776"
    else
      url "https://downloads.unfudged.io/releases/v0.17.11/unf-v0.17.11-x86_64-apple-darwin.tar.gz"
      sha256 "b2bb30159070f057f7756fb2e75c08b4002e4ec095da0ec85c7554f6deb1dfc0"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.11/unf-v0.17.11-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "57e0f1887cd2341729fc53c5dc3fe026c8de2df3c53c0b79992b888a404a4a89"
    else
      url "https://downloads.unfudged.io/releases/v0.17.11/unf-v0.17.11-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1c932d15efa3b9f9a6562df46042a00e6bec3e2ecfaf01f0f2d110c43805953f"
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
