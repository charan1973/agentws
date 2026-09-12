use crate::commands;
use crate::manifest;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Pre-select library skills by name (comma-separated).
        #[arg(long, value_delimiter = ',')]
        skills: Option<Vec<String>>,
        /// Pre-select reusable AGENTS.md snippets by name (comma-separated).
        #[arg(long = "agents-md", value_delimiter = ',')]
        agents_md: Option<Vec<String>>,
        /// Apply a named workspace template.
        #[arg(long)]
        template: Option<String>,
        /// Copy selected library skills into the workspace instead of symlinking.
        #[arg(long)]
        copy: bool,
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
    /// Re-read exposed values and refresh cross-repo dotenv wiring.
    Rewire {
        #[arg(long)]
        story: Option<String>,
    },
    /// Rebuild root instructions and re-sync managed skill/settings composition.
    Refresh {
        #[arg(long)]
        story: Option<String>,
    },

    /// Manage reusable skills, AGENTS.md snippets, and templates.
    Library {
        #[command(subcommand)]
        action: LibraryAction,
    },

    /// Manage named workspace templates.
    Template {
        #[command(subcommand)]
        action: TemplateAction,
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
    /// Show the append-only repo request history for a workspace.
    History {
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
    /// Watch for repo requests and approve or deny them interactively.
    /// Use --tmux to open the watcher in a dedicated pane.
    Approvals {
        #[arg(long)]
        story: Option<String>,
        /// Open the approval watcher in a new tmux pane and return immediately.
        #[arg(long)]
        tmux: bool,
        /// How often to check for new requests.
        #[arg(long, default_value_t = 500)]
        poll_ms: u64,
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
    /// Install agentws tools into one or more agent harnesses for a workspace.
    Integrate {
        /// Agent names: pi, claude, codex, opencode, or all.
        #[arg(required = true)]
        agents: Vec<String>,
        #[arg(long)]
        story: Option<String>,
        /// Replace a conflicting managed entry.
        #[arg(long)]
        force: bool,
    },
    /// Run a command with hard filesystem isolation (macOS, opt-in).
    Sandbox {
        #[arg(long)]
        story: Option<String>,
        /// Permit network access (denied by default).
        #[arg(long)]
        allow_network: bool,
        /// Permit reads from an additional path (repeatable).
        #[arg(long)]
        allow_read: Vec<PathBuf>,
        /// Permit reads and writes at an additional path (repeatable).
        #[arg(long)]
        allow_write: Vec<PathBuf>,
        /// Command and arguments; place them after `--`.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
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
    /// (hidden) invoke an agent-facing tool without JSON-RPC (used by pi).
    #[command(hide = true, name = "_tool-call")]
    ToolCall { name: String, arguments: String },
}

#[derive(Subcommand)]
pub enum LibraryAction {
    /// List effective items from the built-in and external libraries.
    List,
    /// Add a skill directory, Markdown snippet, or TOML template.
    Add { path: PathBuf },
    /// Remove an item from the built-in library.
    Remove {
        name: String,
        /// Disambiguate duplicate names: skill, agents-md, or template.
        #[arg(long)]
        kind: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TemplateAction {
    /// List effective templates.
    List,
    /// Print a template.
    Show { name: String },
    /// Snapshot a workspace as a reusable template.
    Save {
        name: String,
        #[arg(long)]
        from: Option<String>,
    },
    /// Delete a template from the built-in library.
    Delete { name: String },
    /// Open a template in $VISUAL or $EDITOR and validate it on exit.
    Edit { name: String },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::New {
            story,
            repos,
            base,
            skills,
            agents_md,
            template,
            copy,
        } => {
            commands::new::run_with_options(&story, repos, base, skills, agents_md, template, copy)
        }
        Command::List => commands::list::run(),
        Command::Use { story } => commands::use_ws::run(&story),
        Command::Status { story } => {
            let s = (!story.is_empty()).then_some(story);
            commands::status::run(s)
        }
        Command::Open { story } => commands::open::run(&story),
        Command::Code { story } => commands::code::run(story),
        Command::Delete {
            stories,
            dry_run,
            force,
            yes,
        } => commands::delete::run(stories, dry_run, force, yes),
        Command::Add { repo, story, base } => commands::add::run(story, repo, base),
        Command::Remove { repo, story } => commands::remove::run(story, repo),
        Command::Rewire { story } => commands::env::rewire(story),
        Command::Refresh { story } => commands::refresh::run(story),
        Command::Library { action } => match action {
            LibraryAction::List => commands::library::list(),
            LibraryAction::Add { path } => commands::library::add(path),
            LibraryAction::Remove { name, kind } => commands::library::remove(name, kind),
        },
        Command::Template { action } => match action {
            TemplateAction::List => commands::template::list(),
            TemplateAction::Show { name } => commands::template::show(name),
            TemplateAction::Save { name, from } => commands::template::save(name, from),
            TemplateAction::Delete { name } => commands::template::delete(name),
            TemplateAction::Edit { name } => commands::template::edit(name),
        },
        Command::Request {
            repo,
            story,
            reason,
        } => commands::expand::request(story, repo, reason),
        Command::Pending { story } => commands::expand::pending(story),
        Command::History { story } => commands::expand::history(story),
        Command::Approve { id_or_repo, story } => commands::expand::approve(story, id_or_repo),
        Command::Deny { id_or_repo, story } => commands::expand::deny(story, id_or_repo),
        Command::Approvals {
            story,
            tmux,
            poll_ms,
        } => commands::approvals::run(story, tmux, poll_ms),
        Command::Archive { story } => commands::lifecycle::archive(story),
        Command::Restore { story } => commands::lifecycle::restore(story),
        Command::Mcp => crate::mcp::run(),
        Command::McpConfig { agent } => commands::mcp_config::run(&agent),
        Command::Integrate {
            agents,
            story,
            force,
        } => commands::integrate::run(story, agents, force),
        Command::Sandbox {
            story,
            allow_network,
            allow_read,
            allow_write,
            command,
        } => commands::sandbox::run(story, allow_network, allow_read, allow_write, command),
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
        Command::ToolCall { name, arguments } => {
            let args: serde_json::Value = serde_json::from_str(&arguments)?;
            println!("{}", crate::mcp::invoke(&name, &args)?);
            Ok(())
        }
    }
}
