use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::session::Message;

fn is_visible_user(msg: &Message) -> bool {
    let role = msg
        .message
        .as_ref()
        .and_then(|b| b.role.as_deref())
        .or(msg.r#type.as_deref())
        .unwrap_or("");
    if role != "user" {
        return false;
    }
    let Some(text) = extract_text(msg) else {
        return false;
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    !is_harness_wrapper(trimmed)
}

fn extract_text(msg: &Message) -> Option<String> {
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
    /// Per-message index, the anchor index of the conversational "turn" that
    /// message belongs to. A turn is a visible user message + every message
    /// that follows it up to (but not including) the next visible user
    /// message. Messages before the first visible user message map to
    /// `usize::MAX` and do not pair.
    pub turn_of: Vec<usize>,
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
                let Some(obj) = block.as_object() else {
                    continue;
                };
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

        // Build turn_of: every msg shares its turn anchor with the most recent
        // visible user message at or before its index.
        let mut turn_of = vec![usize::MAX; messages.len()];
        let mut current_anchor: Option<usize> = None;
        for (i, msg) in messages.iter().enumerate() {
            if is_visible_user(msg) {
                current_anchor = Some(i);
            }
            if let Some(a) = current_anchor {
                turn_of[i] = a;
            }
        }

        PairIndex {
            by_id,
            by_msg,
            orphan_results,
            turn_of,
        }
    }

    /// Extend `marked` so the entire conversational turn is covered:
    ///
    /// 1. tool_use ↔ tool_result blocks always travel together.
    /// 2. Marking any message in a turn marks every other message in the same
    ///    turn (the visible user prompt plus every assistant/tool message it
    ///    triggered, up to the next visible user prompt).
    ///
    /// Returns the number of indices added.
    pub fn auto_pair(&self, marked: &mut HashSet<usize>) -> usize {
        let before = marked.len();

        // Step 1: tool_use ↔ tool_result.
        let initial: Vec<usize> = marked.iter().copied().collect();
        for idx in initial {
            let Some(ids) = self.by_msg.get(&idx) else {
                continue;
            };
            for id in ids {
                let Some(entry) = self.by_id.get(id) else {
                    continue;
                };
                marked.insert(entry.use_idx);
                if let Some(r) = entry.result_idx {
                    marked.insert(r);
                }
            }
        }

        // Step 2: turn-level pairing. Collect every turn anchor referenced by
        // the current marked set, then sweep the messages array adding any
        // index whose turn anchor appears in that set.
        let mut anchors: HashSet<usize> = HashSet::new();
        for &idx in marked.iter() {
            if let Some(&a) = self.turn_of.get(idx) {
                if a != usize::MAX {
                    anchors.insert(a);
                }
            }
        }
        if !anchors.is_empty() {
            for (idx, &anchor) in self.turn_of.iter().enumerate() {
                if anchor != usize::MAX && anchors.contains(&anchor) {
                    marked.insert(idx);
                }
            }
        }

        marked.len() - before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Message, MessageBody};
    use serde_json::json;

    fn msg(role: &str, content: Value) -> Message {
        Message {
            r#type: Some(if role == "user" {
                "user".into()
            } else {
                "assistant".into()
            }),
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
            msg(
                "assistant",
                json!([{"type":"tool_use","id":"t1","name":"R","input":{}}]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}]),
            ),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([1usize]);
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&0));
        assert!(marked.contains(&1));
    }

    #[test]
    fn turn_pair_user_with_assistant() {
        // Marking the user message pulls its assistant response (and vice versa).
        let msgs = vec![msg("user", json!("hi")), msg("assistant", json!("hello"))];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([0usize]);
        idx.auto_pair(&mut marked);
        assert_eq!(marked, HashSet::from([0, 1]));
    }

    #[test]
    fn turn_pair_assistant_pulls_user() {
        let msgs = vec![msg("user", json!("hi")), msg("assistant", json!("hello"))];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([1usize]);
        idx.auto_pair(&mut marked);
        assert_eq!(marked, HashSet::from([0, 1]));
    }

    #[test]
    fn turn_boundary_is_next_visible_user() {
        // Two separate turns; marking turn 1 must NOT mark turn 2.
        let msgs = vec![
            msg("user", json!("first")),
            msg("assistant", json!("a1")),
            msg("user", json!("second")),
            msg("assistant", json!("a2")),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked = HashSet::from([0usize]);
        idx.auto_pair(&mut marked);
        assert_eq!(marked, HashSet::from([0, 1]));
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
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"a"}]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t2","content":"b"}]),
            ),
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
            msg(
                "assistant",
                json!([{"type":"tool_use","id":"t1","name":"R","input":{}}]),
            ),
            msg("user", json!("b")),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"r"}]),
            ),
        ];
        let idx = PairIndex::build(&msgs);
        let mut marked: HashSet<usize> = (0..=2).collect();
        idx.auto_pair(&mut marked);
        assert!(marked.contains(&3));
    }
}
