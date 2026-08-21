class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.19.1/unf-v0.19.1-aarch64-apple-darwin.tar.gz"
      sha256 "265e7da055289be30c651a8cfdaaae56dcb933ddd3a2dcac9eb6bef21768098a"
    else
      url "https://downloads.unfudged.io/releases/v0.19.1/unf-v0.19.1-x86_64-apple-darwin.tar.gz"
      sha256 "808e230a97ce17a749bdf84e2816d96ca99f721ac9ca14c66822a712e9d6c3c5"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.19.1/unf-v0.19.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "e9e46b8ff67fe1a78fc66899803250770d5e427ffc77037ee09172334f5539e5"
    else
      url "https://downloads.unfudged.io/releases/v0.19.1/unf-v0.19.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "02244cfb228d038af293e25fb1c19ea769c1fd3c7d336484f475c7c9a2a941ac"
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
