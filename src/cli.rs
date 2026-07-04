use crate::commands;
use crate::manifest;
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
        /// Base branch to branch from (default: each repo's default branch).
        #[arg(long)]
        base: Option<String>,
    },

    /// List all workspaces.
    List,

    /// Set the active workspace (default target for commands run outside a workspace dir).
    Use { story: String },

    /// Show status of a workspace (or the current/active one).
    Status {
        #[arg(default_value = "")]
        story: String,
    },

    /// Print the workspace root path (use `cd "$(agentws open <story>)"`).
    Open { story: String },

    /// Open the workspace in VS Code via a generated multi-root `.code-workspace`.
    /// `story` is optional and resolved like other commands; a bare `agentws code`
    /// from inside an activated workspace just works.
    Code {
        #[arg(long)]
        story: Option<String>,
    },

    /// Delete one or more workspaces (removes worktrees + manifest; branches kept).
    /// Bare `delete` opens a fuzzy multi-select; dirty worktrees are kept unless
    /// `--force` (a single named workspace keeps the legacy force behavior).
    Delete {
        /// Workspace names to delete. With none, opens the fuzzy picker.
        stories: Vec<String>,
        /// List what would be deleted and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Also delete workspaces with uncommitted changes (kept by default).
        #[arg(long)]
        force: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    // ---- expansion (human-initiated) ----
    /// Add a repo to a workspace (no approval needed).
    Add {
        repo: String,
        #[arg(long)]
        story: Option<String>,
        #[arg(long)]
        base: Option<String>,
    },
    /// Remove a repo from a workspace.
    Remove {
        repo: String,
        #[arg(long)]
        story: Option<String>,
    },

    // ---- expansion (permission-gated) ----
    /// Queue a request to add a repo (resolved with approve/deny).
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

    // ---- integration ----
    /// Run the agentws MCP server over stdio (for agent tool integration).
    Mcp,
    /// Print the MCP server config snippet for an agent.
    McpConfig { agent: String },
    /// Print shell completions for the agentws binary (bash | zsh | fish | elvish | powershell).
    Completions { shell: String },
    /// Show resolved configuration and key paths.
    Config,
    /// Scan configured roots and list discovered repositories.
    Discover,
    /// Print the shell `activate`/`deactivate` integration — `eval "$(agentws init-shell <shell>)"`.
    InitShell { shell: Option<String> },

    /// (hidden) list story names, one per line — used by shell completions.
    #[command(hide = true, name = "_list-stories")]
    ListStories,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New { story, repos, base } => commands::new::run(&story, repos, base),
        Command::List => commands::list::run(),
        Command::Use { story } => commands::use_ws::run(&story),
        Command::Status { story } => {
            let s = (!story.is_empty()).then_some(story);
            commands::status::run(s)
        }
        Command::Open { story } => commands::open::run(&story),
        Command::Code { story } => commands::code::run(story),
        Command::Delete { stories, dry_run, force, yes } => {
            commands::delete::run(stories, dry_run, force, yes)
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
        Command::Config => commands::config::run(),
        Command::Discover => commands::discover::run(),
        Command::InitShell { shell } => commands::init_shell::run(shell),
        Command::ListStories => {
            for s in manifest::list_stories().unwrap_or_default() {
                println!("{s}");
            }
            Ok(())
        }
    }
}
