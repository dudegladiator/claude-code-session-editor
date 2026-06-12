use std::path::PathBuf;

use crate::screens;

pub struct Config {
    pub projects_dir: Option<PathBuf>,
}

pub enum Screen {
    List(screens::list::ListState),
    Edit(Box<screens::edit::EditState>),
}

pub struct App {
    pub screen: Screen,
    pub projects_dir: PathBuf,
    pub should_quit: bool,
}

pub fn run(cfg: Config) -> anyhow::Result<()> {
    let projects_dir = match cfg.projects_dir {
        Some(p) => p,
        None => default_projects_dir()?,
    };

    let entries = crate::scan::scan(&projects_dir)?;
    let list_state = screens::list::ListState::new(entries);

    let mut app = App {
        screen: Screen::List(list_state),
        projects_dir,
        should_quit: false,
    };

    screens::run_event_loop(&mut app)
}

fn default_projects_dir() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not resolve home dir"))?;
    Ok(home.join(".claude").join("projects"))
}
