//! Non-interactive CLI subcommands. Output is designed to be parsed by other
//! agents (Claude Code, Codex, scripts) — every `--json` mode emits a
//! deterministic shape with stable keys.

pub mod fork;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::Value;

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
    is_fork: bool,
    fork_origin: Option<String>,
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
        print_table_header();
        for e in &entries {
            print_table_row(e);
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
        print_table_header();
        for e in &hits {
            print_table_row(e);
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
        is_fork: e.is_fork,
        fork_origin: e.fork_origin.clone(),
    }
}

fn print_table_header() {
    println!(
        "{:<40} {:<50} {:<17} {:<10} id",
        "project", "title", "modified", "size"
    );
}

fn print_table_row(e: &SessionEntry) {
    println!(
        "{:<40} {:<50} {:<17} {:<10} {}",
        truncate(&e.project_slug, 40),
        truncate(&e.title, 50),
        format_mtime(e),
        human_size(e.size),
        e.session_id
    );
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
    let _ = pairing;

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

// ---------- delete (always forks) ----------

#[derive(Serialize)]
struct DeleteOutput {
    source_path: String,
    new_session_id: Option<String>,
    new_path: Option<String>,
    resume_command: Option<String>,
    parent_uuid_relinked: usize,
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
    pairing.auto_pair(&mut marked);

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

    let mut warnings = Vec::new();
    if !pairing.orphan_results.is_empty() {
        warnings.push(format!(
            "session has {} orphan tool_result(s) at indices {:?}",
            pairing.orphan_results.len(),
            pairing.orphan_results
        ));
    }

    // Render the would-be content (relink runs here too) just to compute
    // relinked count and validate the plan.
    let (_content, relinked) = session.render_with_relink(&marked)?;
    let after = total - marked.len();

    let (saved, new_session_id, new_path, resume_command) = if dry_run {
        // Even in dry-run, surface the new id we would mint so the agent can
        // pre-write a resume command in its plan.
        let preview_id = uuid::Uuid::new_v4().to_string();
        let preview_path = entry
            .path
            .with_file_name(format!("{preview_id}.jsonl"))
            .display()
            .to_string();
        (
            false,
            Some(preview_id.clone()),
            Some(preview_path),
            Some(format!("claude --resume {preview_id}")),
        )
    } else {
        let outcome = fork::fork_session(&session, &marked)?;
        let resume = format!("claude --resume {}", outcome.new_session_id);
        (
            true,
            Some(outcome.new_session_id.clone()),
            Some(outcome.new_path.display().to_string()),
            Some(resume),
        )
    };

    let out = DeleteOutput {
        source_path: entry.path.display().to_string(),
        new_session_id,
        new_path,
        resume_command,
        parent_uuid_relinked: relinked,
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
        println!("source:              {}", out.source_path);
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
        if let Some(p) = &out.new_path {
            println!("forked:  {p}");
        }
        if let Some(r) = &out.resume_command {
            println!("resume:  {r}");
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
    is_fork: bool,
    fork_origin: Option<String>,
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
        is_fork: entry.is_fork,
        fork_origin: entry.fork_origin.clone(),
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
        if out.is_fork {
            println!(
                "fork:              yes (origin: {})",
                out.fork_origin.as_deref().unwrap_or("unknown")
            );
        }
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

// ---------- heatmap ----------

#[derive(Serialize)]
struct HeatmapTurn {
    anchor_idx: usize,
    start_idx: usize,
    end_idx: usize,
    msg_count: usize,
    tokens: usize,
    has_tool_use: bool,
    preview: String,
}

#[derive(Serialize)]
struct HeatmapOutput {
    path: String,
    session_id: String,
    total_messages: usize,
    total_tokens: usize,
    turns: Vec<HeatmapTurn>,
}

pub fn heatmap(projects_dir: &Path, target: &str, limit: Option<usize>, json: bool) -> Result<()> {
    let entry = resolve_target(projects_dir, target)?;
    let session = Session::load(&entry.path)?;
    let pairing = PairIndex::build(&session.messages);
    let tokens = TokenCounter::new();

    // Group message indices by their turn anchor (visible-user idx).
    let mut by_anchor: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (idx, anchor) in pairing.turn_of.iter().enumerate() {
        if *anchor == usize::MAX {
            continue;
        }
        by_anchor.entry(*anchor).or_default().push(idx);
    }

    let mut total_tokens = 0usize;
    let mut turns: Vec<HeatmapTurn> = by_anchor
        .into_iter()
        .map(|(anchor, idxs)| {
            let start = *idxs.first().unwrap_or(&anchor);
            let end = *idxs.last().unwrap_or(&anchor);
            let mut t = 0usize;
            let mut has_tool = false;
            for &i in &idxs {
                t += tokens.count(i, &session.messages[i]);
                let (u, _) = collect_tool_ids(&session.messages[i]);
                if !u.is_empty() {
                    has_tool = true;
                }
            }
            total_tokens += t;
            let preview = preview_for(&session.messages[anchor]);
            HeatmapTurn {
                anchor_idx: anchor,
                start_idx: start,
                end_idx: end,
                msg_count: idxs.len(),
                tokens: t,
                has_tool_use: has_tool,
                preview,
            }
        })
        .collect();

    turns.sort_by_key(|t| std::cmp::Reverse(t.tokens));
    if let Some(n) = limit {
        turns.truncate(n);
    } else {
        turns.truncate(20);
    }

    let out = HeatmapOutput {
        path: entry.path.display().to_string(),
        session_id: entry.session_id.clone(),
        total_messages: session.messages.len(),
        total_tokens,
        turns,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("path:           {}", out.path);
        println!(
            "{} turns shown out of session total {} tokens",
            out.turns.len(),
            out.total_tokens
        );
        println!();
        println!(
            "{:>5} {:>9} {:>5} {:>5}  preview",
            "idx", "tokens", "msgs", "tool"
        );
        println!("{}", "-".repeat(80));
        for t in &out.turns {
            println!(
                "{:>5} {:>9} {:>5} {:>5}  {}",
                t.anchor_idx,
                t.tokens,
                t.msg_count,
                if t.has_tool_use { "yes" } else { "" },
                truncate(&t.preview, 80),
            );
        }
    }
    Ok(())
}

fn preview_for(msg: &Message) -> String {
    extract_plain_text(msg)
        .unwrap_or_default()
        .replace('\n', " ")
        .trim()
        .to_string()
}

// ---------- agent guide ----------
//
// Source of truth lives at AGENTS.md in the repo root so GitHub renders it
// nicely AND `cc-session agent-guide` prints the same content. include_str!
// inlines the file at compile time, so the binary stays self-contained.

pub const AGENT_GUIDE: &str = include_str!("../AGENTS.md");

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
            is_fork: false,
            fork_origin: None,
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
