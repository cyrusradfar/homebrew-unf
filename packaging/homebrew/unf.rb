class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "VERSION_PLACEHOLDER"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_ARM"
    else
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_PLACEHOLDER_X86"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX_ARM"
    else
      url "https://downloads.unfudged.io/releases/vVERSION_PLACEHOLDER/unf-vVERSION_PLACEHOLDER-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_PLACEHOLDER_LINUX"
    end
  end

  def install
    bin.install "unf"
  end

  def post_install
    # Clear the "stopped" marker so the sentinel can start.
    # `unf stop` creates this file; without removing it, the sentinel
    # exits immediately and brew services shows "not running".
    unf_home = Pathname.new(Dir.home)/".unfudged"
    stopped = unf_home/"stopped"
    stopped.delete if stopped.exist?
  end

  service do
    run [opt_bin/"unf", "__sentinel"]
    keep_alive true
    log_path var/"log/unfudged-sentinel.log"
    error_log_path var/"log/unfudged-sentinel.log"
  end

  def caveats
    <<~EOS
      To start the daemon:
        brew services start unf

      To watch a project:
        cd /path/to/project && unf watch

      For the desktop app:
        brew install --cask cyrusradfar/unf/unfudged
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/unf --version")
  end
end
