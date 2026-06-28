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

    /// Set the active workspace (default target for commands run outside a workspace dir).
    Use {
        story: String,
    },

    /// Show status of a workspace (or the current one if run from within it).
    Status {
        #[arg(default_value = "")]
        story: String,
    },

    /// Print the workspace root path (use `cd "$(agentws open <story>)"`).
    Open { story: String },

    /// Delete a workspace (removes worktrees + manifest; branches are kept).
    Delete {
        story: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// (Re)launch the agent for an existing workspace.
    Launch {
        #[arg(default_value = "")]
        story: String,
        /// Override the agent to launch.
        #[arg(long)]
        agent: Option<String>,
    },

    // ---- expansion (P1 human-initiated) ----
    /// Add a repo to a workspace (human-initiated; no approval needed).
    Add {
        /// Repository name to add.
        repo: String,
        /// Story (defaults to the workspace of the current directory).
        #[arg(long)]
        story: Option<String>,
        /// Base branch (default: the repo's default branch).
        #[arg(long)]
        base: Option<String>,
    },

    /// Remove a repo from a workspace.
    Remove {
        repo: String,
        #[arg(long)]
        story: Option<String>,
    },

    // ---- expansion (P2 permission-gated) ----
    /// Queue a request to add a repo (agent-facing; resolved with approve/deny).
    Request {
        repo: String,
        #[arg(long)]
        story: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// List pending repo requests for a workspace.
    Pending {
        #[arg(long)]
        story: Option<String>,
    },
    /// Approve a pending request (by id or repo name): creates the worktree.
    Approve {
        /// Request id or repo name.
        id_or_repo: String,
        #[arg(long)]
        story: Option<String>,
    },
    /// Deny a pending request (by id or repo name).
    Deny {
        id_or_repo: String,
        #[arg(long)]
        story: Option<String>,
    },

    // ---- lifecycle ----
    /// Archive a workspace: remove worktrees but keep the manifest + branches.
    Archive { story: String },
    /// Restore an archived workspace: recreate the worktrees.
    Restore { story: String },

    // ---- agent integration ----
    /// Run the agentws MCP server over stdio (for agent tool integration).
    Mcp,
    /// Print the MCP server config snippet for an agent.
    McpConfig { agent: String },
    /// Print shell completions (bash | zsh | fish | elvish | powershell).
    Completions { shell: String },
    /// Print a shell helper to `cd` into a workspace.
    InitShell { shell: String },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { story, repos, agent, base, no_launch } => {
            commands::new::run(&story, repos, agent, base, no_launch)
        }
        Command::List => commands::list::run(),
        Command::Use { story } => commands::use_ws::run(&story),
        Command::Status { story } => {
            let s = (!story.is_empty()).then_some(story);
            commands::status::run(s)
        }
        Command::Open { story } => commands::open::run(&story),
        Command::Delete { story, yes } => commands::delete::run(&story, yes),
        Command::Launch { story, agent } => {
            let s = (!story.is_empty()).then_some(story);
            commands::launch::run(s, agent)
        }
        Command::Add { repo, story, base } => commands::add::run(story, repo, base),
        Command::Remove { repo, story } => commands::remove::run(story, repo),
        Command::Request { repo, story, reason } => {
            commands::expand::request(story, repo, reason)
        }
        Command::Pending { story } => commands::expand::pending(story),
        Command::Approve { id_or_repo, story } => {
            commands::expand::approve(story, id_or_repo)
        }
        Command::Deny { id_or_repo, story } => {
            commands::expand::deny(story, id_or_repo)
        }
        Command::Archive { story } => commands::lifecycle::archive(story),
        Command::Restore { story } => commands::lifecycle::restore(story),
        Command::Mcp => crate::mcp::run(),
        Command::McpConfig { agent } => commands::mcp_config::run(&agent),
        Command::Completions { shell } => commands::completions::run(&shell),
        Command::InitShell { shell } => commands::init_shell::run(&shell),
    }
}
