class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.15"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.15/unf-v0.17.15-aarch64-apple-darwin.tar.gz"
      sha256 "6d499ec827805816fafec3cc1cd8ede307b0a8e2ec6f0a440ccc5140fb91abac"
    else
      url "https://downloads.unfudged.io/releases/v0.17.15/unf-v0.17.15-x86_64-apple-darwin.tar.gz"
      sha256 "29aee4aa5b94733280c4d291636267c75dc910097ede145a51d16a3d6650893f"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.15/unf-v0.17.15-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "2fa9178de988c45a67d997d41a77f0dbadd3bcf70b575f799c0ea10fc3c1247f"
    else
      url "https://downloads.unfudged.io/releases/v0.17.15/unf-v0.17.15-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c6f879476e824ff466e66efe64fb3e3ca75d05a5d7d2153f9bed06acc71e1540"
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
