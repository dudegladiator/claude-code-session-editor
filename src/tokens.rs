use std::cell::RefCell;
use std::collections::HashMap;

use tiktoken_rs::CoreBPE;

use crate::session::Message;

/// Tokenizes whole JSONL lines so the count reflects what Claude Code actually
/// loads (text + tool_use args + tool_result content + metadata). Uses
/// `cl100k_base` as a stable, well-known approximation.
///
/// For messages that came off disk, this measures `original_line` directly —
/// no re-serialization, no shape drift. For in-memory messages (forks,
/// re-linked parents), it falls back to `serde_json::to_string`.
pub struct TokenCounter {
    encoder: RefCell<Option<CoreBPE>>,
    cache: RefCell<HashMap<usize, usize>>,
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCounter {
    pub fn new() -> Self {
        Self {
            encoder: RefCell::new(None),
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn count(&self, msg_idx: usize, msg: &Message) -> usize {
        if let Some(cached) = self.cache.borrow().get(&msg_idx) {
            return *cached;
        }
        let count = self.encode(&render_for_count(msg));
        self.cache.borrow_mut().insert(msg_idx, count);
        count
    }

    /// Count an arbitrary string with the same encoder. Useful for sentinel
    /// lines or summary text that isn't a Message.
    #[allow(dead_code)]
    pub fn count_str(&self, s: &str) -> usize {
        self.encode(s)
    }

    fn encode(&self, s: &str) -> usize {
        if s.is_empty() {
            return 0;
        }
        self.with_encoder(|enc| enc.encode_with_special_tokens(s).len())
    }

    fn with_encoder<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&CoreBPE) -> R,
    {
        let mut slot = self.encoder.borrow_mut();
        if slot.is_none() {
            *slot = Some(tiktoken_rs::cl100k_base().expect("cl100k_base init"));
        }
        f(slot.as_ref().unwrap())
    }
}

fn render_for_count(msg: &Message) -> String {
    if let Some(line) = &msg.original_line {
        return line.clone();
    }
    serde_json::to_string(msg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Message, MessageBody};
    use serde_json::{json, Value};

    fn msg_with_text(text: &str) -> Message {
        Message {
            r#type: Some("user".into()),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: Some(MessageBody {
                role: Some("user".into()),
                content: Some(Value::String(text.into())),
                extra: Default::default(),
            }),
            extra: Default::default(),
            original_line: None,
        }
    }

    #[test]
    fn empty_message_zero() {
        // Even an "empty" message has structural JSON; just verify it counts
        // the bytes of the serialized struct (more than zero).
        let m = Message {
            r#type: None,
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: None,
            extra: Default::default(),
            original_line: None,
        };
        let c = TokenCounter::new();
        // serialized as `{}` -> at least 1 token.
        assert!(c.count(0, &m) > 0);
    }

    #[test]
    fn whole_line_used_when_original_present() {
        // If original_line is set, it wins over serde shape — even when the
        // line carries fields the struct doesn't model.
        let mut m = msg_with_text("hi");
        m.original_line = Some(
            "{\"type\":\"user\",\"experimental\":\"\",\"message\":{\"role\":\"user\",\"content\":\"hi\",\"x\":\"y\"}}".into(),
        );
        let c = TokenCounter::new();
        let with_orig = c.count(0, &m);

        let mut m2 = msg_with_text("hi");
        m2.original_line = None;
        let c2 = TokenCounter::new();
        let without = c2.count(0, &m2);

        assert!(
            with_orig >= without,
            "original_line count ({with_orig}) should be >= struct-only count ({without})"
        );
    }

    #[test]
    fn tool_use_input_counted() {
        // Big tool_use input must not vanish.
        let big_arg = "x".repeat(2000);
        let line = format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t","name":"R","input":{{"data":"{big_arg}"}}}}]}}}}"#
        );
        let m = Message {
            r#type: Some("assistant".into()),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: None,
            extra: Default::default(),
            original_line: Some(line),
        };
        let c = TokenCounter::new();
        let n = c.count(0, &m);
        assert!(n > 200, "expected sizeable count for 2KB input, got {n}");
    }

    #[test]
    fn tool_result_content_counted() {
        let stdout = "log line\n".repeat(200);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t","content":{}}}]}}}}"#,
            serde_json::to_string(&stdout).unwrap()
        );
        let m = Message {
            r#type: Some("user".into()),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: None,
            extra: Default::default(),
            original_line: Some(line),
        };
        let c = TokenCounter::new();
        let n = c.count(0, &m);
        assert!(
            n > 100,
            "expected sizeable count for repeated stdout, got {n}"
        );
    }

    #[test]
    fn cache_returns_same_value() {
        let m = msg_with_text("cache me");
        let c = TokenCounter::new();
        let a = c.count(7, &m);
        let b = c.count(7, &m);
        assert_eq!(a, b);
    }

    #[test]
    fn count_str_works() {
        let c = TokenCounter::new();
        assert_eq!(c.count_str(""), 0);
        assert!(c.count_str("hello world") > 0);
    }

    #[test]
    fn block_array_counts_text_blocks() {
        let m = Message {
            r#type: Some("assistant".into()),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: Some(MessageBody {
                role: Some("assistant".into()),
                content: Some(json!([
                    {"type":"text","text":"hello"},
                    {"type":"tool_use","id":"t","name":"R","input":{}},
                    {"type":"text","text":"world"}
                ])),
                extra: Default::default(),
            }),
            extra: Default::default(),
            original_line: None,
        };
        let c = TokenCounter::new();
        assert!(c.count(0, &m) > 0);
    }
}
