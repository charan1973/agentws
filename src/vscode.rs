//! VS Code multi-root workspace file generation.
//!
//! Each agentws workspace gets a `<story>.code-workspace` at its root that lists
//! every repo worktree as a top-level folder. Opened with `code`, this gives a
//! flat explorer (one root per repo, no extra nesting level) and a separate
//! Source Control entry per worktree — so each worktree can be diffed against
//! `main` on its own, in the GUI.
//!
//! The file is **derived state**: the manifest (`workspace.db`) is the source
//! of truth, and this file is regenerated whenever the set of repos changes
//! (`new` / `add` / `remove` / `restore`) and deleted on `archive`. `agentws code`
//! regenerates it on demand if missing, so a bare `agentws code` always opens a
//! correct view even after the file was removed.

use crate::manifest;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Path of the `.code-workspace` file for a workspace.
pub fn workspace_file(ws: &manifest::Workspace) -> PathBuf {
    ws.root.join(format!("{}.code-workspace", ws.story))
}

/// (Re)write the `.code-workspace` from the current manifest entries.
///
/// Folder paths are relative to the workspace root (where the file lives), so
/// the whole `~/.agentws/<story>/` tree stays relocatable. JSON is built with
/// `serde_json` so unusual repo names (quotes, backslashes) can't corrupt it.
pub fn write_workspace(ws: &manifest::Workspace) -> Result<()> {
    let folders: Vec<Value> = ws.repos.iter().map(|r| json!({ "path": r.name })).collect();
    let doc = json!({
        "folders": folders,
        "settings": {}
    });
    let body = format!("{}\n", serde_json::to_string_pretty(&doc)?);
    std::fs::write(workspace_file(ws), body)?;
    Ok(())
}

/// Remove the `.code-workspace`. Used on archive, when the worktrees are gone.
/// Idempotent: a missing file is not an error.
pub fn remove_workspace(ws: &manifest::Workspace) -> Result<()> {
    let f = workspace_file(ws);
    if f.exists() {
        std::fs::remove_file(&f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{RepoEntry, Workspace};
    use chrono::Utc;
    use std::path::{Path, PathBuf};

    fn ws_with_root(root: &Path, names: &[&str]) -> Workspace {
        Workspace {
            story: "demo".into(),
            root: root.to_path_buf(),
            created: Utc::now(),
            repos: names
                .iter()
                .map(|n| RepoEntry {
                    name: (*n).into(),
                    origin: PathBuf::from("/orig").join(n),
                    worktree: root.join(n),
                    branch: "feat/demo".into(),
                    base: "main".into(),
                })
                .collect(),
            requests: vec![],
            archived: false,
            skills: vec![],
            agents_md: vec![],
            setup: Default::default(),
        }
    }

    #[test]
    fn workspace_file_is_named_after_story() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws_with_root(dir.path(), &[]);
        assert_eq!(workspace_file(&ws), dir.path().join("demo.code-workspace"));
    }

    #[test]
    fn write_workspace_lists_each_repo_as_relative_folder() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws_with_root(dir.path(), &["svc-auth", "svc-api"]);
        write_workspace(&ws).unwrap();
        let content = std::fs::read_to_string(workspace_file(&ws)).unwrap();

        // valid JSON, round-trips through serde
        let v: Value = serde_json::from_str(&content).unwrap();
        let paths: Vec<String> = v["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(paths, vec!["svc-auth".to_string(), "svc-api".to_string()]);

        // paths are relative, not the absolute origin paths
        assert!(!content.contains("/orig"), "should be relative: {content}");
        assert!(v["settings"].as_object().unwrap().is_empty());
    }

    #[test]
    fn write_workspace_handles_quirky_repo_names() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws_with_root(dir.path(), &["a\"b"]); // would break hand-rolled JSON
        write_workspace(&ws).unwrap();
        let content = std::fs::read_to_string(workspace_file(&ws)).unwrap();
        serde_json::from_str::<Value>(&content).unwrap(); // still valid
    }

    #[test]
    fn remove_workspace_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let ws = ws_with_root(dir.path(), &["a"]);
        remove_workspace(&ws).unwrap(); // missing — ok
        write_workspace(&ws).unwrap();
        assert!(workspace_file(&ws).exists());
        remove_workspace(&ws).unwrap(); // present — removed
        assert!(!workspace_file(&ws).exists());
    }
}
