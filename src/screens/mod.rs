pub mod edit;
pub mod list;

use std::io::stdout;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, Screen};

pub fn run_event_loop(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = main_loop(&mut terminal, app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn main_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| match &mut app.screen {
            Screen::List(state) => list::render(frame, state),
            Screen::Edit(state) => edit::render(frame, state.as_mut()),
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key)?;
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let transition = match &mut app.screen {
        Screen::List(state) => list::handle_key(state, key),
        Screen::Edit(state) => edit::handle_key(state.as_mut(), key, app.force),
    }?;
    apply_transition(app, transition)
}

pub enum Transition {
    None,
    Quit,
    OpenEdit(std::path::PathBuf),
    BackToList,
}

fn apply_transition(app: &mut App, t: Transition) -> Result<()> {
    match t {
        Transition::None => {}
        Transition::Quit => app.should_quit = true,
        Transition::OpenEdit(path) => {
            let session = crate::session::Session::load(&path)?;
            let pairing = crate::pairing::PairIndex::build(&session.messages);
            let state = edit::EditState::new(session, pairing);
            app.screen = Screen::Edit(Box::new(state));
        }
        Transition::BackToList => {
            // Re-scan to refresh sizes/mtimes after potential save.
            let entries = crate::scan::scan(&app.projects_dir)?;
            app.screen = Screen::List(list::ListState::new(entries));
        }
    }
    Ok(())
}
