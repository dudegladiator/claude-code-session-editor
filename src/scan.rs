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
}

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
            let title = derive_title(&path);
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

            entries.push(SessionEntry {
                project_slug: project_slug.clone(),
                session_id,
                title,
                mtime,
                size: meta.len(),
                path,
            });
        }
    }

    entries.sort_by_key(|e| std::cmp::Reverse(e.mtime));
    Ok(entries)
}

fn derive_title(path: &Path) -> String {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return "<unreadable>".into(),
    };
    let reader = BufReader::new(file);
    for (peeked, line) in reader.lines().enumerate() {
        if peeked >= 50 {
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
        if v.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
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
        return clamp_title(&text);
    }
    "<no user message>".into()
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
