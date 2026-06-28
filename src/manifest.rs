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
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub requests: Vec<RepoRequest>,
    /// Set true after `archive` removes the worktrees.
    #[serde(default)]
    pub archived: bool,
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

/// Resolve which workspace a command targets, in priority order:
///   1. an explicit `--story` argument,
///   2. the cwd is inside a workspace dir (`~/.agentws/<story>`),
///   3. the active-workspace pointer (`~/.agentws/.current`),
///   4. there is exactly one workspace — use it,
///   5. otherwise: error with a helpful hint.
pub fn resolve_story(given: Option<String>) -> Result<String> {
    // 1. explicit
    if let Some(s) = given.filter(|s| !s.is_empty()) {
        return Ok(s);
    }
    // 2. activated shell ($AGENTWS_WORKSPACE env var) — per-shell, authoritative
    if let Ok(s) = std::env::var("AGENTWS_WORKSPACE") {
        if !s.is_empty() && exists(&s) {
            return Ok(s);
        }
    }
    // 3. cwd inside a workspace
    if let Some(s) = infer_story_from_cwd()? {
        return Ok(s);
    }
    // 4. active pointer (global fallback)
    if let Some(s) = get_current()? {
        if exists(&s) {
            return Ok(s);
        }
    }
    // 5. exactly one workspace
    let stories = list_stories().unwrap_or_default();
    if stories.len() == 1 {
        return Ok(stories[0].clone());
    }
    // 6. error
    let hint = if stories.is_empty() {
        "No workspaces exist yet. Create one with `agentws new <story>`.".to_string()
    } else {
        format!(
            "Specify one with `agentws <cmd> --story <name>`, or set an active one with
  `agentws use <name>`.
Available workspaces: {}",
            stories.join(", ")
        )
    };
    anyhow::bail!("could not determine which workspace to use.\n{hint}")
}

/// Path to the active-workspace pointer file: `~/.agentws/.current`.
fn current_pointer_path() -> Result<PathBuf> {
    Ok(config::workspaces_root()?.join(".current"))
}

/// Set the active workspace (used as a fallback by `resolve_story`).
pub fn set_current(story: &str) -> Result<()> {
    let path = current_pointer_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, story)?;
    Ok(())
}

/// Read the active workspace, if any. Stale pointers (pointing at a deleted
/// workspace) are treated as absent.
pub fn get_current() -> Result<Option<String>> {
    let path = current_pointer_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&path)?.trim().to_string();
    if s.is_empty() || !exists(&s) {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// Clear the active-workspace pointer if (and only if) it points at `story`.
/// Reads the raw file content so deletion still works once the manifest is gone.
pub fn clear_current_if(story: &str) -> Result<()> {
    let path = current_pointer_path()?;
    if !path.exists() {
        return Ok(());
    }
    let s = fs::read_to_string(&path)?.trim().to_string();
    if s == story {
        fs::remove_file(path).ok();
    }
    Ok(())
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
