//! agentws — per-story workspaces for AI coding agents across polyrepo codebases.
//!
//! Library root. The `agentws` binary (`src/main.rs`) is a thin wrapper over
//! [`cli::run`]; the modules are `pub` here so they can be exercised by the
//! in-module unit tests (`#[cfg(test)]`) and the integration tests under `tests/`.

pub mod cli;
pub mod commands;
pub mod config;
pub mod discovery;
pub mod manifest;
pub mod mcp;
pub mod ops;
pub mod picker;
pub mod util;
pub mod worktree;
