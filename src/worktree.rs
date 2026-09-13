use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Best-effort default branch for a repo:
/// 1. origin/HEAD symbolic ref, 2. `main`, 3. `master`, 4. current branch.
pub fn default_branch(repo: &Path) -> Result<String> {
    if let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])
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
    if !dest.exists() {
        return false;
    }
    match Command::new("git")
        .arg("-C")
        .arg(dest)
        .args(["status", "--porcelain"])
        .output()
    {
        Ok(output) if output.status.success() => !output.stdout.is_empty(),
        // An existing path that Git cannot inspect is not safe to classify as
        // clean. Bulk cleanup preserves it unless the human passes --force.
        _ => true,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(args: &[&str], dir: &Path) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn make_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(&["init", "-q"], path);
        git(&["symbolic-ref", "HEAD", "refs/heads/main"], path);
        git(&["config", "user.email", "t@t.t"], path);
        git(&["config", "user.name", "t"], path);
        std::fs::write(path.join("f.txt"), "hi").unwrap();
        git(&["add", "-A"], path);
        git(&["commit", "-qm", "init"], path);
    }

    #[test]
    fn default_branch_detects_main() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("origin");
        make_repo(&repo);
        assert_eq!(default_branch(&repo).unwrap(), "main");
    }

    #[test]
    fn add_list_dirty_remove_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        let dest = tmp.path().join("wt");
        make_repo(&origin);

        add_worktree(&origin, &dest, "feat/story", "main").unwrap();
        assert!(dest.is_dir(), "worktree dir should exist");
        assert!(
            dest.join("f.txt").exists(),
            "committed file should be present"
        );
        // git reports worktree paths canonicalized (macOS: /tmp -> /private/tmp),
        // so compare canonical forms rather than raw paths.
        let canon_dest = dest.canonicalize().unwrap();
        let listed = list_worktrees(&origin).unwrap();
        assert!(
            listed
                .iter()
                .any(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == canon_dest),
            "dest ({}) should be among worktrees: {:?}",
            dest.display(),
            listed
        );

        // clean worktree is not dirty
        assert!(!is_dirty(&dest), "fresh worktree should not be dirty");
        // an untracked change makes it dirty
        std::fs::write(dest.join("untracked.txt"), "x").unwrap();
        assert!(
            is_dirty(&dest),
            "worktree with untracked file should be dirty"
        );

        remove_worktree(&origin, &dest).unwrap();
        assert!(!dest.exists(), "dest should be gone after remove");
    }

    #[test]
    fn add_worktree_rejects_bad_base() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin");
        let dest = tmp.path().join("wt");
        make_repo(&origin);
        let res = add_worktree(&origin, &dest, "feat/x", "does-not-exist");
        assert!(res.is_err(), "should fail when base branch is absent");
    }

    #[test]
    fn dirty_check_is_conservative_for_existing_non_git_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let plain = tmp.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        assert!(is_dirty(&plain));
        assert!(!is_dirty(&tmp.path().join("missing")));
    }
}
