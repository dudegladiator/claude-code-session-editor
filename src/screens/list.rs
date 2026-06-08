use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::scan::SessionEntry;
use crate::search;

use super::Transition;

#[derive(PartialEq, Eq)]
pub enum Mode {
    Browse,
    Search,
}

pub struct ListState {
    pub entries: Vec<SessionEntry>,
    pub query: String,
    pub mode: Mode,
    pub table: TableState,
}

impl ListState {
    pub fn new(entries: Vec<SessionEntry>) -> Self {
        let mut table = TableState::default();
        if !entries.is_empty() {
            table.select(Some(0));
        }
        Self {
            entries,
            query: String::new(),
            mode: Mode::Browse,
            table,
        }
    }

    pub fn filtered(&self) -> Vec<&SessionEntry> {
        self.entries
            .iter()
            .filter(|e| search::matches(e, &self.query))
            .collect()
    }

    fn selected_path(&self) -> Option<std::path::PathBuf> {
        let filtered = self.filtered();
        let idx = self.table.selected()?;
        filtered.get(idx).map(|e| e.path.clone())
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len == 0 {
            self.table.select(None);
            return;
        }
        let cur = self.table.selected().unwrap_or(0) as isize;
        let new = (cur + delta).clamp(0, (len as isize) - 1) as usize;
        self.table.select(Some(new));
    }
}

pub fn handle_key(state: &mut ListState, key: KeyEvent) -> Result<Transition> {
    match state.mode {
        Mode::Browse => match key.code {
            KeyCode::Char('q') => return Ok(Transition::Quit),
            KeyCode::Char('j') | KeyCode::Down => state.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => state.move_selection(-1),
            KeyCode::Char('t') => state.table.select(Some(0)),
            KeyCode::Char('b') => {
                let len = state.filtered().len();
                if len > 0 {
                    state.table.select(Some(len - 1));
                }
            }
            KeyCode::Char('/') => {
                state.mode = Mode::Search;
                state.query.clear();
            }
            KeyCode::Enter => {
                if let Some(path) = state.selected_path() {
                    return Ok(Transition::OpenEdit(path));
                }
            }
            _ => {}
        },
        Mode::Search => match key.code {
            KeyCode::Esc => {
                state.mode = Mode::Browse;
                state.query.clear();
                state.table.select(Some(0));
            }
            KeyCode::Enter => {
                state.mode = Mode::Browse;
                state.table.select(Some(0));
            }
            KeyCode::Backspace => {
                state.query.pop();
                state.table.select(Some(0));
            }
            KeyCode::Char(c) => {
                state.query.push(c);
                state.table.select(Some(0));
            }
            _ => {}
        },
    }
    Ok(Transition::None)
}

pub fn render(frame: &mut Frame, state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let title = if state.mode == Mode::Search {
        format!("cc-session — search: {}_", state.query)
    } else if state.query.is_empty() {
        "cc-session — sessions".to_string()
    } else {
        format!("cc-session — filter: {}", state.query)
    };
    let header = Paragraph::new(title).block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    let filtered = state.filtered();
    let rows: Vec<Row> = filtered
        .iter()
        .map(|e| {
            Row::new(vec![
                Cell::from(e.project_slug.clone()),
                Cell::from(e.title.clone()),
                Cell::from(format_mtime(e)),
                Cell::from(human_size(e.size)),
                Cell::from(e.session_id.clone()),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(35),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Min(36),
        ],
    )
    .header(
        Row::new(vec!["project", "title", "modified", "size", "id"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().borders(Borders::ALL))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(table, chunks[1], &mut state.table);

    let footer_text = match state.mode {
        Mode::Browse => "j/k move  t/b top/bottom  /  search  Enter open  q quit",
        Mode::Search => "type to filter  Enter accept  Esc cancel",
    };
    frame.render_widget(Paragraph::new(footer_text), chunks[2]);
}

fn format_mtime(e: &SessionEntry) -> String {
    let dt: DateTime<Local> = e.mtime.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn human_size(b: u64) -> String {
    const K: u64 = 1024;
    if b < K {
        format!("{b}B")
    } else if b < K * K {
        format!("{:.1}K", b as f64 / K as f64)
    } else {
        format!("{:.1}M", b as f64 / (K * K) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn make_entry(project: &str, title: &str) -> SessionEntry {
        SessionEntry {
            project_slug: project.into(),
            session_id: format!("{project}-{title}"),
            title: title.into(),
            mtime: SystemTime::UNIX_EPOCH,
            size: 1024,
            path: PathBuf::from(format!("/tmp/{project}/{title}.jsonl")),
        }
    }

    #[test]
    fn move_selection_clamps() {
        let mut s = ListState::new(vec![make_entry("a", "1"), make_entry("a", "2")]);
        s.move_selection(-5);
        assert_eq!(s.table.selected(), Some(0));
        s.move_selection(10);
        assert_eq!(s.table.selected(), Some(1));
    }

    #[test]
    fn filter_narrows() {
        let mut s = ListState::new(vec![
            make_entry("alpha", "auth"),
            make_entry("beta", "billing"),
        ]);
        s.query = "auth".into();
        assert_eq!(s.filtered().len(), 1);
    }

    #[test]
    fn empty_filter_no_panic_on_enter() {
        let mut s = ListState::new(vec![make_entry("a", "x")]);
        s.query = "zzz".into();
        assert!(s.selected_path().is_none());
    }
}
