use crate::util::expand_tilde;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Where per-story workspace directories live: `~/.agentws/<story>`.
pub fn workspaces_root() -> Result<PathBuf> {
    Ok(home_dir().context("no home directory")?.join(".agentws"))
}

pub fn workspace_dir(story: &str) -> Result<PathBuf> {
    Ok(workspaces_root()?.join(story))
}

pub fn config_dir() -> Result<PathBuf> {
    Ok(home_dir()
        .context("no home directory")?
        .join(".config")
        .join("agentws"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

#[allow(dead_code)]
pub fn cache_dir() -> Result<PathBuf> {
    Ok(home_dir()
        .context("no home directory")?
        .join(".cache")
        .join("agentws"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Roots scanned for git repositories.
    #[serde(default)]
    pub repo_roots: Vec<PathBuf>,

    /// Default base branch (default: each repo's default branch).
    #[serde(default)]
    pub default_base: Option<String>,

    /// Paths to symlink from the original repo into each worktree
    /// (e.g. ["node_modules", ".env*"]). Defaults to none.
    #[serde(default)]
    pub symlinks: Vec<String>,

    /// Shell command run inside each worktree right after it is created
    /// (e.g. "npm ci"). Defaults to none.
    #[serde(default)]
    pub post_create: Option<String>,
}

impl Config {
    pub fn repo_roots_expanded(&self) -> Vec<PathBuf> {
        self.repo_roots.iter().map(expand_tilde).collect()
    }
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading config at {}", path.display()))?;
    let cfg: Config = toml::from_str(&text)
        .with_context(|| format!("parsing config at {}", path.display()))?;
    Ok(cfg)
}

/// Returns the config path, writing a documented example if it doesn't exist yet.
pub fn ensure_example() -> Result<PathBuf> {
    let path = config_path()?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let example = r#"# agentws configuration
# Roots that will be scanned for git repositories.
repo_roots = ["~/work"]

# Default base branch to create story branches from (default: each repo's default branch).
# default_base = "main"

# Paths to symlink from each original repo into its worktree.
# symlinks = ["node_modules", ".env*"]

# Shell command run inside each worktree right after creation.
# post_create = "npm ci"
"#;
        fs::write(&path, example)?;
    }
    Ok(path)
}
