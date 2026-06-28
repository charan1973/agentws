use crate::config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub story: String,
    pub root: PathBuf,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub agent: Option<AgentRef>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub requests: Vec<RepoRequest>,
    /// Set true after `archive` removes the worktrees.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRef {
    pub kind: String,
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub origin: PathBuf,
    pub worktree: PathBuf,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRequest {
    pub id: String,
    pub repo: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// pending | approved | denied
    pub status: String,
    /// agent | human
    pub by: String,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub resolved: Option<DateTime<Utc>>,
}

pub fn root_for(story: &str) -> Result<PathBuf> {
    config::workspace_dir(story)
}

pub fn manifest_path_for(story: &str) -> Result<PathBuf> {
    Ok(root_for(story)?.join("workspace.json"))
}

pub fn exists(story: &str) -> bool {
    manifest_path_for(story)
        .map(|p| p.exists())
        .unwrap_or(false)
}

pub fn save(ws: &Workspace) -> Result<()> {
    fs::create_dir_all(&ws.root)?;
    let path = ws.root.join("workspace.json");
    let lock_path = ws.root.join(".workspace.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .context("opening manifest lock")?;
    lock.lock_exclusive().context("locking manifest")?;
    let res = (|| -> Result<()> {
        let json = serde_json::to_string_pretty(ws)?;
        fs::write(&path, format!("{json}\n"))?;
        Ok(())
    })();
    let _ = lock.unlock();
    res.context("writing workspace manifest")
}

pub fn load(story: &str) -> Result<Workspace> {
    load_path(&manifest_path_for(story)?)
}

pub fn load_path(path: &Path) -> Result<Workspace> {
    let text =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
}

/// Infer the workspace story from the current working directory, if it is
/// (or is inside) `~/.agentws/<story>`.
pub fn infer_story_from_cwd() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let base = config::workspaces_root()?;
    if let Ok(rel) = cwd.strip_prefix(&base) {
        if let Some(first) = rel.components().next() {
            return Ok(Some(first.as_os_str().to_string_lossy().to_string()));
        }
    }
    Ok(None)
}

/// Resolve a story name: use the given one, else infer from cwd.
pub fn resolve_story(given: Option<String>) -> Result<String> {
    match given {
        Some(s) if !s.is_empty() => Ok(s),
        _ => infer_story_from_cwd()?.ok_or_else(|| {
            anyhow::anyhow!(
                "no story name given, and not currently inside a workspace directory"
            )
        }),
    }
}

pub fn list_stories() -> Result<Vec<String>> {
    let root = config::workspaces_root()?;
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("workspace.json").exists() {
                if let Some(name) = p.file_name() {
                    out.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}
