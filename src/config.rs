use crate::util::expand_tilde;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

/// Built-in user library containing reusable skills, instruction snippets,
/// and workspace templates.
pub fn library_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("library"))
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

    /// Additional team/shared library roots. Each root uses the same
    /// `skills/`, `agents/`, and `templates/` layout as the built-in library.
    #[serde(default)]
    pub library_dirs: Vec<PathBuf>,

    /// Template applied to a bare `agentws new <story>` invocation.
    #[serde(default)]
    pub default_template: Option<String>,

    /// Preferred structured default configuration (`[default]`).
    #[serde(default)]
    pub default: Defaults,

    /// Paths to symlink from the original repo into each worktree
    /// (e.g. ["node_modules", ".env*"]). Defaults to none.
    #[serde(default)]
    pub symlinks: Vec<String>,

    /// Shell command run inside each worktree right after it is created
    /// (e.g. "npm ci"). Defaults to none.
    #[serde(default)]
    pub post_create: Option<String>,

    /// Per-repository dotenv exposure/consumption rules for cross-repo wiring.
    #[serde(default)]
    pub env: BTreeMap<String, RepoEnv>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEnv {
    /// Dotenv file, relative to the repository worktree.
    #[serde(default = "default_env_file")]
    pub file: PathBuf,
    /// Template name -> dotenv key exported by this repository.
    #[serde(default)]
    pub exposes: BTreeMap<String, String>,
    /// Dotenv key -> template containing `{repo.name}` references.
    #[serde(default)]
    pub consumes: BTreeMap<String, String>,
    /// Fallback values keyed by placeholder name when a producer is absent.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default)]
    pub template: Option<String>,
}

impl Default for RepoEnv {
    fn default() -> Self {
        Self {
            file: default_env_file(),
            exposes: BTreeMap::new(),
            consumes: BTreeMap::new(),
            defaults: BTreeMap::new(),
        }
    }
}

fn default_env_file() -> PathBuf {
    PathBuf::from(".env")
}

impl Config {
    pub fn repo_roots_expanded(&self) -> Vec<PathBuf> {
        self.repo_roots.iter().map(expand_tilde).collect()
    }

    /// Effective library roots in precedence order. The built-in user library
    /// wins over external roots, then `library_dirs` are considered in order.
    pub fn library_roots_expanded(&self) -> Result<Vec<PathBuf>> {
        let mut roots = vec![library_dir()?];
        for root in &self.library_dirs {
            let expanded = expand_tilde(root);
            if !roots.contains(&expanded) {
                roots.push(expanded);
            }
        }
        Ok(roots)
    }

    pub fn effective_default_template(&self) -> Option<&str> {
        self.default
            .template
            .as_deref()
            .or(self.default_template.as_deref())
    }
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading config at {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing config at {}", path.display()))
}

/// Parse a config from a TOML string. Split out from `load` for testability.
pub fn parse(text: &str) -> Result<Config> {
    Ok(toml::from_str(text)?)
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

# Additional shared library roots. Each contains skills/, agents/, templates/.
# library_dirs = ["~/team-agentws-library"]

# Paths to symlink from each original repo into its worktree.
# symlinks = ["node_modules", ".env*"]

# Shell command run inside each worktree right after creation.
# post_create = "npm ci"

# Optional cross-repo env wiring. Example:
# [env.api]
# file = ".env.local"
# exposes = { port = "PORT" }
#
# [env.web]
# file = ".env.local"
# consumes = { API_URL = "http://localhost:{api.port}" }
# defaults = { "api.port" = "5000" }

# Apply a named template to bare `agentws new <story>` commands.
# [default]
# template = "full-stack"
"#;
        fs::write(&path, example)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_is_default() {
        let cfg = parse("").unwrap();
        assert!(cfg.repo_roots.is_empty());
        assert!(cfg.default_base.is_none());
        assert!(cfg.library_dirs.is_empty());
        assert!(cfg.default_template.is_none());
        assert!(cfg.default.template.is_none());
        assert!(cfg.symlinks.is_empty());
        assert!(cfg.post_create.is_none());
    }

    #[test]
    fn parse_roots_and_hooks() {
        let cfg = parse(
            r#"
repo_roots = ["~/work", "/abs/repos"]
symlinks = ["node_modules", ".env*"]
post_create = "npm ci"
default_base = "develop"
library_dirs = ["~/team-agentws"]
default_template = "standard"
"#,
        )
        .unwrap();
        assert_eq!(cfg.repo_roots.len(), 2);
        // tilde stays literal here; expansion only happens in repo_roots_expanded()
        assert_eq!(cfg.repo_roots[0], std::path::PathBuf::from("~/work"));
        assert_eq!(
            cfg.symlinks,
            vec!["node_modules".to_string(), ".env*".to_string()]
        );
        assert_eq!(cfg.post_create.as_deref(), Some("npm ci"));
        assert_eq!(cfg.default_base.as_deref(), Some("develop"));
        assert_eq!(cfg.library_dirs, vec![PathBuf::from("~/team-agentws")]);
        assert_eq!(cfg.default_template.as_deref(), Some("standard"));
        assert_eq!(cfg.effective_default_template(), Some("standard"));
        assert!(cfg.env.is_empty());
    }

    #[test]
    fn parse_unknown_keys_ignored() {
        // no deny_unknown_fields, so unknown fields are silently dropped
        let cfg = parse("bogus_key = 1\n").unwrap();
        assert!(cfg.repo_roots.is_empty());
    }

    #[test]
    fn repo_roots_expanded_resolves_tilde() {
        let Some(home) = home_dir() else {
            return;
        };
        let cfg = parse("repo_roots = [\"~/work\"]\n").unwrap();
        let expanded = cfg.repo_roots_expanded();
        assert_eq!(expanded, vec![home.join("work")]);
    }

    #[test]
    fn library_roots_put_user_library_first_and_dedupe() {
        let mut cfg = Config::default();
        let own = library_dir().unwrap();
        cfg.library_dirs = vec![own.clone(), PathBuf::from("~/team-agentws")];
        let roots = cfg.library_roots_expanded().unwrap();
        assert_eq!(roots[0], own);
        assert_eq!(roots.len(), 2);
        assert!(roots[1].ends_with("team-agentws"));
    }

    #[test]
    fn structured_default_template_is_preferred() {
        let cfg =
            parse("default_template = \"legacy\"\n[default]\ntemplate = \"standard\"\n").unwrap();
        assert_eq!(cfg.effective_default_template(), Some("standard"));
    }

    #[test]
    fn parse_cross_repo_env_wiring() {
        let cfg = parse(
            r#"
[env.api]
file = ".env.local"
exposes = { port = "PORT" }

[env.web]
consumes = { API_URL = "http://localhost:{api.port}" }
defaults = { "api.port" = "5000" }
"#,
        )
        .unwrap();
        assert_eq!(cfg.env["api"].file, PathBuf::from(".env.local"));
        assert_eq!(cfg.env["api"].exposes["port"], "PORT");
        assert_eq!(
            cfg.env["web"].consumes["API_URL"],
            "http://localhost:{api.port}"
        );
        assert_eq!(cfg.env["web"].defaults["api.port"], "5000");
        assert_eq!(cfg.env["web"].file, PathBuf::from(".env"));
    }
}
