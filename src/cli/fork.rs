//! Fork-on-delete: produces a new session JSONL file with a fresh UUID,
//! leaving the source untouched. Used by both the CLI `delete` command and
//! the TUI edit screen's save flow.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::io::atomic;
use crate::scan::FORK_SENTINEL_TYPE;
use crate::session::Session;

#[derive(Debug)]
pub struct ForkOutcome {
    pub new_session_id: String,
    pub new_path: PathBuf,
    #[allow(dead_code)]
    pub origin_session_id: String,
    #[allow(dead_code)]
    pub relinked: usize,
}

/// Write a new session JSONL file under the same project directory,
/// prefixed with a `cc-session-fork` sentinel line and with all top-level
/// `sessionId` fields rewritten to the new UUID.
pub fn fork_session(session: &Session, omit: &HashSet<usize>) -> Result<ForkOutcome> {
    let new_id = Uuid::new_v4().to_string();
    let parent = session
        .path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("source session has no parent directory"))?;
    let new_path = parent.join(format!("{new_id}.jsonl"));

    let origin_session_id = session
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let (rendered, relinked) = session.render_with_relink(omit)?;

    // Rewrite top-level `sessionId` per line. Any line that is not parseable
    // JSON or has no sessionId field passes through unchanged. Prepend a
    // sentinel line so `scan` can mark this file as a fork.
    let sentinel = serde_json::json!({
        "type": FORK_SENTINEL_TYPE,
        "origin": origin_session_id,
        "forked_at": Utc::now().to_rfc3339(),
        "cc_session_version": env!("CARGO_PKG_VERSION"),
    });
    let mut out = String::new();
    out.push_str(&serde_json::to_string(&sentinel)?);
    out.push('\n');

    for line in rendered.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed.is_empty() {
            out.push_str(line);
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(mut v) => {
                if let Value::Object(map) = &mut v {
                    if let Some(sid) = map.get_mut("sessionId") {
                        if sid.is_string() {
                            *sid = Value::String(new_id.clone());
                        }
                    }
                }
                out.push_str(&serde_json::to_string(&v)?);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            Err(_) => out.push_str(line),
        }
    }

    atomic::write_atomic(&new_path, &out)?;

    Ok(ForkOutcome {
        new_session_id: new_id,
        new_path,
        origin_session_id,
        relinked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use std::fs;
    use std::io::Write;

    fn write_tmp_session(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("11111111-1111-1111-1111-111111111111.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        (dir, path)
    }

    #[test]
    fn fork_creates_new_file_with_sentinel() {
        let content = "{\"type\":\"user\",\"uuid\":\"a\",\"sessionId\":\"11111111-1111-1111-1111-111111111111\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n";
        let (_dir, path) = write_tmp_session(content);
        let session = Session::load(&path).unwrap();

        let omit = HashSet::new();
        let out = fork_session(&session, &omit).unwrap();

        assert!(out.new_path.exists());
        assert_ne!(out.new_session_id, "11111111-1111-1111-1111-111111111111");

        // Original untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), content);

        // New file: first line is sentinel; second line has rewritten sessionId.
        let new_content = fs::read_to_string(&out.new_path).unwrap();
        let mut lines = new_content.lines();
        let sentinel: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(sentinel["type"], "cc-session-fork");
        assert_eq!(sentinel["origin"], "11111111-1111-1111-1111-111111111111");

        let user_line: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(user_line["sessionId"], out.new_session_id);
        assert_eq!(user_line["uuid"], "a"); // intra-session uuids untouched
    }

    #[test]
    fn fork_with_delete_drops_marked_messages() {
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"a\",\"sessionId\":\"old\"}\n",
            "{\"type\":\"assistant\",\"uuid\":\"b\",\"parentUuid\":\"a\",\"sessionId\":\"old\"}\n",
            "{\"type\":\"user\",\"uuid\":\"c\",\"parentUuid\":\"b\",\"sessionId\":\"old\"}\n",
        );
        let (_dir, path) = write_tmp_session(content);
        let session = Session::load(&path).unwrap();

        let mut omit = HashSet::new();
        omit.insert(1); // drop b
        let out = fork_session(&session, &omit).unwrap();

        let new_content = fs::read_to_string(&out.new_path).unwrap();
        // sentinel + 2 surviving lines
        assert_eq!(new_content.lines().count(), 3);
        // c.parentUuid relinked from b -> a
        assert!(new_content.contains("\"parentUuid\":\"a\""));
        assert_eq!(out.relinked, 1);
    }

    #[test]
    fn fork_passes_through_lines_without_sessionid() {
        // ai-title and similar lines have no top-level sessionId; they must
        // pass through unmodified.
        let content = concat!(
            "{\"type\":\"ai-title\",\"aiTitle\":\"Some Title\",\"sessionId\":\"old\"}\n",
            "{\"type\":\"user\",\"uuid\":\"a\"}\n",
        );
        let (_dir, path) = write_tmp_session(content);
        let session = Session::load(&path).unwrap();

        let omit = HashSet::new();
        let out = fork_session(&session, &omit).unwrap();
        let new_content = fs::read_to_string(&out.new_path).unwrap();

        // ai-title's sessionId rewritten too (still a top-level sessionId).
        assert!(new_content.contains(&out.new_session_id));
        // aiTitle preserved.
        assert!(new_content.contains("Some Title"));
    }
}
