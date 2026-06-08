# cc-session — Claude Code Session Editor

Interactive terminal UI for browsing, searching, and surgically editing Claude Code session JSONL files.

Claude Code persists conversations as JSONL under `~/.claude/projects/<slug>/<uuid>.jsonl`. Built-ins only support rewind, clear, and compact. `cc-session` fixes that.

## Features

- **Session browser** — every session across every project, sorted by recency, with project, title, modified time, size, and full UUID.
- **Fuzzy search** — same engine as fzf/Helix (`nucleo-matcher`), searches project + title, ranks by score.
- **Message viewer** — shows only real user/assistant text by default; system messages, tool blocks, attachments, and harness wrappers (`<bash-input>`, `<system-reminder>`, etc.) are hidden but preserved on disk.
- **Edit a session** — open any session and surgically edit it.
- **Delete individual messages** — pick one and drop it.
- **Delete ranges, from-top, from-bottom** — bulk trim by range, prefix, or suffix.
- **Turn-level auto-pair** — deleting a user message also deletes its assistant response (and vice versa). Marking any message in a turn deletes the whole turn cleanly.
- **tool_use ↔ tool_result safety** — tool calls always travel with their results, so resume never breaks.
- **Token counts per message** — tiktoken `cl100k_base`, with `usage` metadata as fallback when present.
- **Atomic save with `.bak` backup** — write `.tmp`, fsync, rename. Backup written every save.
- **Concurrent-open detection** — `lsof` check refuses to save while Claude Code holds the file. Override with `--force`.
- **LLM-friendly non-interactive CLI** — `list`, `search`, `show`, `info`, `delete` subcommands with `--json` output. Other agents (Claude Code, Codex, scripts) can drive every action a human can.
- **Cross-platform** — macOS and Linux. Windows lsof equivalent deferred.

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
brew tap dudegladiator/tap
brew install cc-session
```

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
