class CcSession < Formula
  desc "Interactive TUI editor for Claude Code session JSONL files"
  homepage "https://github.com/dudegladiator/claude-code-session-editor"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-aarch64-apple-darwin.tar.gz"
      sha256 "f3196eaa4772a7507594cfe0e5dc9f52133485c984a936cc05831161fa9a5a49"
    end
    on_intel do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-apple-darwin.tar.gz"
      sha256 "6d72bb91efe29e5b0e2b56b0306c5f22903e7aa73fcaa861d3e477ac16f5d323"
    end
  end

  on_linux do
    url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "d4bf82ddd36fc3247debbfe2b0467da2a786753e21e94e6250c9aed4b08cb6b3"
  end

  def install
    bin.install Dir["cc-session-*"].first => "cc-session"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cc-session --version")
  end
end
