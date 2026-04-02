class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.4"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.4/unf-v0.18.4-aarch64-apple-darwin.tar.gz"
      sha256 "23ccdccbaad58e5b4b6b6e776b896925879b3e21eb5a0dede80e2c0a1a5404da"
    else
      url "https://downloads.unfudged.io/releases/v0.18.4/unf-v0.18.4-x86_64-apple-darwin.tar.gz"
      sha256 "ad38748cd5489b8b0c34f1a8f1f510ab2d62dff11c20ab9dca695f19bb46064c"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.4/unf-v0.18.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "1f11ad25eacc5d335ac1163fd6db2d8920eca5f8c699ea07416d8167cf876048"
    else
      url "https://downloads.unfudged.io/releases/v0.18.4/unf-v0.18.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "20c541960b47134a5ffd33fed29d6a0ffaa848856e843d16fe95a7328d9b8e24"
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
