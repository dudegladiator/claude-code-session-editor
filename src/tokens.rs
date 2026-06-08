use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::Value;
use tiktoken_rs::CoreBPE;

use crate::session::Message;

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

    /// Returns approximate token count for a message.
    /// Uses pre-computed `usage.input_tokens` / `usage.output_tokens` when present.
    pub fn count(&self, msg_idx: usize, msg: &Message) -> usize {
        if let Some(cached) = self.cache.borrow().get(&msg_idx) {
            return *cached;
        }
        let count = self.compute(msg);
        self.cache.borrow_mut().insert(msg_idx, count);
        count
    }

    fn compute(&self, msg: &Message) -> usize {
        if let Some(body) = &msg.message {
            if let Some(usage) = body.extra.get("usage").and_then(Value::as_object) {
                let input = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let output = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let total = (input + output) as usize;
                if total > 0 {
                    return total;
                }
            }
        }
        let text = collect_text(msg);
        if text.is_empty() {
            return 0;
        }
        self.with_encoder(|enc| enc.encode_with_special_tokens(&text).len())
    }

    fn with_encoder<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&CoreBPE) -> R,
    {
        let mut slot = self.encoder.borrow_mut();
        if slot.is_none() {
            // cl100k_base covers GPT-4 / Claude approximation. tiktoken-rs provides
            // a constructor that returns CoreBPE directly.
            *slot = Some(tiktoken_rs::cl100k_base().expect("cl100k_base init"));
        }
        f(slot.as_ref().unwrap())
    }
}

fn collect_text(msg: &Message) -> String {
    let body = match &msg.message {
        Some(b) => b,
        None => return String::new(),
    };
    match &body.content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                let obj = b.as_object()?;
                if obj.get("type").and_then(Value::as_str) == Some("text") {
                    obj.get("text").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::MessageBody;
    use serde_json::json;

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
        assert_eq!(c.count(0, &m), 0);
    }

    #[test]
    fn plain_text_counts_positive() {
        let m = msg_with_text("hello world this is a test");
        let c = TokenCounter::new();
        let n = c.count(0, &m);
        assert!(n > 0 && n < 20);
    }

    #[test]
    fn usage_metadata_takes_precedence() {
        let mut body = MessageBody {
            role: Some("assistant".into()),
            content: Some(Value::String("a".repeat(10_000))),
            extra: Default::default(),
        };
        body.extra.insert(
            "usage".into(),
            json!({"input_tokens": 5, "output_tokens": 7}),
        );
        let m = Message {
            r#type: Some("assistant".into()),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: Some(body),
            extra: Default::default(),
            original_line: None,
        };
        let c = TokenCounter::new();
        assert_eq!(c.count(0, &m), 12);
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
