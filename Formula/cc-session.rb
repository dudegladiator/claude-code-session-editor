class CcSession < Formula
  desc "Interactive TUI editor for Claude Code session JSONL files"
  homepage "https://github.com/dudegladiator/claude-code-session-editor"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-aarch64-apple-darwin.tar.gz"
      sha256 "17d8071429f2e9db45819a1bd618f09116e9a7af8a4f69a68b349bd6458a7e62"
    end
    on_intel do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-apple-darwin.tar.gz"
      sha256 "c58d11af2dd999c4527d4be42c56c865d3ec939fb3acf80ce9fc524f41023346"
    end
  end

  on_linux do
    url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "85b200eecfe12e963a72d2b3b7d4d92d4bcc9895ad949a4923d4194cc96f307b"
  end

  def install
    bin.install Dir["cc-session-*"].first => "cc-session"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cc-session --version")
  end
end
