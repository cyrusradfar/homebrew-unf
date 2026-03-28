class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.16"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-apple-darwin.tar.gz"
      sha256 "dd155696beac6d7a839e507af3f00216e35d45b70a1da759773ffc17e0cbe1e3"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-apple-darwin.tar.gz"
      sha256 "b43ff06eb7f1ba66f679e5fe741e7956fbabf3f0b90c9c48ba02fc1d0e62943b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0b7695e15c78e861573c926860e1217d55ea85e1d9983f3707df31f6e919783b"
    else
      url "https://downloads.unfudged.io/releases/v0.17.16/unf-v0.17.16-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "27f2612dc944d642d62838ee76d06b3a2cbb83fbe48b2a5a7f01ffd5c7bbcf0d"
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
