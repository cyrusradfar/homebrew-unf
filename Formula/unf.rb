class Unf < Formula
  desc "Filesystem flight recorder — never lose a file change again"
  homepage "https://unfudged.io"
  version "0.17.9"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.9/unf-v0.17.9-aarch64-apple-darwin.tar.gz"
      sha256 "c7227a42d5a935dcce1e786764a2df22e030be2f8edec5f90488598a9187009e"
    else
      url "https://downloads.unfudged.io/releases/v0.17.9/unf-v0.17.9-x86_64-apple-darwin.tar.gz"
      sha256 "3f1651f5fe15faf41e4e16647945161eee431b541b34dacf150b8fccea69cddc"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://downloads.unfudged.io/releases/v0.17.9/unf-v0.17.9-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "48d82ba2d102d826b23c139073023451bbe87571e4e1ee435cba37771683c309_ARM"
    else
      url "https://downloads.unfudged.io/releases/v0.17.9/unf-v0.17.9-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "48d82ba2d102d826b23c139073023451bbe87571e4e1ee435cba37771683c309"
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
