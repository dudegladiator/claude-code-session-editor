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

    /// Render a subset of messages (those whose indices are not in `omit`) as
    /// JSONL bytes. Untouched messages reuse their `original_line` for byte
    /// equivalence; new or modified messages serialize via serde.
    pub fn render(&self, omit: &std::collections::HashSet<usize>) -> Result<String, SessionError> {
        let mut out = String::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            if omit.contains(&idx) {
                continue;
            }
            match &msg.original_line {
                Some(line) => {
                    out.push_str(line);
                    out.push('\n');
                }
                None => {
                    out.push_str(&serde_json::to_string(msg).map_err(|source| {
                        SessionError::Parse {
                            line: idx + 1,
                            source,
                        }
                    })?);
                    out.push('\n');
                }
            }
        }
        if !self.trailing_newline && out.ends_with('\n') {
            out.pop();
        }
        Ok(out)
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
