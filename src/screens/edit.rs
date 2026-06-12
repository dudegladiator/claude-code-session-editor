use std::collections::HashSet;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

use crate::cli::fork;
use crate::pairing::PairIndex;
use crate::session::{Message, Session};
use crate::tokens::TokenCounter;

use super::Transition;

#[derive(PartialEq, Eq)]
pub enum Modal {
    None,
    ConfirmSave { count: usize },
    SaveResult(String),
}

pub struct EditState {
    pub session: Session,
    pub pairing: PairIndex,
    pub tokens: TokenCounter,
    /// Index into `visible` (NOT into session.messages).
    pub selected: usize,
    /// Real message indices the user marked for delete.
    pub marked: HashSet<usize>,
    /// Real message indices that are shown on screen (filtered).
    pub visible: Vec<usize>,
    pub list_state: ListState,
    pub banner: Option<(String, std::time::Instant)>,
    pub modal: Modal,
    pub anchor: Option<usize>,
}

impl EditState {
    pub fn new(session: Session, pairing: PairIndex) -> Self {
        let visible = compute_visible(&session.messages);
        let mut list_state = ListState::default();
        if !visible.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            session,
            pairing,
            tokens: TokenCounter::new(),
            selected: 0,
            marked: HashSet::new(),
            visible,
            list_state,
            banner: None,
            modal: Modal::None,
            anchor: None,
        }
    }

    /// Real message index for the currently selected visible row.
    fn current_real_idx(&self) -> Option<usize> {
        self.visible.get(self.selected).copied()
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.visible.len();
        if len == 0 {
            return;
        }
        let cur = self.selected as isize;
        let new = (cur + delta).clamp(0, (len as isize) - 1) as usize;
        self.selected = new;
        self.list_state.select(Some(new));
    }

    fn toggle_mark_current(&mut self) {
        let Some(idx) = self.current_real_idx() else {
            return;
        };
        if self.marked.contains(&idx) {
            self.marked.remove(&idx);
            if let Some(ids) = self.pairing.by_msg.get(&idx) {
                for id in ids {
                    if let Some(entry) = self.pairing.by_id.get(id) {
                        self.marked.remove(&entry.use_idx);
                        if let Some(r) = entry.result_idx {
                            self.marked.remove(&r);
                        }
                    }
                }
            }
        } else {
            self.marked.insert(idx);
            let added = self.pairing.auto_pair(&mut self.marked);
            if added > 0 {
                self.banner_set(format!("auto-paired {added} tool call(s)"));
            }
        }
    }

    fn mark_range_to_anchor(&mut self) {
        let anchor = match self.anchor.take() {
            Some(a) => a,
            None => return,
        };
        let (lo, hi) = if anchor <= self.selected {
            (anchor, self.selected)
        } else {
            (self.selected, anchor)
        };
        // Translate visible row range to real-index range; mark everything
        // between (inclusive) so hidden tool/system messages between two
        // visible user/assistant rows also get deleted.
        let real_lo = self.visible[lo];
        let real_hi = self.visible[hi];
        for i in real_lo..=real_hi {
            self.marked.insert(i);
        }
        let added = self.pairing.auto_pair(&mut self.marked);
        self.banner_set(format!(
            "range marked: {} msg(s); auto-paired {} extra",
            hi - lo + 1,
            added
        ));
    }

    #[allow(dead_code)]
    fn mark_to_top(&mut self) {
        let Some(real_idx) = self.current_real_idx() else {
            return;
        };
        for i in 0..=real_idx {
            self.marked.insert(i);
        }
        let added = self.pairing.auto_pair(&mut self.marked);
        self.banner_set(format!(
            "marked top→here ({}); auto-paired {} extra",
            self.selected + 1,
            added
        ));
    }

    #[allow(dead_code)]
    fn mark_to_bottom(&mut self) {
        let Some(real_idx) = self.current_real_idx() else {
            return;
        };
        let len = self.session.messages.len();
        for i in real_idx..len {
            self.marked.insert(i);
        }
        let added = self.pairing.auto_pair(&mut self.marked);
        self.banner_set(format!(
            "marked here→bottom ({}); auto-paired {} extra",
            self.visible.len().saturating_sub(self.selected),
            added
        ));
    }

    fn banner_set(&mut self, msg: String) {
        self.banner = Some((msg, std::time::Instant::now()));
    }

    fn banner_visible(&self) -> Option<&str> {
        let (text, when) = self.banner.as_ref()?;
        if when.elapsed() < std::time::Duration::from_secs(3) {
            Some(text.as_str())
        } else {
            None
        }
    }

    fn perform_save(&mut self) -> Result<()> {
        let removed = self.marked.len();
        match fork::fork_session(&self.session, &self.marked) {
            Ok(out) => {
                self.modal = Modal::SaveResult(format!(
                    "Forked. {} message(s) removed.\nresume: claude --resume {}\nfile: {}",
                    removed,
                    out.new_session_id,
                    out.new_path.display()
                ));
                self.marked.clear();
                Ok(())
            }
            Err(e) => {
                self.modal = Modal::SaveResult(format!("Fork failed: {e}"));
                Ok(())
            }
        }
    }
}

pub fn handle_key(state: &mut EditState, key: KeyEvent) -> Result<Transition> {
    match &state.modal {
        Modal::ConfirmSave { count } => {
            let count = *count;
            match key.code {
                KeyCode::Char('y') => {
                    state.modal = Modal::None;
                    if count == 0 {
                        state.modal = Modal::SaveResult("no changes to save".into());
                    } else {
                        state.perform_save()?;
                    }
                }
                _ => state.modal = Modal::None,
            }
            return Ok(Transition::None);
        }
        Modal::SaveResult(_) => {
            state.modal = Modal::None;
            return Ok(Transition::None);
        }
        Modal::None => {}
    }

    match key.code {
        // q / Esc: always return to list immediately; pending marks are dropped.
        KeyCode::Char('q') | KeyCode::Esc => return Ok(Transition::BackToList),
        KeyCode::Char('j') | KeyCode::Down => state.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => state.move_selection(-1),
        KeyCode::Char('t') => {
            state.selected = 0;
            state.list_state.select(Some(0));
        }
        KeyCode::Char('b') => {
            let len = state.visible.len();
            if len > 0 {
                state.selected = len - 1;
                state.list_state.select(Some(state.selected));
            }
        }
        KeyCode::Char('d') => {
            if state.anchor.is_some() {
                state.mark_range_to_anchor();
            } else {
                state.toggle_mark_current();
            }
        }
        KeyCode::Char('v') => {
            state.anchor = Some(state.selected);
            state.banner_set("range anchor set; move and press 'd' to mark range".into());
        }
        KeyCode::Char('s') => {
            state.modal = Modal::ConfirmSave {
                count: state.marked.len(),
            };
        }
        _ => {}
    }
    Ok(Transition::None)
}

pub fn render(frame: &mut Frame, state: &mut EditState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let header_text = format!(
        "cc-session — {}  ({} marked, {} visible / {} total)",
        state.session.path.display(),
        state.marked.len(),
        state.visible.len(),
        state.session.messages.len()
    );
    let header = Paragraph::new(header_text).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = state
        .visible
        .iter()
        .map(|&real_idx| build_item(real_idx, &state.session.messages[real_idx], state))
        .collect();

    let list = List::new(items)
        .block(Block::default())
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, chunks[1], &mut state.list_state);

    let footer_text = match state.banner_visible() {
        Some(b) => b.to_string(),
        None => "j/k move  t/b top/bottom  d mark  v range  s save  q back".into(),
    };
    frame.render_widget(
        Paragraph::new(footer_text).wrap(Wrap { trim: true }),
        chunks[2],
    );

    render_modal(frame, state, frame.area());
}

fn build_item(idx: usize, msg: &Message, state: &EditState) -> ListItem<'static> {
    let role = msg
        .message
        .as_ref()
        .and_then(|b| b.role.as_deref())
        .or(msg.r#type.as_deref())
        .unwrap_or("?")
        .to_string();
    let body = summarize_content(msg);
    let token_count = state.tokens.count(idx, msg);
    let ts = msg.timestamp.clone().unwrap_or_default();

    let role_color = match role.as_str() {
        "user" => Color::Cyan,
        "assistant" => Color::Green,
        "system" => Color::Yellow,
        _ => Color::Gray,
    };

    let marker = if state.marked.contains(&idx) {
        "✗ "
    } else {
        "  "
    };

    let body_style = if state.marked.contains(&idx) {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(marker, Style::default().fg(Color::Red)),
        Span::styled(
            format!("[{role}] "),
            Style::default().fg(role_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{token_count}t  "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(ts, Style::default().fg(Color::DarkGray)),
    ]));
    if body.is_empty() {
        lines.push(Line::from(""));
    } else {
        for ln in body.split('\n') {
            lines.push(Line::from(Span::styled(ln.to_string(), body_style)));
        }
    }
    lines.push(Line::from(""));
    ListItem::new(lines)
}

/// Extract only plain-text content (skip tool_use, tool_result, system_reminder
/// stuffing, etc.). Returns None if the message has no human-readable text.
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

fn summarize_content(msg: &Message) -> String {
    let raw = extract_plain_text(msg).unwrap_or_default();
    let flat: String = raw
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= 400 {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(400).collect();
        s.push('…');
        s
    }
}

/// Build the list of message indices to display: only user/assistant messages
/// that carry actual human-readable text. Hides system, attachments, tool blocks,
/// snapshots, command-message wrappers, and other pure-metadata entries.
fn compute_visible(messages: &[Message]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        let role = msg
            .message
            .as_ref()
            .and_then(|b| b.role.as_deref())
            .or(msg.r#type.as_deref())
            .unwrap_or("");
        if role != "user" && role != "assistant" {
            continue;
        }
        let Some(text) = extract_plain_text(msg) else {
            continue;
        };
        // Filter out pure tooling / harness wrappers that present as text but
        // carry no real conversation.
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_harness_wrapper(trimmed) {
            continue;
        }
        out.push(i);
    }
    out
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

fn render_modal(frame: &mut Frame, state: &EditState, area: Rect) {
    let (title, body) = match &state.modal {
        Modal::None => return,
        Modal::ConfirmSave { count } => (
            "fork?",
            format!(
                "drop {count} message(s) and write a new session file?\noriginal stays untouched.\n[y] confirm   any other key cancels"
            ),
        ),
        Modal::SaveResult(msg) => ("result", msg.clone()),
    };
    let popup = centered_rect(60, 30, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));
    let para = Paragraph::new(body).block(block).wrap(Wrap { trim: true });
    frame.render_widget(para, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Message, MessageBody, Session};
    use serde_json::json;

    fn msg(role: &str, content: serde_json::Value) -> Message {
        Message {
            r#type: Some(role.into()),
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

    fn make_state(msgs: Vec<Message>) -> EditState {
        let session = Session {
            path: std::path::PathBuf::from("/tmp/x.jsonl"),
            messages: msgs,
            trailing_newline: true,
        };
        let pairing = PairIndex::build(&session.messages);
        EditState::new(session, pairing)
    }

    // Note: `selected` indexes into `state.visible` (filtered rows), not into
    // the full messages array. Tests use only user/assistant text messages so
    // visible == [0, 1, 2, ...].

    #[test]
    fn toggle_mark_pairs_tool_use_and_result() {
        let mut s = make_state(vec![
            msg("user", json!("ask")),
            // assistant tool_use is hidden (no plain text); insert a wrapper
            // that has both text + tool_use so it is visible.
            msg(
                "assistant",
                json!([
                    {"type":"text","text":"calling tool"},
                    {"type":"tool_use","id":"t1","name":"R","input":{}}
                ]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}]),
            ),
        ]);
        // Visible rows: user(0), assistant(1). tool_result(2) is hidden.
        assert_eq!(s.visible, vec![0, 1]);
        s.selected = 1;
        s.toggle_mark_current();
        // marking the assistant should auto-pair the hidden tool_result too.
        assert!(s.marked.contains(&1));
        assert!(s.marked.contains(&2));
    }

    #[test]
    fn toggle_twice_clears_pair() {
        let mut s = make_state(vec![
            msg(
                "assistant",
                json!([
                    {"type":"text","text":"calling"},
                    {"type":"tool_use","id":"t1","name":"R","input":{}}
                ]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t1","content":"ok"}]),
            ),
        ]);
        // Only assistant is visible.
        assert_eq!(s.visible, vec![0]);
        s.selected = 0;
        s.toggle_mark_current();
        s.toggle_mark_current();
        assert!(s.marked.is_empty());
    }

    #[test]
    fn mark_to_top_inclusive() {
        let mut s = make_state(vec![
            msg("user", json!("a")),
            msg("user", json!("b")),
            msg("user", json!("c")),
        ]);
        s.selected = 1;
        s.mark_to_top();
        assert!(s.marked.contains(&0));
        assert!(s.marked.contains(&1));
        assert!(!s.marked.contains(&2));
    }

    #[test]
    fn mark_to_bottom_inclusive() {
        let mut s = make_state(vec![
            msg("user", json!("a")),
            msg("user", json!("b")),
            msg("user", json!("c")),
        ]);
        s.selected = 1;
        s.mark_to_bottom();
        assert!(!s.marked.contains(&0));
        assert!(s.marked.contains(&1));
        assert!(s.marked.contains(&2));
    }

    #[test]
    fn move_selection_clamps() {
        let mut s = make_state(vec![msg("user", json!("a")), msg("user", json!("b"))]);
        s.move_selection(-5);
        assert_eq!(s.selected, 0);
        s.move_selection(50);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn range_marks_inclusive() {
        let mut s = make_state(vec![
            msg("user", json!("a")),
            msg("user", json!("b")),
            msg("user", json!("c")),
            msg("user", json!("d")),
        ]);
        s.anchor = Some(1);
        s.selected = 3;
        s.mark_range_to_anchor();
        assert!(s.marked.contains(&1));
        assert!(s.marked.contains(&2));
        assert!(s.marked.contains(&3));
    }

    #[test]
    fn system_and_tool_messages_hidden() {
        let s = make_state(vec![
            msg("system", json!("init")),
            msg("user", json!("hello")),
            msg(
                "assistant",
                json!([{"type":"tool_use","id":"t","name":"R","input":{}}]),
            ),
            msg(
                "user",
                json!([{"type":"tool_result","tool_use_id":"t","content":"x"}]),
            ),
            msg("assistant", json!("real answer")),
        ]);
        assert_eq!(s.visible, vec![1, 4]);
    }

    #[test]
    fn harness_wrappers_hidden() {
        let s = make_state(vec![
            msg(
                "user",
                json!("<local-command-caveat>boilerplate</local-command-caveat>"),
            ),
            msg("user", json!("<bash-input>ls</bash-input>")),
            msg("user", json!("real question")),
        ]);
        assert_eq!(s.visible, vec![2]);
    }
}
