class CcSession < Formula
  desc "Interactive TUI editor for Claude Code session JSONL files"
  homepage "https://github.com/dudegladiator/claude-code-session-editor"
  version "0.4.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-aarch64-apple-darwin.tar.gz"
      sha256 "daf6420855b3d4a402f21ee15c21957682e99524421b315dfad0f5afb6c16a3e"
    end
    on_intel do
      url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-apple-darwin.tar.gz"
      sha256 "1d8a18ac5c52f565297a0df96a5ac456ccb24752252867cd0a9d7c382f4272d8"
    end
  end

  on_linux do
    url "https://github.com/dudegladiator/claude-code-session-editor/releases/download/v#{version}/cc-session-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "189c5073e5081b93f0c09e13a24e372c0b5fe9ed6cab8366d54a59826f6719a8"
  end

  def install
    bin.install Dir["cc-session-*"].first => "cc-session"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cc-session --version")
  end
end
