use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at line {line}: {source}")]
    Parse {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

/// Top-level entry in a Claude Code session JSONL.
///
/// Known fields are typed; any unknown fields are captured in `extra` so they
/// round-trip verbatim across save. `original_line` holds the exact bytes the
/// message was loaded from, so untouched messages save byte-equal regardless of
/// serde key ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,

    #[serde(
        rename = "parentUuid",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_uuid: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MessageBody>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,

    /// Raw line as read from disk. `None` for messages constructed in memory.
    #[serde(skip)]
    pub original_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    /// Either a plain string or an array of content blocks (text, tool_use, tool_result, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub path: PathBuf,
    pub messages: Vec<Message>,
    /// True if the source file ended with a trailing newline.
    pub trailing_newline: bool,
}

impl Session {
    pub fn load(path: &Path) -> Result<Self, SessionError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();
        let mut last_was_newline = false;

        for (idx, line) in reader.lines().enumerate() {
            let raw = line?;
            last_was_newline = false;
            if raw.is_empty() {
                last_was_newline = true;
                continue;
            }
            let mut msg: Message =
                serde_json::from_str(&raw).map_err(|source| SessionError::Parse {
                    line: idx + 1,
                    source,
                })?;
            msg.original_line = Some(raw);
            messages.push(msg);
        }

        // BufRead::lines drops trailing newline info; re-stat to detect it.
        let trailing_newline = file_ends_with_newline(path).unwrap_or(last_was_newline);

        Ok(Session {
            path: path.to_path_buf(),
            messages,
            trailing_newline,
        })
    }

    /// Render a subset of messages (those whose indices are not in `omit`)
    /// as JSONL bytes. Survivors whose `parentUuid` points into the deleted
    /// set are re-linked to the nearest surviving ancestor (or `None` if the
    /// chain reaches the root); their line is re-serialized so the file
    /// stays internally consistent. Other untouched messages reuse their
    /// `original_line` for byte equivalence.
    ///
    /// Returns the rendered bytes and the number of survivors whose
    /// `parentUuid` was rewritten.
    pub fn render_with_relink(
        &self,
        omit: &std::collections::HashSet<usize>,
    ) -> Result<(String, usize), SessionError> {
        // Build uuid -> idx map for the full session, plus parent-uuid lookup
        // by idx so we can climb chains in O(1) per step.
        let mut uuid_to_idx: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::with_capacity(self.messages.len());
        for (idx, msg) in self.messages.iter().enumerate() {
            if let Some(u) = &msg.uuid {
                uuid_to_idx.insert(u.as_str(), idx);
            }
        }

        // For each msg, compute the parentUuid that should appear in the
        // rewritten file: walk up via parent_uuid until you find a surviving
        // ancestor, or None. Cache results.
        let mut resolved_parent: Vec<Option<String>> = vec![None; self.messages.len()];
        let mut needs_rewrite: Vec<bool> = vec![false; self.messages.len()];
        for (idx, msg) in self.messages.iter().enumerate() {
            if omit.contains(&idx) {
                continue;
            }
            let original = msg.parent_uuid.clone();
            let mut cursor = original.as_deref();
            loop {
                let Some(p_uuid) = cursor else {
                    // Chain reached root.
                    resolved_parent[idx] = None;
                    break;
                };
                let Some(&p_idx) = uuid_to_idx.get(p_uuid) else {
                    // Parent uuid does not refer to any known message in this
                    // file. Preserve verbatim — this is foreign data we can't
                    // reason about.
                    resolved_parent[idx] = Some(p_uuid.to_string());
                    break;
                };
                if !omit.contains(&p_idx) {
                    resolved_parent[idx] = Some(p_uuid.to_string());
                    break;
                }
                // Parent is deleted; climb to its parent.
                cursor = self.messages[p_idx].parent_uuid.as_deref();
            }
            if resolved_parent[idx] != original {
                needs_rewrite[idx] = true;
            }
        }

        let mut out = String::new();
        let mut relinked = 0usize;
        for (idx, msg) in self.messages.iter().enumerate() {
            if omit.contains(&idx) {
                continue;
            }
            if needs_rewrite[idx] {
                relinked += 1;
                let mut clone = msg.clone();
                clone.parent_uuid = resolved_parent[idx].clone();
                clone.original_line = None;
                let line = serde_json::to_string(&clone).map_err(|source| SessionError::Parse {
                    line: idx + 1,
                    source,
                })?;
                out.push_str(&line);
                out.push('\n');
            } else {
                match &msg.original_line {
                    Some(line) => {
                        out.push_str(line);
                        out.push('\n');
                    }
                    None => {
                        let line =
                            serde_json::to_string(msg).map_err(|source| SessionError::Parse {
                                line: idx + 1,
                                source,
                            })?;
                        out.push_str(&line);
                        out.push('\n');
                    }
                }
            }
        }
        if !self.trailing_newline && out.ends_with('\n') {
            out.pop();
        }
        Ok((out, relinked))
    }

    /// Backwards-compatible render that discards the relink count.
    pub fn render(&self, omit: &std::collections::HashSet<usize>) -> Result<String, SessionError> {
        Ok(self.render_with_relink(omit)?.0)
    }
}

fn file_ends_with_newline(path: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len == 0 {
        return Ok(false);
    }
    f.seek(SeekFrom::End(-1))?;
    let mut buf = [0u8; 1];
    f.read_exact(&mut buf)?;
    Ok(buf[0] == b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_round_trip_simple() {
        let content = "{\"type\":\"user\",\"uuid\":\"a\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n{\"type\":\"assistant\",\"uuid\":\"b\",\"message\":{\"role\":\"assistant\",\"content\":\"hello\"}}\n";
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        assert_eq!(s.messages.len(), 2);
        let omit = std::collections::HashSet::new();
        assert_eq!(s.render(&omit).unwrap(), content);
    }

    #[test]
    fn unknown_fields_preserved() {
        let content = "{\"type\":\"user\",\"uuid\":\"a\",\"experimental_flag\":true,\"message\":{\"role\":\"user\",\"content\":\"hi\",\"weird\":42}}\n";
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        assert_eq!(
            s.render(&std::collections::HashSet::new()).unwrap(),
            content
        );
    }

    #[test]
    fn empty_file() {
        let f = write_tmp("");
        let s = Session::load(f.path()).unwrap();
        assert!(s.messages.is_empty());
    }

    #[test]
    fn malformed_line_returns_error() {
        let f = write_tmp("{\"type\":\"user\"}\nnot-json\n");
        let err = Session::load(f.path()).unwrap_err();
        match err {
            SessionError::Parse { line, .. } => assert_eq!(line, 2),
            _ => panic!("expected parse error"),
        }
    }

    #[test]
    fn relink_skips_deleted_ancestor() {
        // Chain: a (root) -> b -> c -> d. Delete b and c. d should now point
        // to a; a should still point to nothing.
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"a\",\"message\":{\"role\":\"user\",\"content\":\"a\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"b\",\"parentUuid\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"b\"}}\n",
            "{\"type\":\"user\",\"uuid\":\"c\",\"parentUuid\":\"b\",\"message\":{\"role\":\"user\",\"content\":\"c\"}}\n",
            "{\"type\":\"assistant\",\"uuid\":\"d\",\"parentUuid\":\"c\",\"message\":{\"role\":\"assistant\",\"content\":\"d\"}}\n",
        );
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        let mut omit = std::collections::HashSet::new();
        omit.insert(1); // b
        omit.insert(2); // c
        let (out, relinked) = s.render_with_relink(&omit).unwrap();
        assert_eq!(relinked, 1, "only d's parent should change");
        // d's line should be re-serialized with parentUuid=a.
        assert!(out.contains("\"uuid\":\"d\""));
        assert!(out.contains("\"parentUuid\":\"a\""));
        // a is untouched.
        assert!(out.contains("\"uuid\":\"a\""));
        // b and c are gone.
        assert!(!out.contains("\"uuid\":\"b\""));
        assert!(!out.contains("\"uuid\":\"c\""));
    }

    #[test]
    fn relink_to_root_when_all_ancestors_deleted() {
        // a -> b -> c. Delete a and b. c's parentUuid should become null
        // (omitted in JSON because Option::None skip_serializing).
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"a\"}\n",
            "{\"type\":\"assistant\",\"uuid\":\"b\",\"parentUuid\":\"a\"}\n",
            "{\"type\":\"user\",\"uuid\":\"c\",\"parentUuid\":\"b\"}\n",
        );
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        let mut omit = std::collections::HashSet::new();
        omit.insert(0);
        omit.insert(1);
        let (out, relinked) = s.render_with_relink(&omit).unwrap();
        assert_eq!(relinked, 1);
        assert!(out.contains("\"uuid\":\"c\""));
        // c should NOT contain a parentUuid field after relink.
        assert!(!out.contains("\"parentUuid\""));
    }

    #[test]
    fn relink_no_change_byte_equal() {
        // Nothing deleted -> output byte-equal to input, relinked=0.
        let content = concat!(
            "{\"type\":\"user\",\"uuid\":\"a\"}\n",
            "{\"type\":\"assistant\",\"uuid\":\"b\",\"parentUuid\":\"a\"}\n",
        );
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        let omit = std::collections::HashSet::new();
        let (out, relinked) = s.render_with_relink(&omit).unwrap();
        assert_eq!(relinked, 0);
        assert_eq!(out, content);
    }

    #[test]
    fn relink_preserves_unknown_parent_uuid() {
        // c.parentUuid points outside the file (e.g. came from a fork).
        // Even when ancestor uuids are not present, we should not blank it.
        let content =
            "{\"type\":\"user\",\"uuid\":\"c\",\"parentUuid\":\"foreign-id\"}\n".to_string();
        let f = write_tmp(&content);
        let s = Session::load(f.path()).unwrap();
        let omit = std::collections::HashSet::new();
        let (out, relinked) = s.render_with_relink(&omit).unwrap();
        assert_eq!(relinked, 0);
        assert!(out.contains("\"parentUuid\":\"foreign-id\""));
    }

    #[test]
    fn omit_drops_indices() {
        let content = "{\"type\":\"user\",\"uuid\":\"a\"}\n{\"type\":\"user\",\"uuid\":\"b\"}\n{\"type\":\"user\",\"uuid\":\"c\"}\n";
        let f = write_tmp(content);
        let s = Session::load(f.path()).unwrap();
        let mut omit = std::collections::HashSet::new();
        omit.insert(1);
        let out = s.render(&omit).unwrap();
        assert_eq!(
            out,
            "{\"type\":\"user\",\"uuid\":\"a\"}\n{\"type\":\"user\",\"uuid\":\"c\"}\n"
        );
    }
}
