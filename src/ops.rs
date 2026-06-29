//! Shared workspace operations used by multiple commands
//! (adding/removing repos, symlink + hook application, request ids).

use crate::{config, discovery, manifest, worktree};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Short, unique-enough id for a repo request.
pub fn new_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
    format!("{:04x}", (t ^ n.rotate_left(8)) & 0xffff)
}

/// Resolve a repo name to its origin path by scanning configured roots.
pub fn find_repo(name: &str) -> Result<discovery::Repo> {
    let cfg = config::load()?;
    let roots = cfg.repo_roots_expanded();
    let all = discovery::discover(&roots);
    all.into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow!("no discovered repo named '{name}'"))
}

/// Create a worktree for `name` under the workspace root, apply symlinks/hooks,
/// and push a RepoEntry onto the workspace. Caller persists the manifest.
pub fn add_repo_to_workspace(
    ws: &mut manifest::Workspace,
    name: &str,
    base_override: Option<&str>,
) -> Result<manifest::RepoEntry> {
    if ws.repos.iter().any(|r| r.name == name) {
        bail!(
            "repo '{name}' is already in workspace '{}'",
            ws.story
        );
    }
    let repo = find_repo(name)?;
    let dest = ws.root.join(name);
    let branch = format!("feat/{}", ws.story);
    let base = match base_override {
        Some(b) => b.to_string(),
        None => worktree::default_branch(&repo.path).unwrap_or_else(|_| "main".to_string()),
    };
    worktree::add_worktree(&repo.path, &dest, &branch, &base)?;
    apply_symlinks(&repo.path, &dest);
    run_post_create(&dest);
    let entry = manifest::RepoEntry {
        name: name.to_string(),
        origin: repo.path.clone(),
        worktree: dest,
        branch,
        base,
    };
    ws.repos.push(entry.clone());
    Ok(entry)
}

/// Remove a repo's worktree and drop its entry. Caller persists the manifest.
pub fn remove_repo_from_workspace(
    ws: &mut manifest::Workspace,
    name: &str,
) -> Result<()> {
    let idx = ws
        .repos
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow!("repo '{name}' is not in workspace '{}'", ws.story))?;
    let entry = ws.repos.remove(idx);
    worktree::remove_worktree(&entry.origin, &entry.worktree)?;
    Ok(())
}

/// Symlink configured paths (node_modules, .env, …) from origin into the worktree.
pub fn apply_symlinks(origin: &Path, dest: &Path) {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    for pat in &cfg.symlinks {
        // Treat each configured entry as a literal name/glob in the origin root.
        for entry in match glob_in(origin, pat) {
            Ok(v) => v,
            Err(_) => continue,
        } {
            let name = match entry.file_name() {
                Some(n) => n,
                None => continue,
            };
            let link = dest.join(name);
            if link.exists() || link.is_symlink() {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&entry, &link).ok();
            #[cfg(not(unix))]
            std::fs::copy(&entry, &link).ok();
        }
    }
}

/// Run the configured `post_create` hook inside the worktree after creation.
pub fn run_post_create(dest: &Path) {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    let Some(cmd) = cfg.post_create.as_deref() else {
        return;
    };
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dest)
        .status();
}

/// Simple non-recursive glob over immediate children of `dir` matching `pat`.
fn glob_in(dir: &Path, pat: &str) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let prefix = pat.split_once('*').map(|(p, _)| p).unwrap_or(pat);
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if pat == name.as_ref()
            || (pat.contains('*') && name.starts_with(prefix))
        {
            out.push(entry.path());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_four_hex_chars() {
        let id = new_id();
        assert_eq!(id.len(), 4, "id was {id}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "id was {id}");
    }

    #[test]
    fn new_ids_differ_across_calls() {
        // 4-hex space (65536); two consecutive calls colliding is ~1/65536 — fine for CI
        let mut ids = Vec::new();
        for _ in 0..8 {
            ids.push(new_id());
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert!(unique.len() > 1, "ids should vary across calls");
    }
}
