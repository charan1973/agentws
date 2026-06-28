mod agent;
mod cli;
mod commands;
mod config;
mod discovery;
mod manifest;
mod mcp;
mod ops;
mod picker;
mod util;
mod worktree;

fn main() -> anyhow::Result<()> {
    cli::run()
}
