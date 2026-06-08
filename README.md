# cc-session — Claude Code Session Editor

Interactive terminal UI for browsing, searching, and surgically editing Claude Code session JSONL files.

Claude Code persists conversations as JSONL under `~/.claude/projects/<slug>/<uuid>.jsonl`. Built-ins only support rewind, clear, and compact. `cc-session` lets you:

- Browse and search every session across all projects.
- Open any session and view messages with role, text, token count, timestamp.
- Delete individual messages, ranges, from-top, or from-bottom.
- Auto-pair tool_use and tool_result blocks so you never orphan one half.
- Save atomically with a `.bak` backup — and refuse to save when Claude Code holds the file open.

## Install

```sh
cargo install ccsession
```

Homebrew tap (forthcoming):

```sh
brew install <tap>/cc-session
# binary installs as `cc-session`
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
cc-session list --json [--project <slug>] [--limit N]
cc-session show <id-or-path> [--json] [--full] [--include-hidden]
cc-session info <id-or-path> [--json]
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
