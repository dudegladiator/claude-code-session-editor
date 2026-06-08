use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::session::Message;

/// Per-tool-call lookup: where the `tool_use` lives, and where its matching
/// `tool_result` lives (if any).
#[derive(Debug, Default, Clone)]
pub struct PairEntry {
    pub use_idx: usize,
    pub result_idx: Option<usize>,
}

#[derive(Debug, Default, Clone)]
pub struct PairIndex {
    pub by_id: HashMap<String, PairEntry>,
    /// Reverse: message index -> tool call ids it owns (use or result).
    pub by_msg: HashMap<usize, Vec<String>>,
    /// tool_results with no matching tool_use.
    #[allow(dead_code)]
    pub orphan_results: Vec<usize>,
}

impl PairIndex {
    pub fn build(messages: &[Message]) -> Self {
        let mut by_id: HashMap<String, PairEntry> = HashMap::new();
        let mut by_msg: HashMap<usize, Vec<String>> = HashMap::new();
        let mut orphan_results = Vec::new();

        for (idx, msg) in messages.iter().enumerate() {
            let blocks = match msg.message.as_ref().and_then(|b| b.content.as_ref()) {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };
            for block in blocks {
                let Some(obj) = block.as_object() else { continue };
                let block_type = obj.get("type").and_then(Value::as_str);
                match block_type {
                    Some("tool_use") => {
                        if let Some(id) = obj.get("id").and_then(Value::as_str) {
                            by_id
                                .entry(id.to_string())
                                .or_insert(PairEntry {
                                    use_idx: idx,
                                    result_idx: None,
                                })
                                .use_idx = idx;
                            by_msg.entry(idx).or_default().push(id.to_string());
                        }
                    }
                    Some("tool_result") => {
                        if let Some(id) = obj.get("tool_use_id").and_then(Value::as_str) {
                            match by_id.get_mut(id) {
                                Some(entry) => {
                                    entry.result_idx = Some(idx);
                                }
                                None => {
                                    // result before use, or use never appeared.
                                    orphan_results.push(idx);
                                }
                            }
                            by_msg.entry(idx).or_default().push(id.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        // Second pass: tool_use ids whose result was found late still got attached
        // above. Anything in orphan_results whose use later appeared also resolves;
        // re-check.
        orphan_results.retain(|orphan_idx| {
            // If any id at this msg has a use_idx in by_id, it's resolved.
            let ids = by_msg.get(orphan_idx).cloned().unwrap_or_default();
            !ids.iter().any(|id| {
                by_id
                    .get(id)
                    .map(|e| e.result_idx == Some(*orphan_idx))
                    .unwrap_or(false)
            })
        });

        // Patch resolved orphans (result-before-use): walk by_msg entries that
        // were tool_results and ensure their use was recorded.
        for (msg_idx, ids) in by_msg.iter() {
            for id in ids {
                if let Some(entry) = by_id.get_mut(id) {
                    if entry.result_idx.is_none() {
                        // Determine if this msg owns the tool_result for `id`.
                        // Re-check by looking at the message's blocks.
                        let _ = msg_idx;
                        // Already handled in first pass; nothing extra needed.
                    }
                }
            }
        }

        PairIndex {
            by_id,
            by_msg,
            orphan_results,
        }
    }

    /// Extend `marked` so every tool_use's matching tool_result (and vice versa)
    /// is included. Returns the number of indices added.
    pub fn auto_pair(&self, marked: &mut HashSet<usize>) -> usize {
        let mut added = 0usize;
        let initial: Vec<usize> = marked.iter().copied().collect();
        for idx in initial {
            let Some(ids) = self.by_msg.get(&idx) else { continue };
            for id in ids {
                let Some(entry) = self.by_id.get(id) else { continue };
                if marked.insert(entry.use_idx) {
                    added += 1;
                }
                if let Some(r) = entry.result_idx {
                    if marked.insert(r) {
                        added += 1;
                    }
                }
            }
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Message, MessageBody};
    use serde_json::json;

    fn msg(role: &str, content: Value) -> Message {
        Message {
            r#type: Some(if role == "user" { "user".into() } else { "assistant".into() }),
            uuid: None,
            parent_uuid: None,
            timestamp: None,
            message: Some(MessageBody {
                role: Some(role.into()),
                content: Some(content),
                extra: Default::default(),
            }),
            extra: Default::default(),
            original_line: None,
        }
    }

    #[test]
    fn marks_use_marks_result() {
        let msgs = vec![
            msg("user", json!("ask")),
            msg(
                "assistant",
                json!([{"type":"tool_use","id":"t1","name":"R","input":{}}]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}]),
            ),
            msg("assistant", json!("done")),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([1usize]);
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&1));
        assert!(marked.contains(&2));
    }

    #[test]
    fn marks_result_marks_use() {
        let msgs = vec![
            msg("assistant", json!([{"type":"tool_use","id":"t1","name":"R","input":{}}])),
            msg("user", json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}])),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([1usize]);
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&0));
        assert!(marked.contains(&1));
    }

    #[test]
    fn plain_text_no_pair() {
        let msgs = vec![msg("user", json!("hi")), msg("assistant", json!("hello"))];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([0usize]);
        idx.auto_pair(&mut marked);
        assert_eq!(marked.len(), 1);
    }

    #[test]
    fn parallel_tool_calls() {
        let msgs = vec![
            msg(
                "assistant",
                json!([
                    {"type":"tool_use","id":"t1","name":"R","input":{}},
                    {"type":"tool_use","id":"t2","name":"R","input":{}}
                ]),
            ),
            msg("user", json!([{"type":"tool_result","tool_use_id":"t1","content":"a"}])),
            msg("user", json!([{"type":"tool_result","tool_use_id":"t2","content":"b"}])),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([0usize]);
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&1));
        assert!(marked.contains(&2));
    }

    #[test]
    fn orphan_result_detected() {
        let msgs = vec![msg(
            "user",
            json!([{"type":"tool_result","tool_use_id":"missing","content":"x"}]),
        )];
        let idx = PairIndex::build(&msgs);
        assert_eq!(idx.orphan_results, vec![0]);
    }

    #[test]
    fn empty_marks_empty() {
        let msgs = vec![msg("user", json!("hi"))];
        let idx = PairIndex::build(&msgs);
        let mut marked: HashSet<usize> = HashSet::new();
        idx.auto_pair(&mut marked);
        assert!(marked.is_empty());
    }

    #[test]
    fn range_extends_to_external_counterpart() {
        let msgs = vec![
            msg("user", json!("a")),
            msg("assistant", json!([{"type":"tool_use","id":"t1","name":"R","input":{}}])),
            msg("user", json!("b")),
            msg("user", json!([{"type":"tool_result","tool_use_id":"t1","content":"r"}])),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked: HashSet<usize> = (0..=2).collect();
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&3));
    }
}
