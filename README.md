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
| `gg` / `G` | jump top / bottom |
| `d` | toggle delete on current message (auto-pairs tool_use/result) |
| `v` | start visual range selection |
| `s` | save |
| `q` | back to list |

## Safety

- Always closes Claude Code first. `cc-session` detects open file handles via `lsof` and refuses to save unless `--force` is passed.
- Every save writes `<file>.bak` first.
- Saves are atomic: write to `.tmp`, fsync, rename.
- Tool_use and tool_result blocks always delete together.

## License

MIT or Apache-2.0, at your option.
