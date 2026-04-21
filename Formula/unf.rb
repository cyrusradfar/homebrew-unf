class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.18.5"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.5/unf-v0.18.5-aarch64-apple-darwin.tar.gz"
      sha256 "46cfc1fd606a70743e79a1ab5dbe0997c1b4d9c88f4c0de4afd8acbed7f54a48"
    else
      url "https://downloads.unfudged.io/releases/v0.18.5/unf-v0.18.5-x86_64-apple-darwin.tar.gz"
      sha256 "932e89f84bdac9d3e2a6176aac37ea3d8756e8b6f9e5c795ef66c9c7b57dd93b"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.18.5/unf-v0.18.5-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "3cabf437b43e55a7be0671fe0e92f9d078278033c0091c69774342017b50f783"
    else
      url "https://downloads.unfudged.io/releases/v0.18.5/unf-v0.18.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e2cfc599d247bd354247591e85432c8a930b2245cea9fe41fabba21a26aa0be1"
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
