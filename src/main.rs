use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod app;
mod cli;
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
    /// Override the Claude Code projects directory (defaults to ~/.claude/projects).
    #[arg(long, value_name = "DIR", global = true)]
    projects_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List sessions across projects.
    List {
        /// Filter to a specific project slug (substring match).
        #[arg(long)]
        project: Option<String>,
        /// Output JSON instead of a human table.
        #[arg(long)]
        json: bool,
        /// Limit output to N entries.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Fuzzy search sessions by project + title. Same matcher the TUI uses.
    Search {
        /// Query string. Subsequence match, case-insensitive.
        query: String,
        /// Limit output to N best matches.
        #[arg(long)]
        limit: Option<usize>,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show messages in a session.
    Show {
        /// Session id (uuid), file path, or substring of either.
        target: String,
        /// Show every message including system, tool blocks, attachments.
        #[arg(long)]
        include_hidden: bool,
        /// Show full message text instead of a 400-char preview.
        #[arg(long)]
        full: bool,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// Delete messages from a session by index. Always forks: writes a NEW
    /// session file with a new UUID and leaves the original untouched.
    /// Output prints a `claude --resume <new-id>` command to continue.
    /// Auto-pairs tool_use/tool_result and re-links parentUuids.
    Delete {
        /// Session id, file path, or substring of either.
        target: String,
        /// Comma-separated 0-based indices to delete (e.g. 3,5,7).
        #[arg(long, value_delimiter = ',')]
        indices: Vec<usize>,
        /// Delete the first N messages.
        #[arg(long)]
        from_top: Option<usize>,
        /// Delete the last N messages.
        #[arg(long)]
        from_bottom: Option<usize>,
        /// Inclusive range "lo..hi" (0-based).
        #[arg(long)]
        range: Option<String>,
        /// Show what would be removed without writing the fork file.
        #[arg(long)]
        dry_run: bool,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print metadata about a session.
    Info {
        target: String,
        #[arg(long)]
        json: bool,
    },
    /// Show the heaviest conversational turns by token count, descending.
    /// Use this to pick which turns to drop with `delete`.
    Heatmap {
        /// Session id, file path, or substring of either.
        target: String,
        /// Limit output to N turns. Default 20.
        #[arg(long)]
        limit: Option<usize>,
        /// Output JSON.
        #[arg(long)]
        json: bool,
    },
    /// Self-update to the latest release (or a specific version).
    Update {
        /// Install a specific tag (e.g. `v1.0.0`). Default: latest.
        #[arg(long)]
        version: Option<String>,
    },
    /// Print a structured agent guide: workflow, JSON shapes, env vars,
    /// exit codes. Designed for LLMs and scripts to read once and operate
    /// autonomously.
    AgentGuide,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let projects_dir = match &cli.projects_dir {
        Some(p) => p.clone(),
        None => default_projects_dir()?,
    };

    match cli.command {
        None => app::run(app::Config {
            projects_dir: Some(projects_dir),
        }),
        Some(Command::List {
            project,
            json,
            limit,
        }) => cli::list(&projects_dir, project.as_deref(), json, limit),
        Some(Command::Search { query, limit, json }) => {
            cli::search(&projects_dir, &query, limit, json)
        }
        Some(Command::Show {
            target,
            include_hidden,
            full,
            json,
        }) => cli::show(&projects_dir, &target, include_hidden, full, json),
        Some(Command::Delete {
            target,
            indices,
            from_top,
            from_bottom,
            range,
            dry_run,
            json,
        }) => cli::delete(
            &projects_dir,
            &target,
            cli::DeleteSpec {
                indices,
                from_top,
                from_bottom,
                range,
            },
            dry_run,
            json,
        ),
        Some(Command::Info { target, json }) => cli::info(&projects_dir, &target, json),
        Some(Command::Heatmap {
            target,
            limit,
            json,
        }) => cli::heatmap(&projects_dir, &target, limit, json),
        Some(Command::Update { version }) => cli::update(version.as_deref()),
        Some(Command::AgentGuide) => {
            print!("{}", cli::AGENT_GUIDE);
            Ok(())
        }
    }
}

fn default_projects_dir() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not resolve home dir"))?;
    Ok(home.join(".claude").join("projects"))
}
