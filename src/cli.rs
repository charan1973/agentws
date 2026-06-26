use crate::commands;
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agentws",
    version,
    about = "Per-story workspaces for AI coding agents across polyrepo codebases"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a new workspace for a story.
    New {
        /// Story name (becomes the branch suffix, e.g. feat/<name>).
        story: String,

        /// Pre-select repos by name (comma-separated); skips the interactive picker.
        #[arg(long, value_delimiter = ',')]
        repos: Option<Vec<String>>,

        /// Agent to launch (claude | codex | opencode | pi).
        #[arg(long)]
        agent: Option<String>,

        /// Base branch to branch from (default: each repo's default branch).
        #[arg(long)]
        base: Option<String>,

        /// Don't launch the agent; just create the workspace.
        #[arg(long)]
        no_launch: bool,
    },

    /// List all workspaces.
    List,

    /// Show status of a workspace (or the current one).
    Status {
        /// Story name. If omitted, inferred from the current directory.
        #[arg(default_value = "")]
        story: String,
    },

    /// Print the workspace root path (use `cd "$(agentws open <story>)"`).
    Open {
        story: String,
    },

    /// Delete a workspace (removes worktrees + manifest; branches are kept).
    Delete {
        story: String,

        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New {
            story,
            repos,
            agent,
            base,
            no_launch,
        } => commands::new::run(&story, repos, agent, base, no_launch),
        Command::List => commands::list::run(),
        Command::Status { story } => {
            let s = if story.is_empty() { None } else { Some(story) };
            commands::status::run(s)
        }
        Command::Open { story } => commands::open::run(&story),
        Command::Delete { story, yes } => commands::delete::run(&story, yes),
    }
}
