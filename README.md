# cc-session — Claude Code Session Editor

Interactive terminal UI for browsing, searching, and surgically editing Claude Code session JSONL files.

Claude Code persists conversations as JSONL under `~/.claude/projects/<slug>/<uuid>.jsonl`. Built-ins only support rewind, clear, and compact. `cc-session` fixes that.

## Features

- **Edit your session data to manage context** — surgically delete noise (long tool dumps, stale exploration, leaked secrets) from a Claude Code session before resuming, so you keep useful history without burning context window.
- **Safe deletes** — turn-level auto-pair removes a user prompt with its assistant reply; tool_use ↔ tool_result blocks always travel together. Atomic save with `.bak` backup; refuses to write while Claude Code has the file open (`--force` to override).
- **TUI + scriptable CLI** — interactive ratatui browser with fuzzy search, plus `list / search / show / info / delete / update` subcommands with `--json` output so other agents (Claude Code, Codex, scripts) can drive every action.

## Install

**One-liner (macOS / Linux):**

```sh
curl -fsSL https://get-claude-code-session-editor.harshiitkgp.in/install.sh | sh
```

Detects your OS + arch, downloads the matching prebuilt binary from the latest GitHub release, and drops `cc-session` into `/usr/local/bin` (or `~/.local/bin` if that's not writable). Override location via `CC_SESSION_INSTALL_DIR=...`, version via `CC_SESSION_VERSION=v0.1.0`.

**From source:**

```sh
cargo install cc-session
```

**Homebrew:**

```sh
brew install dudegladiator/claude-code-session-editor/cc-session
```

(This repo doubles as the tap — `Formula/cc-session.rb` lives at the root.)

## Usage

```sh
cc-session            # launch TUI
cc-session --force    # bypass concurrent-open check
cc-session --version
```

### Keys

**List screen**

| Key | Action |
|-----|--------|
| `j` / `k` | move selection |
| `/` | enter search |
| `Esc` | exit search |
| `Enter` | open session |
| `q` | quit |

**Edit screen**

| Key | Action |
|-----|--------|
| `j` / `k` | move selection |
| `t` / `b` | jump top / bottom |
| `d` | toggle delete on current message (auto-pairs tool_use/result) |
| `v` | start visual range selection |
| `s` | save |
| `q` | back to list |

## Non-interactive CLI (LLM-friendly)

Every operation is also available as a subcommand with structured output, so other agents (Claude Code, Codex, scripts, CI) can drive the editor without a human at the terminal.

```sh
cc-session list   [--json] [--project <slug>] [--limit N]
cc-session search <query> [--json] [--limit N]
cc-session show   <id-or-path> [--json] [--full] [--include-hidden]
cc-session info   <id-or-path> [--json]
cc-session delete <id-or-path> --indices 3,5,7 [--from-top N] [--from-bottom N] [--range lo..hi] [--dry-run] [--force] [--json]
cc-session update [--version v0.2.0]
```
`<id-or-path>` accepts a full path, a session UUID, or a unique substring of one. Indices are 0-based positions in the raw JSONL (use `cc-session show --json` to map text → index). Auto-pair always extends the delete set to keep `tool_use`/`tool_result` blocks together; `paired_added` in the output reports what was added.

### Example LLM workflow

```sh
# 1. Pick a session.
cc-session list --json --limit 5

# 2. Inspect messages.
cc-session show 20042ea8 --json

# 3. Preview the edit.
cc-session delete 20042ea8 --indices 4,6 --dry-run --json

# 4. Apply.
cc-session delete 20042ea8 --indices 4,6 --json
```

The `delete` JSON output names every key an agent needs: `requested`, `paired_added`, `after_auto_pair`, `total_messages_before`, `total_messages_after`, `dry_run`, `saved`, `backup`, `warnings`. Pair this with `--dry-run` to plan, then drop the flag to apply.

## Safety

- Always closes Claude Code first. `cc-session` detects open file handles via `lsof` and refuses to save unless `--force` is passed.
- Every save writes `<file>.bak` first.
- Saves are atomic: write to `.tmp`, fsync, rename.
- Tool_use and tool_result blocks always delete together.

## License

MIT or Apache-2.0, at your option.
