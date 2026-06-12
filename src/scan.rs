use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub project_slug: String,
    pub session_id: String,
    pub title: String,
    pub mtime: SystemTime,
    pub size: u64,
    pub path: PathBuf,
    /// True when the session file carries the cc-session-fork sentinel,
    /// meaning this file was produced by `cc-session delete` and the title
    /// should be displayed with an `[edited]` prefix.
    pub is_fork: bool,
    /// When `is_fork`, the original session id this file was forked from
    /// (best-effort, may be empty).
    pub fork_origin: Option<String>,
}

/// Public marker line type used to tag forked session files. Kept here as a
/// constant so scan / fork agree.
pub const FORK_SENTINEL_TYPE: &str = "cc-session-fork";

const TITLE_LIMIT: usize = 60;

pub fn scan(projects_dir: &Path) -> anyhow::Result<Vec<SessionEntry>> {
    let mut entries = Vec::new();
    if !projects_dir.exists() {
        return Ok(entries);
    }

    let project_dirs = match fs::read_dir(projects_dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("warning: cannot read {}: {}", projects_dir.display(), e);
            return Ok(entries);
        }
    };

    for project_entry in project_dirs.flatten() {
        let ptype = match project_entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !ptype.is_dir() {
            continue;
        }
        let project_slug = project_entry.file_name().to_string_lossy().to_string();
        let project_path = project_entry.path();

        let session_files = match fs::read_dir(&project_path) {
            Ok(rd) => rd,
            Err(e) => {
                eprintln!("warning: cannot read {}: {}", project_path.display(), e);
                continue;
            }
        };

        for session_entry in session_files.flatten() {
            let path = session_entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let meta = match session_entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }

            let session_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let scanned = scan_one(&path);
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

            let title = if scanned.is_fork {
                format!("[edited] {}", scanned.title)
            } else {
                scanned.title
            };

            entries.push(SessionEntry {
                project_slug: project_slug.clone(),
                session_id,
                title,
                mtime,
                size: meta.len(),
                path,
                is_fork: scanned.is_fork,
                fork_origin: scanned.fork_origin,
            });
        }
    }

    entries.sort_by_key(|e| std::cmp::Reverse(e.mtime));
    Ok(entries)
}

struct ScannedMeta {
    title: String,
    is_fork: bool,
    fork_origin: Option<String>,
}

fn scan_one(path: &Path) -> ScannedMeta {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => {
            return ScannedMeta {
                title: "<unreadable>".into(),
                is_fork: false,
                fork_origin: None,
            };
        }
    };
    let reader = BufReader::new(file);
    let mut first_user_text: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut is_fork = false;
    let mut fork_origin: Option<String> = None;

    for (peeked, line) in reader.lines().enumerate() {
        if peeked >= 1000 {
            break;
        }
        let raw = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if raw.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let entry_type = v.get("type").and_then(Value::as_str);

        if entry_type == Some(FORK_SENTINEL_TYPE) {
            is_fork = true;
            if let Some(o) = v.get("origin").and_then(Value::as_str) {
                if !o.is_empty() {
                    fork_origin = Some(o.to_string());
                }
            }
            continue;
        }

        if ai_title.is_none() && entry_type == Some("ai-title") {
            if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                let t = t.trim();
                if !t.is_empty() {
                    ai_title = Some(clamp_title(t));
                }
            }
            continue;
        }

        if first_user_text.is_none() && entry_type == Some("user") {
            let content = v.get("message").and_then(|m| m.get("content"));
            let text = match content {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(Value::as_str) == Some("text") {
                            b.get("text").and_then(Value::as_str).map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => continue,
            };
            first_user_text = Some(text);
        }
    }

    let title = ai_title
        .or_else(|| first_user_text.as_deref().map(clamp_title))
        .unwrap_or_else(|| "<no user message>".into());

    ScannedMeta {
        title,
        is_fork,
        fork_origin,
    }
}

fn clamp_title(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= TITLE_LIMIT {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(TITLE_LIMIT).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_session(dir: &Path, project: &str, name: &str, content: &str) -> PathBuf {
        let pdir = dir.join(project);
        fs::create_dir_all(&pdir).unwrap();
        let p = pdir.join(format!("{name}.jsonl"));
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    #[test]
    fn empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let entries = scan(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn missing_dir_returns_empty() {
        let entries = scan(Path::new("/nonexistent/ccsession-test")).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn collects_sessions_across_projects() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "proj-a",
            "sess1",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello world\"}}\n",
        );
        make_session(
            dir.path(),
            "proj-b",
            "sess2",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"another\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn title_falls_back_when_no_user_msg() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"system\",\"message\":{\"role\":\"system\",\"content\":\"x\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert_eq!(entries[0].title, "<no user message>");
    }

    #[test]
    fn title_truncates_long_text() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(200);
        make_session(
            dir.path(),
            "p",
            "s",
            &format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{long}\"}}}}\n"
            ),
        );
        let entries = scan(dir.path()).unwrap();
        assert!(entries[0].title.ends_with('…'));
        assert!(entries[0].title.chars().count() <= TITLE_LIMIT + 1);
    }

    #[test]
    fn title_flattens_newlines() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"line1\\nline2\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert!(!entries[0].title.contains('\n'));
        assert!(entries[0].title.contains("line1 line2"));
    }

    #[test]
    fn ai_title_overrides_first_user_message() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"raw question\"}}\n{\"type\":\"ai-title\",\"aiTitle\":\"Pretty Generated Title\"}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert_eq!(entries[0].title, "Pretty Generated Title");
    }

    #[test]
    fn empty_ai_title_falls_back_to_user_msg() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"ai-title\",\"aiTitle\":\"\"}\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"fallback text\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert_eq!(entries[0].title, "fallback text");
    }

    #[test]
    fn fork_sentinel_marks_entry() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"cc-session-fork\",\"origin\":\"old-id\",\"forked_at\":\"2026-06-11T00:00:00Z\"}\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert!(entries[0].is_fork);
        assert_eq!(entries[0].fork_origin.as_deref(), Some("old-id"));
        assert!(entries[0].title.starts_with("[edited] "));
    }

    #[test]
    fn no_sentinel_no_badge() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert!(!entries[0].is_fork);
        assert!(!entries[0].title.starts_with("[edited]"));
    }

    #[test]
    fn malformed_first_line_recovers() {
        let dir = tempfile::tempdir().unwrap();
        make_session(
            dir.path(),
            "p",
            "s",
            "not json\n{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"ok\"}}\n",
        );
        let entries = scan(dir.path()).unwrap();
        assert_eq!(entries[0].title, "ok");
    }
}
