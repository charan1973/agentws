use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Best-effort default branch for a repo:
/// 1. origin/HEAD symbolic ref, 2. `main`, 3. `master`, 4. current branch.
pub fn default_branch(repo: &Path) -> Result<String> {
    if let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(branch) = s.strip_prefix("origin/") {
                return Ok(branch.to_string());
            }
        }
    }

    for candidate in ["main", "master"] {
        if branch_exists(repo, candidate)? {
            return Ok(candidate.to_string());
        }
    }

    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("detecting current branch")?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s == "HEAD" {
        bail!("could not determine base branch for {}", repo.display());
    }
    Ok(s)
}

fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .output()?;
    Ok(out.status.success())
}

/// Create a worktree of `origin` at `dest` on a new branch `branch` off `base`.
/// Idempotent: if `dest` already exists, it is reused.
pub fn add_worktree(origin: &Path, dest: &Path, branch: &str, base: &str) -> Result<()> {
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let out = Command::new("git")
        .arg("-C")
        .arg(origin)
        .args(["worktree", "add", "-b", branch])
        .arg(dest)
        .arg(base)
        .output()
        .with_context(|| format!("creating worktree at {}", dest.display()))?;

    if out.status.success() {
        return Ok(());
    }

    let err = String::from_utf8_lossy(&out.stderr);
    // Branch already exists from a previous run: check it out instead of -b.
    if err.contains("already exists") {
        let out2 = Command::new("git")
            .arg("-C")
            .arg(origin)
            .args(["worktree", "add"])
            .arg(dest)
            .arg(branch)
            .output()?;
        if out2.status.success() {
            return Ok(());
        }
        bail!(
            "git worktree add failed:\n{}",
            String::from_utf8_lossy(&out2.stderr)
        );
    }
    bail!("git worktree add failed:\n{err}");
}

/// Remove a worktree. Best-effort: ignores "not a working tree" errors.
pub fn remove_worktree(origin: &Path, dest: &Path) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(origin)
        .args(["worktree", "remove", "--force"])
        .arg(dest)
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    // Already gone / not registered — treat as success.
    if err.contains("not a working") || err.contains("is not a working") {
        return Ok(());
    }
    bail!("git worktree remove failed:\n{err}");
}

/// Whether a worktree path has uncommitted changes.
pub fn is_dirty(dest: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dest)
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

#[allow(dead_code)]
pub fn list_worktrees(origin: &Path) -> Result<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(origin)
        .args(["worktree", "list", "--porcelain"])
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut paths = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            paths.push(PathBuf::from(p));
        }
    }
    Ok(paths)
}
