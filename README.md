# cc-session — Claude Code Session Editor

Interactive terminal UI for browsing, searching, and surgically editing Claude Code session JSONL files.

Claude Code persists conversations as JSONL under `~/.claude/projects/<slug>/<uuid>.jsonl`. Built-ins only support rewind, clear, and compact. `cc-session` fixes that.

## Features

- **Edit your session data to manage context** — surgically delete noise (long tool dumps, stale exploration, leaked secrets) from a Claude Code session before resuming, so you keep the useful history without burning context window.
- **Fork-on-delete** — every edit writes a NEW session file (new UUID) and prints a `claude --resume <new-id>` command. The original session file is never modified, so you can revert at any time by resuming the old id.
- **Heatmap** — `cc-session heatmap <id>` ranks conversational turns by true token count (whole-JSONL-line tiktoken — text + tool args + tool stdout + metadata). Drop the heaviest first.
- **Safe deletes** — turn-level auto-pair removes a user prompt with its assistant reply; tool_use ↔ tool_result blocks always travel together; surviving `parentUuid` references re-link to the nearest surviving ancestor so Claude Code's resume renderer never breaks.
- **TUI + scriptable CLI** — interactive ratatui browser with fuzzy search, plus `list / search / show / info / heatmap / delete / update / agent-guide` subcommands with `--json` output so other agents (Claude Code, Codex, scripts) can drive every action.

## Install

**One-liner (macOS / Linux):**

```sh
curl -fsSL https://get-claude-code-session-editor.harshiitkgp.in/install.sh | sh
```

Detects your OS + arch, downloads the matching prebuilt binary from the latest GitHub release, and drops `cc-session` into `/usr/local/bin` (or `~/.local/bin` if that's not writable). Override location via `CC_SESSION_INSTALL_DIR=...`, version via `CC_SESSION_VERSION=v1.0.0`.

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
cc-session --version
```

### TUI Keys

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
| `d` | toggle delete on current message (auto-pairs tool_use/result and turn) |
| `v` | start visual range selection |
| `s` | save (forks to new session) |
| `q` | back to list |

## Non-interactive CLI (LLM-friendly)

Every operation is also a subcommand with structured output, so other agents (Claude Code, Codex, scripts, CI) can drive the editor without a human.

```sh
cc-session list    [--json] [--project <slug>] [--limit N]
cc-session search  <query> [--json] [--limit N]
cc-session show    <id-or-path> [--json] [--full] [--include-hidden]
cc-session info    <id-or-path> [--json]
cc-session heatmap <id-or-path> [--json] [--limit N]
cc-session delete  <id-or-path> --indices 3,5,7 [--from-top N] [--from-bottom N] [--range lo..hi] [--dry-run] [--json]
cc-session update  [--version v1.0.0]
cc-session agent-guide
```

`<id-or-path>` accepts a full path, a session UUID, or a unique substring of one. Indices are 0-based positions in the raw JSONL (use `cc-session show --json` to map text → index).

`delete` always **forks**: it writes a new session file with a new UUID and leaves the original untouched. The output includes `new_session_id`, `new_path`, and a ready-to-paste `resume_command` like `claude --resume <new-id>`. There is no `--force`, no lsof check, no `.bak` file — none of those are needed when the source is never mutated.

## How to actually shrink a session

The pattern that works in practice:

```sh
# 1. Find the session id. Inside Claude Code: /status -> Session ID.
#    Or list everything:
cc-session list --json --limit 10

# 2. See the damage. `info` now reports TRUE wire size (text + tool args
#    + tool stdout + metadata) via tiktoken on whole JSONL lines. Expect
#    numbers 2-3x larger than the old text-only "estimated_tokens".
cc-session info 91e440c0 --json
# -> total_messages: 639   estimated_tokens: 662251

# 3. Locate the heaviest conversational turns. A "turn" = visible user
#    message + every assistant/tool message it triggered until the next
#    visible user message. Heatmap rolls up tool I/O into the parent turn,
#    so you see the real cost of each exchange.
cc-session heatmap 91e440c0 --json --limit 8
# anchor   range   msgs  tokens  preview
#     27   27..65    39  115756  let document the life cycle of each of rpcs of ai agent...
#    128  128..242  115  110922  Base directory for this skill: /Users/harsh/.claude/...
#     66   66..124   59   79176  can you read other ai agent folder...
#    265  265..346   82   62891  please correct the doc

# 4. Pick the contiguous worst block. The four turns above happen to be
#    adjacent (27..346) and are all replaceable by their final saved
#    artifacts. Dry-run first to see what auto-pair pulls in.
cc-session delete 91e440c0 --range 27..346 --dry-run --json
# -> messages: 639 -> 319, parent_uuid_relinked: 1, warnings: []

# 5. Apply. This writes a NEW session file with a fresh UUID. The
#    original file is untouched on disk.
cc-session delete 91e440c0 --range 27..346 --json
# -> {
#      "new_session_id":  "1d9a021d-609f-4d0d-9591-baea02f13195",
#      "new_path":        ".../1d9a021d-...jsonl",
#      "resume_command":  "claude --resume 1d9a021d-...",
#      "total_messages_after":  319,
#      "parent_uuid_relinked":  1
#    }

# 6. Resume the new id in Claude Code.
claude --resume 1d9a021d-609f-4d0d-9591-baea02f13195
```

Real run from this repo's own session: `/context` reported **433k → 230k context** (47% drop, 53% drop on the messages bucket). Cost on the next API call dropped proportionally — the new session id forces a fresh prefix-cache slot, so Claude Code can't accidentally serve the old fat prefix.

Things to know:

- **Why fork instead of mutate?** Two reasons. First, deleting in place leaves Claude Code's prefix cache holding the OLD prefix — your `/context` keeps showing the pre-edit size until you start a new session. Second, if anything goes sideways, you just resume the original id; the source file was never touched.
- **Indices are 0-based positions in the raw JSONL**, not visible-message positions. Use `cc-session show <id> --json` to map text → index. `heatmap` already gives you raw indices in `anchor_idx` / `start_idx` / `end_idx`.
- **Auto-pair always runs.** Marking any message in a turn marks the whole turn (user prompt + assistant reply + tool I/O). tool_use ↔ tool_result blocks always travel together. Surviving messages whose `parentUuid` would point into the deleted set are re-linked to the nearest surviving ancestor (or null at root) — the count surfaces as `parent_uuid_relinked`.
- **You can chain edits.** Each `delete` produces a new session id; pass that id back to `cc-session` to trim further. Forks show `[edited]` in `list` and carry `is_fork: true`, `fork_origin: <parent-id>` in JSON.

`cc-session agent-guide` prints the full machine-readable doc (workflow, JSON shapes, env vars, exit codes) — the canonical contract for other agents.

## Safety

- Source session is **never modified** — every delete writes a new file.
- Atomic writes: `<file>.tmp` → fsync → rename.
- Tool_use and tool_result blocks always delete together.
- Surviving messages whose `parentUuid` would reference a deleted ancestor are re-linked automatically (count surfaced as `parent_uuid_relinked`).
- Forks are tagged with a `cc-session-fork` sentinel line so `list` can show an `[edited]` badge.

## License

MIT or Apache-2.0, at your option.
