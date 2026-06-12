# cc-session agent guide

You are an LLM driving cc-session non-interactively. This guide is the
single source of truth for how to use it. Read it once, then operate.

## What this CLI does

Edits Claude Code session JSONL files at `~/.claude/projects/<slug>/<uuid>.jsonl`.
It can browse, search, inspect, and surgically delete messages from any session
while keeping `tool_use` / `tool_result` pairs and conversational turns intact.

**Important behavioral note (v1+):** `delete` NEVER mutates the source file.
It always writes a NEW session file with a fresh UUID and prints a
`claude --resume <new-id>` command. The original is never touched. There is
no `--force` flag, no lsof check, no `.bak` file — none of those are needed
when forking.

## Standard workflow

1. Discover sessions:
   ```sh
   cc-session list --json --limit 20
   cc-session search "<query>" --json --limit 10
   ```
2. Inspect one session:
   ```sh
   cc-session info <id-or-path> --json
   cc-session show <id-or-path> --json
   ```
3. Find heaviest turns to drop:
   ```sh
   cc-session heatmap <id-or-path> --json --limit 10
   ```
4. Plan an edit (always dry-run first):
   ```sh
   cc-session delete <id> --indices 4,6 --dry-run --json
   ```
5. Apply (writes a new session file, original untouched):
   ```sh
   cc-session delete <id> --indices 4,6 --json
   ```
   Output includes `new_session_id`, `new_path`, `resume_command`.
6. (Optional) self-update:
   ```sh
   cc-session update [--version v1.0.0]
   ```

## Target argument (`<id-or-path>`)

For `show` / `info` / `heatmap` / `delete` the first positional arg accepts:

- a full filesystem path to a `.jsonl` file
- a full session UUID (preferred — unambiguous)
- any unique substring of a session UUID (8+ chars usually fine)

If a substring matches multiple sessions, the command errors and lists the
candidates. Pass a longer prefix to disambiguate.

## Index semantics

Indices are 0-based positions in the raw JSONL (one per line). Use
`cc-session show --json` to map message text → index. Note:

- "Visible" messages (user / assistant text) are a subset; system messages,
  `tool_use` blocks, `tool_result` blocks, attachments, and harness wrappers
  (`<bash-input>`, `<system-reminder>`, etc.) are hidden by default. Pass
  `--include-hidden` to see them in `show`.
- Indices in the SOURCE session are stable across deletes (because deletes
  fork instead of mutating). Each new fork has its own index space — if you
  chain edits, re-run `show` against the new session id.

## Auto-pair (always on)

Two safety extensions run on every delete request:

1. `tool_use` ↔ `tool_result` blocks always travel together. Marking either
   side pulls the other.
2. Turn-level pairing: a "turn" = visible user msg + every message that
   follows it until the next visible user msg. Marking ANY message in a
   turn marks the whole turn (user prompt + assistant reply + intermediate
   tool calls).

The delete output reports `requested` (what you asked) and `paired_added`
(what auto-pair added). Always inspect both before applying.

## Resume safety: parentUuid auto-relink

Every fork rewrites surviving messages whose `parentUuid` would point to
a deleted ancestor, walking up to the nearest surviving ancestor (or null
at the root). Reported as `parent_uuid_relinked`. Foreign parent uuids
(referring to messages not in the file) are preserved verbatim.

## `delete` output JSON

```json
{
  "source_path":           "<absolute path of input session>",
  "new_session_id":        "<uuid>",
  "new_path":              "<absolute path of fork>",
  "resume_command":        "claude --resume <uuid>",
  "parent_uuid_relinked":  0,
  "requested":             [],
  "after_auto_pair":       [],
  "paired_added":          [],
  "total_messages_before": 0,
  "total_messages_after":  0,
  "dry_run":               false,
  "saved":                 true,
  "warnings":              []
}
```

`new_session_id`, `new_path`, `resume_command` are populated even in dry-run
(preview values). `saved` is `true` when the fork file was actually written.

## `show` output JSON (per message)

```json
{
  "index":            0,
  "role":             "user",
  "type":             "user",
  "timestamp":        "2026-06-12T00:00:00Z",
  "tokens":           0,
  "visible":          true,
  "has_tool_use":     false,
  "has_tool_result":  false,
  "tool_use_ids":     [],
  "tool_result_ids":  [],
  "text":             "...",
  "truncated":        false
}
```

`tokens` is tiktoken `cl100k_base` counted on the WHOLE raw JSONL line
(text + `tool_use` input + `tool_result` content + metadata). `text` is a
400-char preview by default; pass `--full` to get the full body.

## `info` output JSON

```json
{
  "path": "...", "project": "...", "session_id": "...", "title": "...",
  "modified": "...", "size": 0,
  "is_fork": false,
  "fork_origin": null,
  "total_messages": 0, "visible_messages": 0,
  "user_messages": 0, "assistant_messages": 0,
  "tool_use_count": 0, "tool_result_count": 0,
  "orphan_result_indices": [],
  "estimated_tokens": 0
}
```

`estimated_tokens` is the sum of true per-msg counts.

## `heatmap` output JSON

```json
{
  "path":           "<absolute path>",
  "session_id":     "<uuid>",
  "total_messages": 0,
  "total_tokens":   0,
  "turns": [
    {
      "anchor_idx":   0,
      "start_idx":    0,
      "end_idx":      0,
      "msg_count":    0,
      "tokens":       0,
      "has_tool_use": false,
      "preview":      "..."
    }
  ]
}
```

`turns` is sorted by `tokens` descending. Drop the heaviest first.

## `list` / `search` output JSON (per entry)

```json
{
  "project": "...", "session_id": "...", "title": "...",
  "modified": "...", "size": 0, "path": "...",
  "is_fork": false,
  "fork_origin": null
}
```

`title` carries an `[edited] ` prefix when `is_fork` is true.

## Selection flags for `delete`

You may combine any/all; the union is taken before auto-pair runs.

```
--indices 3,5,7        # exact indices (comma-separated)
--range lo..hi         # inclusive range, both ints
--from-top N           # first N messages
--from-bottom N        # last N messages
```

At least one selection flag is required.

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | success |
| `1` | generic error (parse failure, IO error, ambiguous target, ...) |
| `2+` | reserved for future structured errors |

Always inspect stderr on non-zero exit for the human-readable cause.

## Environment overrides

| Var | Effect |
|-----|--------|
| `CC_SESSION_VERSION` | pin a specific release (used by `update`) |
| `CC_SESSION_INSTALL_DIR` | where `install.sh` drops the binary |
| `CC_SESSION_INSTALLER_URL` | override installer URL for `update` (testing) |

## End-to-end recipe (real run, copy this shape)

```sh
# 1. Locate the session id. Inside Claude Code: /status -> Session ID.
cc-session list --json --limit 10

# 2. See the wire size. estimated_tokens reflects whole-line tiktoken
#    (text + tool_use args + tool_result content + metadata) — usually
#    2-3x larger than what plain message text would suggest.
cc-session info <id> --json

# 3. Find the heaviest CONVERSATIONAL TURNS. A turn rolls up the user
#    prompt + every assistant/tool message it triggered up to the next
#    visible user prompt. This matches what auto-pair will delete.
cc-session heatmap <id> --json --limit 10

# 4. Pick a contiguous block of turns that are clearly noise (long
#    iteration loops, exploratory tool dumps, repeated re-reviews of
#    the same doc, etc). Prefer one --range over many --indices: it's
#    less likely to leave parentUuid orphans, and even when it does,
#    auto-relink fixes them (and reports parent_uuid_relinked).
cc-session delete <id> --range <lo>..<hi> --dry-run --json

# 5. Apply. The output's `resume_command` is ready to paste.
cc-session delete <id> --range <lo>..<hi> --json

# 6. Resume the NEW id in Claude Code:
#    claude --resume <new_session_id>
#
#    The new id forces a fresh prefix-cache slot, so Claude Code's
#    /context immediately reflects the smaller size — no stale cache.
```

## Useful examples (one-liners)

```sh
# find the heaviest turns and drop the worst three
cc-session heatmap <id> --json --limit 5
cc-session delete <id> --indices <a>,<b>,<c> --dry-run --json
cc-session delete <id> --indices <a>,<b>,<c> --json

# delete top 50 messages of a long session, dry run first
cc-session delete <id> --from-top 50 --dry-run --json

# purge messages 200..280 inclusive
cc-session delete <id> --range 200..280 --dry-run --json

# find a session about "auth middleware" and inspect
cc-session search "auth middleware" --json --limit 1
cc-session show <id-from-above> --json

# chain edits: each delete produces a new id; pass that id back to
# cc-session for the next trim. Forks are marked is_fork=true in list.
cc-session delete <new_id_from_step_5> --range ... --json
```

## Things this CLI will NOT do

- Edit message contents in place.
- Reorder messages.
- Merge or split sessions.
- Mutate the source session (every delete forks).
