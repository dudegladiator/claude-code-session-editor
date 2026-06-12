class CcSession < Formula
  desc "Interactive TUI editor for Claude Code session JSONL files"
  homepage "https://github.com/dudegladiator/claude-code-session-editor"
  version "1.0.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-aarch64-apple-darwin.tar.gz"
      sha256 "d0ec8442fb6f63d050320fae82a2247ff41c8d51613ad5101fe1ecd2923963c3"
    end
    on_intel do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-apple-darwin.tar.gz"
      sha256 "b21c807b72152e91bf27c06c8b169266a6a3a03f44bebc8cf342b6fd462b233a"
    end
  end

  on_linux do
    url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "040f60b0030cb16c1fa892016ac7722663e29f59b5ae41172c3f8a9c008e8d5f"
  end

  def install
    bin.install Dir["cc-session-*"].first => "cc-session"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cc-session --version")
  end
end
