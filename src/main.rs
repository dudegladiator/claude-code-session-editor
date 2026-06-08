use clap::Parser;

mod app;
mod io;
mod pairing;
mod scan;
mod screens;
mod search;
mod session;
mod tokens;

#[derive(Parser, Debug)]
#[command(name = "cc-session", version, about = "Claude Code Session Editor")]
struct Cli {
    /// Bypass concurrent-open detection when saving.
    #[arg(long)]
    force: bool,

    /// Override the Claude Code projects directory (defaults to ~/.claude/projects).
    #[arg(long, value_name = "DIR")]
    projects_dir: Option<std::path::PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    app::run(app::Config {
        force: cli.force,
        projects_dir: cli.projects_dir,
    })
}
