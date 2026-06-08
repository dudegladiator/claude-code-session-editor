class CcSession < Formula
  desc "Interactive TUI editor for Claude Code session JSONL files"
  homepage "https://github.com/dudegladiator/claude-code-session-editor"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-aarch64-apple-darwin.tar.gz"
      sha256 "59bfd6d3c28553544403e48ba29537b00eb87adf7f993385d01fd85161214485"
    end
    on_intel do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-apple-darwin.tar.gz"
      sha256 "dcc76b06139e053ea3b28b2c4a2ffff19a78517b3055d1578f163152b10d83f4"
    end
  end

  on_linux do
    url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "8500288377b7122bb08d3b01ad34620932c9197682eb38f4c7fbd57646373d44"
  end

  def install
    bin.install Dir["cc-session-*"].first => "cc-session"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cc-session --version")
  end
end
