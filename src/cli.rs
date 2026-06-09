//! Non-interactive CLI subcommands. Output is designed to be parsed by other
//! agents (Claude Code, Codex, scripts) — every `--json` mode emits a
//! deterministic shape with stable keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::Value;

use crate::io::atomic;
use crate::pairing::PairIndex;
use crate::scan::{self, SessionEntry};
use crate::session::{Message, Session};
use crate::tokens::TokenCounter;

#[derive(Debug)]
pub struct DeleteSpec {
    pub indices: Vec<usize>,
    pub from_top: Option<usize>,
    pub from_bottom: Option<usize>,
    pub range: Option<String>,
}

// ---------- list ----------

#[derive(Serialize)]
struct ListItem {
    project: String,
    session_id: String,
    title: String,
    modified: String,
    size: u64,
    path: String,
}

pub fn list(
    projects_dir: &Path,
    project_filter: Option<&str>,
    json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let mut entries = scan::scan(projects_dir)?;
    if let Some(p) = project_filter {
        let needle = p.to_lowercase();
        entries.retain(|e| e.project_slug.to_lowercase().contains(&needle));
    }
    if let Some(n) = limit {
        entries.truncate(n);
    }

    if json {
        let out: Vec<ListItem> = entries.iter().map(list_item).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "{:<40} {:<50} {:<17} {:<10} id",
            "project", "title", "modified", "size"
        );
        for e in &entries {
            println!(
                "{:<40} {:<50} {:<17} {:<10} {}",
                truncate(&e.project_slug, 40),
                truncate(&e.title, 50),
                format_mtime(e),
                human_size(e.size),
                e.session_id
            );
        }
        println!("\n{} session(s)", entries.len());
    }
    Ok(())
}

pub fn search(projects_dir: &Path, query: &str, limit: Option<usize>, json: bool) -> Result<()> {
    let entries = scan::scan(projects_dir)?;
    let mut hits: Vec<&SessionEntry> = crate::search::fuzzy_filter(&entries, query);
    if let Some(n) = limit {
        hits.truncate(n);
    }
    if json {
        let out: Vec<ListItem> = hits.iter().map(|e| list_item(e)).collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "{:<40} {:<50} {:<17} {:<10} id",
            "project", "title", "modified", "size"
        );
        for e in &hits {
            println!(
                "{:<40} {:<50} {:<17} {:<10} {}",
                truncate(&e.project_slug, 40),
                truncate(&e.title, 50),
                format_mtime(e),
                human_size(e.size),
                e.session_id
            );
        }
        println!("\n{} match(es)", hits.len());
    }
    Ok(())
}

fn list_item(e: &SessionEntry) -> ListItem {
    ListItem {
        project: e.project_slug.clone(),
        session_id: e.session_id.clone(),
        title: e.title.clone(),
        modified: format_mtime(e),
        size: e.size,
        path: e.path.display().to_string(),
    }
}

// ---------- show ----------

#[derive(Serialize)]
struct ShowMessage {
    index: usize,
    role: String,
    r#type: String,
    timestamp: Option<String>,
    tokens: usize,
    visible: bool,
    has_tool_use: bool,
    has_tool_result: bool,
    tool_use_ids: Vec<String>,
    tool_result_ids: Vec<String>,
    text: String,
    truncated: bool,
}

#[derive(Serialize)]
struct ShowOutput {
    path: String,
    session_id: String,
    project: String,
    total_messages: usize,
    visible_messages: usize,
    messages: Vec<ShowMessage>,
}

pub fn show(
    projects_dir: &Path,
    target: &str,
    include_hidden: bool,
    full: bool,
    json: bool,
) -> Result<()> {
    let entry = resolve_target(projects_dir, target)?;
    let session = Session::load(&entry.path)?;
    let pairing = PairIndex::build(&session.messages);
    let tokens = TokenCounter::new();

    let mut visible_count = 0usize;
    let mut messages: Vec<ShowMessage> = Vec::with_capacity(session.messages.len());
    for (idx, msg) in session.messages.iter().enumerate() {
        let visible = is_visible(msg);
        if visible {
            visible_count += 1;
        }
        if !include_hidden && !visible {
            continue;
        }
        let (text, truncated) = render_text(msg, full);
        let role = msg
            .message
            .as_ref()
            .and_then(|b| b.role.as_deref())
            .unwrap_or("")
            .to_string();
        let mtype = msg.r#type.clone().unwrap_or_default();
        let (use_ids, result_ids) = collect_tool_ids(msg);
        messages.push(ShowMessage {
            index: idx,
            role,
            r#type: mtype,
            timestamp: msg.timestamp.clone(),
            tokens: tokens.count(idx, msg),
            visible,
            has_tool_use: !use_ids.is_empty(),
            has_tool_result: !result_ids.is_empty(),
            tool_use_ids: use_ids,
            tool_result_ids: result_ids,
            text,
            truncated,
        });
    }
    let _ = pairing; // pairing index could be exposed too; skip for now.

    if json {
        let out = ShowOutput {
            path: entry.path.display().to_string(),
            session_id: entry.session_id.clone(),
            project: entry.project_slug.clone(),
            total_messages: session.messages.len(),
            visible_messages: visible_count,
            messages,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("path:    {}", entry.path.display());
        println!("project: {}", entry.project_slug);
        println!("id:      {}", entry.session_id);
        println!(
            "messages: {} total, {} visible",
            session.messages.len(),
            visible_count
        );
        println!();
        for m in &messages {
            let badge = if m.visible { "" } else { "[hidden] " };
            let tools = if m.has_tool_use || m.has_tool_result {
                let mut s = String::new();
                if m.has_tool_use {
                    s.push_str(&format!(" tool_use:{}", m.tool_use_ids.join(",")));
                }
                if m.has_tool_result {
                    s.push_str(&format!(" tool_result:{}", m.tool_result_ids.join(",")));
                }
                s
            } else {
                String::new()
            };
            println!(
                "[{idx}] {badge}{role}/{ty}  {tok}t  {ts}{tools}",
                idx = m.index,
                role = if m.role.is_empty() { "?" } else { &m.role },
                ty = if m.r#type.is_empty() { "?" } else { &m.r#type },
                tok = m.tokens,
                ts = m.timestamp.clone().unwrap_or_default(),
            );
            if !m.text.is_empty() {
                for line in m.text.split('\n') {
                    println!("    {line}");
                }
            }
            println!();
        }
    }
    Ok(())
}

// ---------- delete ----------

#[derive(Serialize)]
struct DeleteOutput {
    path: String,
    parent_uuid_relinked: usize,
    backup: Option<String>,
    requested: Vec<usize>,
    after_auto_pair: Vec<usize>,
    paired_added: Vec<usize>,
    total_messages_before: usize,
    total_messages_after: usize,
    dry_run: bool,
    saved: bool,
    warnings: Vec<String>,
}

pub fn delete(
    projects_dir: &Path,
    target: &str,
    spec: DeleteSpec,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<()> {
    let entry = resolve_target(projects_dir, target)?;
    let session = Session::load(&entry.path)?;
    let pairing = PairIndex::build(&session.messages);
    let total = session.messages.len();

    let mut requested: HashSet<usize> = HashSet::new();
    for &i in &spec.indices {
        if i >= total {
            bail!("index {i} out of range (session has {total} messages)");
        }
        requested.insert(i);
    }
    if let Some(n) = spec.from_top {
        for i in 0..n.min(total) {
            requested.insert(i);
        }
    }
    if let Some(n) = spec.from_bottom {
        for i in total.saturating_sub(n)..total {
            requested.insert(i);
        }
    }
    if let Some(r) = &spec.range {
        let (lo, hi) = parse_range(r)?;
        if hi >= total {
            bail!("range upper bound {hi} out of range (session has {total} messages)");
        }
        for i in lo..=hi {
            requested.insert(i);
        }
    }

    if requested.is_empty() {
        bail!("no messages selected; pass --indices, --from-top, --from-bottom, or --range");
    }

    let mut marked = requested.clone();
    let added_count = pairing.auto_pair(&mut marked);

    let mut requested_sorted: Vec<usize> = requested.into_iter().collect();
    requested_sorted.sort_unstable();
    let mut all_sorted: Vec<usize> = marked.iter().copied().collect();
    all_sorted.sort_unstable();
    let mut paired_added: Vec<usize> = all_sorted
        .iter()
        .filter(|i| !requested_sorted.contains(i))
        .copied()
        .collect();
    paired_added.sort_unstable();
    let _ = added_count;

    let mut warnings = Vec::new();
    if !pairing.orphan_results.is_empty() {
        warnings.push(format!(
            "session has {} orphan tool_result(s) at indices {:?}",
            pairing.orphan_results.len(),
            pairing.orphan_results
        ));
    }

    let (content, relinked) = session.render_with_relink(&marked)?;
    let after = total - marked.len();

    let (saved, backup) = if dry_run {
        (false, None)
    } else {
        match atomic::save(&entry.path, &content, force) {
            Ok(out) => (true, Some(out.backup.display().to_string())),
            Err(atomic::SaveError::Conflict) => {
                bail!("file is open by another process; close Claude Code or pass --force");
            }
            Err(atomic::SaveError::Io(e)) => return Err(e.into()),
        }
    };

    let out = DeleteOutput {
        path: entry.path.display().to_string(),
        parent_uuid_relinked: relinked,
        backup,
        requested: requested_sorted,
        after_auto_pair: all_sorted,
        paired_added,
        total_messages_before: total,
        total_messages_after: after,
        dry_run,
        saved,
        warnings,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("path:                {}", out.path);
        println!("requested:           {:?}", out.requested);
        println!("auto-paired added:   {:?}", out.paired_added);
        println!("final delete set:    {:?}", out.after_auto_pair);
        println!(
            "messages: {} -> {} ({} removed)",
            out.total_messages_before,
            out.total_messages_after,
            out.total_messages_before - out.total_messages_after
        );
        println!("parent_uuid relinked: {}", out.parent_uuid_relinked);
        println!("dry_run: {}", out.dry_run);
        println!("saved:   {}", out.saved);
        if let Some(b) = &out.backup {
            println!("backup:  {b}");
        }
        for w in &out.warnings {
            println!("warning: {w}");
        }
    }
    Ok(())
}

// ---------- info ----------

#[derive(Serialize)]
struct InfoOutput {
    path: String,
    project: String,
    session_id: String,
    title: String,
    modified: String,
    size: u64,
    total_messages: usize,
    visible_messages: usize,
    user_messages: usize,
    assistant_messages: usize,
    tool_use_count: usize,
    tool_result_count: usize,
    orphan_result_indices: Vec<usize>,
    estimated_tokens: usize,
}

pub fn info(projects_dir: &Path, target: &str, json: bool) -> Result<()> {
    let entry = resolve_target(projects_dir, target)?;
    let session = Session::load(&entry.path)?;
    let pairing = PairIndex::build(&session.messages);
    let tokens = TokenCounter::new();

    let mut visible = 0usize;
    let mut users = 0usize;
    let mut assistants = 0usize;
    let mut tu = 0usize;
    let mut tr = 0usize;
    let mut total_tokens = 0usize;
    for (i, m) in session.messages.iter().enumerate() {
        if is_visible(m) {
            visible += 1;
        }
        let role = m
            .message
            .as_ref()
            .and_then(|b| b.role.as_deref())
            .unwrap_or("");
        if role == "user" {
            users += 1;
        } else if role == "assistant" {
            assistants += 1;
        }
        let (uids, rids) = collect_tool_ids(m);
        tu += uids.len();
        tr += rids.len();
        total_tokens += tokens.count(i, m);
    }

    let out = InfoOutput {
        path: entry.path.display().to_string(),
        project: entry.project_slug.clone(),
        session_id: entry.session_id.clone(),
        title: entry.title.clone(),
        modified: format_mtime(&entry),
        size: entry.size,
        total_messages: session.messages.len(),
        visible_messages: visible,
        user_messages: users,
        assistant_messages: assistants,
        tool_use_count: tu,
        tool_result_count: tr,
        orphan_result_indices: pairing.orphan_results.clone(),
        estimated_tokens: total_tokens,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("path:              {}", out.path);
        println!("project:           {}", out.project);
        println!("id:                {}", out.session_id);
        println!("title:             {}", out.title);
        println!("modified:          {}", out.modified);
        println!("size:              {}", human_size(out.size));
        println!(
            "messages:          {} total, {} visible ({} user, {} assistant)",
            out.total_messages, out.visible_messages, out.user_messages, out.assistant_messages
        );
        println!(
            "tool calls:        {} use / {} result",
            out.tool_use_count, out.tool_result_count
        );
        if !out.orphan_result_indices.is_empty() {
            println!("orphan results:    {:?}", out.orphan_result_indices);
        }
        println!("estimated tokens:  {}", out.estimated_tokens);
    }
    Ok(())
}

// ---------- restore ----------

#[derive(Serialize)]
struct RestoreOutput {
    path: String,
    backup: String,
    pre_restore_snapshot: Option<String>,
    backup_messages: usize,
    current_messages: Option<usize>,
    backup_size: u64,
    backup_modified: String,
    listed_only: bool,
    restored: bool,
}

pub fn restore(
    projects_dir: &Path,
    target: &str,
    list_only: bool,
    force: bool,
    json: bool,
) -> Result<()> {
    let entry = resolve_target(projects_dir, target)?;
    let bak_path = bak_path_for(&entry.path);
    if !bak_path.exists() {
        bail!(
            "no backup found at {} — cc-session writes <file>.bak on every save",
            bak_path.display()
        );
    }

    // Sanity-check the backup parses; we don't want to restore a corrupt file.
    let backup_session = Session::load(&bak_path)?;
    let backup_messages = backup_session.messages.len();

    let bak_meta = std::fs::metadata(&bak_path)?;
    let bak_size = bak_meta.len();
    let bak_mtime: DateTime<Local> = bak_meta
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        .into();
    let bak_modified = bak_mtime.format("%Y-%m-%d %H:%M:%S").to_string();

    let current_messages = if entry.path.exists() {
        Session::load(&entry.path).ok().map(|s| s.messages.len())
    } else {
        None
    };

    if list_only {
        let out = RestoreOutput {
            path: entry.path.display().to_string(),
            backup: bak_path.display().to_string(),
            pre_restore_snapshot: None,
            backup_messages,
            current_messages,
            backup_size: bak_size,
            backup_modified: bak_modified,
            listed_only: true,
            restored: false,
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!("path:           {}", out.path);
            println!("backup:         {}", out.backup);
            println!("backup msgs:    {}", out.backup_messages);
            if let Some(c) = out.current_messages {
                println!("current msgs:   {c}");
            } else {
                println!("current msgs:   (file missing)");
            }
            println!("backup size:    {}", human_size(out.backup_size));
            println!("backup mtime:   {}", out.backup_modified);
        }
        return Ok(());
    }

    if !force && entry.path.exists() && super::io::lsof::is_open(&entry.path)? {
        bail!("file is open by another process; close Claude Code or pass --force");
    }

    // If a current file exists, snapshot it aside before overwriting so the
    // restore itself is reversible. Use a sibling path that does NOT match
    // *.bak (which we'd clobber on next save).
    let snapshot = if entry.path.exists() {
        let snap = pre_restore_snapshot_path(&entry.path);
        std::fs::copy(&entry.path, &snap)?;
        Some(snap)
    } else {
        None
    };

    // Atomic restore: copy bak -> <path>.tmp, fsync, rename.
    let tmp = with_extension_appended(&entry.path, "tmp");
    std::fs::copy(&bak_path, &tmp)?;
    {
        let f = std::fs::OpenOptions::new().write(true).open(&tmp)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &entry.path)?;

    let out = RestoreOutput {
        path: entry.path.display().to_string(),
        backup: bak_path.display().to_string(),
        pre_restore_snapshot: snapshot.map(|p| p.display().to_string()),
        backup_messages,
        current_messages,
        backup_size: bak_size,
        backup_modified: bak_modified,
        listed_only: false,
        restored: true,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("restored: {}", out.path);
        println!("from:     {}", out.backup);
        if let Some(s) = &out.pre_restore_snapshot {
            println!("prev:     {s}  (snapshot of state before restore)");
        }
        println!(
            "messages: {} (was {})",
            out.backup_messages,
            out.current_messages
                .map(|n| n.to_string())
                .unwrap_or_else(|| "missing".into())
        );
    }
    Ok(())
}

fn bak_path_for(path: &Path) -> PathBuf {
    with_extension_appended(path, "bak")
}

fn pre_restore_snapshot_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    with_extension_appended(path, &format!("pre-restore.{stamp}"))
}

fn with_extension_appended(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

// ---------- agent guide ----------

pub const AGENT_GUIDE: &str = r#"# cc-session agent guide

You are an LLM driving cc-session non-interactively. This guide is the
single source of truth for how to use it. Read it once, then operate.

## What this CLI does

Edits Claude Code session JSONL files at ~/.claude/projects/<slug>/<uuid>.jsonl.
It can browse, search, inspect, and surgically delete messages from any session
while keeping tool_use/tool_result pairs and conversational turns intact.

## Standard workflow

1. Discover sessions:
     cc-session list --json --limit 20
     cc-session search "<query>" --json --limit 10
2. Inspect one session:
     cc-session info <id-or-path> --json
     cc-session show <id-or-path> --json
3. Plan an edit (always dry-run first):
     cc-session delete <id> --indices 4,6 --dry-run --json
4. Apply:
     cc-session delete <id> --indices 4,6 --json
   Pass --force only if the session is currently open in Claude Code; this
   bypasses the lsof safety check.
5. (Optional) self-update:
     cc-session update [--version v0.2.0]
6. If a delete breaks resume in Claude Code, restore from backup:
     cc-session restore <id> --list           # inspect first
     cc-session restore <id>                  # apply (snapshots current
                                              # to <path>.pre-restore.<ts>)

## Target argument (<id-or-path>)

For show / info / delete the first positional arg accepts:
  - a full filesystem path to a .jsonl file
  - a full session UUID (preferred — unambiguous)
  - any unique substring of a session UUID (8+ chars usually fine)
If a substring matches multiple sessions, the command errors and lists the
candidates. Pass a longer prefix to disambiguate.

## Index semantics

Indices are 0-based positions in the raw JSONL (one per line). Use
`cc-session show --json` to map message text -> index. Note:
  - "Visible" messages (user / assistant text) are a subset; system messages,
    tool_use blocks, tool_result blocks, attachments, and harness wrappers
    (<bash-input>, <system-reminder>, etc.) are hidden by default. Pass
    --include-hidden to see them in `show`.
  - Indices DO shift after a successful delete. Always re-run `show` between
    deletes if you are picking by index.

## Auto-pair (always on)

Two safety extensions run on every delete request:
  1. tool_use <-> tool_result blocks always travel together. Marking either
     side pulls the other.
  2. Turn-level pairing: a "turn" = visible user msg + every message that
     follows it until the next visible user msg. Marking ANY message in a
     turn marks the whole turn (user prompt + assistant reply + intermediate
     tool calls).

The delete output reports `requested` (what you asked) and `paired_added`
(what auto-pair added). Always inspect both before applying.

## delete output JSON

  {
    "path":                  "<absolute path>",
    "parent_uuid_relinked":  int,                 // survivors whose parentUuid
                                                  // was rewritten to skip
                                                  // deleted ancestors
    "backup":                "<path>.bak | null when --dry-run",
    "requested":             [int, ...],          // sorted, what you asked
    "after_auto_pair":       [int, ...],          // sorted, final delete set
    "paired_added":          [int, ...],          // sorted, set diff
    "total_messages_before": int,
    "total_messages_after":  int,
    "dry_run":               bool,
    "saved":                 bool,
    "warnings":              [str, ...]           // e.g. orphan tool_results
  }

## show output JSON (per message)

  {
    "index":            int,
    "role":             "user" | "assistant" | "system" | ...,
    "type":             "<jsonl type field>",
    "timestamp":        ISO8601 | null,
    "tokens":           int,                    // tiktoken cl100k_base
    "visible":          bool,
    "has_tool_use":     bool,
    "has_tool_result":  bool,
    "tool_use_ids":     [str, ...],
    "tool_result_ids":  [str, ...],
    "text":             str,                    // 400-char preview by default
    "truncated":        bool                    // true when text was clipped
  }

## info output JSON

  {
    "path", "project", "session_id", "title", "modified", "size",
    "total_messages", "visible_messages", "user_messages", "assistant_messages",
    "tool_use_count", "tool_result_count",
    "orphan_result_indices": [int, ...],
    "estimated_tokens": int
  }

## list / search output JSON (per entry)

  { "project", "session_id", "title", "modified", "size", "path" }

## Selection flags for delete

You may combine any/all; the union is taken before auto-pair runs.
  --indices 3,5,7        // exact indices (comma-separated)
  --range lo..hi         // inclusive range, both ints
  --from-top N           // first N messages
  --from-bottom N        // last N messages

At least one selection flag is required.

## Safety guarantees

  - Atomic save: writes <file>.tmp, fsync, rename to <file>.
  - Backup: every save first writes <file>.bak (overwriting any prior bak).
  - Concurrent-open: if `lsof` reports the file is open by another process,
    save returns SaveError::Conflict ("file is open by another process; close
    Claude Code or pass --force"). On non-unix or when lsof is missing, this
    check is skipped with a stderr warning.
  - Round-trip: untouched messages save byte-equal — unknown JSONL fields
    are preserved verbatim via `serde(flatten)`.

## Exit codes

  0  success
  1  generic error (parse failure, conflict, IO error, ambiguous target, ...)
  2+ reserved for future structured errors
Always inspect stderr on non-zero exit for the human-readable cause.

## Environment overrides

  CC_SESSION_VERSION         pin a specific release (used by `update`).
  CC_SESSION_INSTALL_DIR     where install.sh drops the binary.
  CC_SESSION_INSTALLER_URL   override installer URL for `update` (testing).

## Useful examples (one-liners an agent can paste)

  # delete top 50 messages of a long session, dry run first
  cc-session delete <id> --from-top 50 --dry-run --json
  cc-session delete <id> --from-top 50 --json

  # purge messages 200..280 inclusive
  cc-session delete <id> --range 200..280 --dry-run --json

  # remove a single off-topic exchange (turn-pair pulls the assistant reply)
  cc-session delete <id> --indices 14 --dry-run --json

  # find a session about "auth middleware" and inspect
  cc-session search "auth middleware" --json --limit 1
  cc-session show <id-from-above> --json

## Resume safety: parentUuid auto-relink

Every save scans surviving messages and rewrites any `parentUuid` that
points to a now-deleted ancestor, walking up the chain to the nearest
surviving ancestor (or null if the chain reaches the root). The count is
reported in `parent_uuid_relinked`. This keeps Claude Code's resume
renderer happy after scattered deletes; if it ever fails anyway,
`cc-session restore <id>` rolls back to the .bak snapshot.

## Things this CLI will NOT do

  - Edit message contents in place.
  - Reorder messages.
  - Merge or split sessions.
  - Apply changes while Claude Code is actively writing to the file
    (refuses unless --force).
"#;

// ---------- update ----------

const INSTALLER_URL: &str = "https://get-claude-code-session-editor.harshiitkgp.in/install.sh";

pub fn update(version: Option<&str>) -> Result<()> {
    use std::process::{Command, Stdio};

    let installer_url =
        std::env::var("CC_SESSION_INSTALLER_URL").unwrap_or_else(|_| INSTALLER_URL.to_string());

    println!("fetching installer: {installer_url}");

    let mut curl = Command::new("curl")
        .args(["-fsSL", &installer_url])
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to spawn curl (is it installed?)")?;

    let curl_stdout = curl.stdout.take().expect("curl stdout");

    let mut sh = Command::new("sh");
    sh.stdin(curl_stdout);
    if let Some(v) = version {
        sh.env("CC_SESSION_VERSION", v);
    }
    let status = sh.status().context("failed to spawn sh")?;

    let curl_status = curl.wait().context("curl wait failed")?;
    if !curl_status.success() {
        bail!("curl exited with status {curl_status}");
    }
    if !status.success() {
        bail!("installer exited with status {status}");
    }
    println!("update complete.");
    Ok(())
}

// ---------- helpers ----------

fn resolve_target(projects_dir: &Path, target: &str) -> Result<SessionEntry> {
    // Direct path?
    let p = PathBuf::from(target);
    if p.is_file() {
        // Build a minimal SessionEntry from the path itself.
        let meta = std::fs::metadata(&p)?;
        let session_id = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let project_slug = p
            .parent()
            .and_then(|d| d.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        return Ok(SessionEntry {
            project_slug,
            session_id,
            title: String::new(),
            mtime: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            size: meta.len(),
            path: p,
        });
    }

    let entries = scan::scan(projects_dir)?;
    let needle = target.to_lowercase();
    let mut matches: Vec<&SessionEntry> = entries
        .iter()
        .filter(|e| {
            e.session_id.to_lowercase() == needle
                || e.session_id.to_lowercase().starts_with(&needle)
                || e.session_id.to_lowercase().contains(&needle)
        })
        .collect();
    if matches.is_empty() {
        return Err(anyhow!("no session matched '{target}'"));
    }
    if matches.len() > 1 {
        // Prefer exact id match.
        let exact: Vec<&&SessionEntry> = matches
            .iter()
            .filter(|e| e.session_id.to_lowercase() == needle)
            .collect();
        if exact.len() == 1 {
            return Ok((*exact[0]).clone());
        }
        let listing: Vec<String> = matches
            .iter()
            .take(5)
            .map(|e| e.session_id.clone())
            .collect();
        return Err(anyhow!(
            "'{target}' matched {} sessions (showing first 5: {:?}); pass a longer prefix or full id",
            matches.len(),
            listing
        ));
    }
    Ok(matches.remove(0).clone())
}

fn parse_range(s: &str) -> Result<(usize, usize)> {
    let (lo, hi) = s
        .split_once("..")
        .ok_or_else(|| anyhow!("range must be 'lo..hi', got '{s}'"))?;
    let lo: usize = lo.parse().context("range lo")?;
    let hi: usize = hi.parse().context("range hi")?;
    if lo > hi {
        bail!("range lo ({lo}) > hi ({hi})");
    }
    Ok((lo, hi))
}

fn render_text(msg: &Message, full: bool) -> (String, bool) {
    let raw = extract_plain_text(msg).unwrap_or_default();
    if full {
        return (raw, false);
    }
    let flat: String = raw
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= 400 {
        (trimmed.to_string(), false)
    } else {
        let mut s: String = trimmed.chars().take(400).collect();
        s.push('…');
        (s, true)
    }
}

fn extract_plain_text(msg: &Message) -> Option<String> {
    let body = msg.message.as_ref()?;
    match &body.content {
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(Value::Array(blocks)) => {
            let parts: Vec<String> = blocks
                .iter()
                .filter_map(|b| {
                    let obj = b.as_object()?;
                    if obj.get("type").and_then(Value::as_str) == Some("text") {
                        obj.get("text").and_then(Value::as_str).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn collect_tool_ids(msg: &Message) -> (Vec<String>, Vec<String>) {
    let mut uses = Vec::new();
    let mut results = Vec::new();
    let blocks = msg
        .message
        .as_ref()
        .and_then(|b| b.content.as_ref())
        .and_then(|c| c.as_array());
    if let Some(arr) = blocks {
        for b in arr {
            let Some(o) = b.as_object() else { continue };
            match o.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    if let Some(id) = o.get("id").and_then(Value::as_str) {
                        uses.push(id.to_string());
                    }
                }
                Some("tool_result") => {
                    if let Some(id) = o.get("tool_use_id").and_then(Value::as_str) {
                        results.push(id.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    (uses, results)
}

fn is_visible(msg: &Message) -> bool {
    let role = msg
        .message
        .as_ref()
        .and_then(|b| b.role.as_deref())
        .or(msg.r#type.as_deref())
        .unwrap_or("");
    if role != "user" && role != "assistant" {
        return false;
    }
    let Some(t) = extract_plain_text(msg) else {
        return false;
    };
    let trimmed = t.trim();
    if trimmed.is_empty() {
        return false;
    }
    !is_harness_wrapper(trimmed)
}

fn is_harness_wrapper(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("<local-command-caveat>")
        || t.starts_with("<command-message>")
        || t.starts_with("<command-name>")
        || t.starts_with("<command-args>")
        || t.starts_with("<bash-input>")
        || t.starts_with("<bash-stdout>")
        || t.starts_with("<bash-stderr>")
        || t.starts_with("<system-reminder>")
}

fn format_mtime(e: &SessionEntry) -> String {
    let dt: DateTime<Local> = e.mtime.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn human_size(b: u64) -> String {
    const K: u64 = 1024;
    if b < K {
        format!("{b}B")
    } else if b < K * K {
        format!("{:.1}K", b as f64 / K as f64)
    } else {
        format!("{:.1}M", b as f64 / (K * K) as f64)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_ok() {
        assert_eq!(parse_range("3..7").unwrap(), (3, 7));
    }

    #[test]
    fn parse_range_inverted_errors() {
        assert!(parse_range("9..2").is_err());
    }

    #[test]
    fn parse_range_bad_format() {
        assert!(parse_range("3-7").is_err());
        assert!(parse_range("abc..7").is_err());
    }

    #[test]
    fn truncate_short() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_long() {
        let r = truncate("abcdefghij", 5);
        assert!(r.ends_with('…'));
        assert_eq!(r.chars().count(), 5);
    }
}
