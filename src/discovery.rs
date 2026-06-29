use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub name: String,
    pub path: PathBuf,
}

const MAX_DEPTH: usize = 4;

/// Skip these directory names while scanning.
const SKIP_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".next",
    ".git",
    "coverage",
    ".cache",
];

/// Walk `roots` (to a limited depth) and collect git repositories.
pub fn discover(roots: &[PathBuf]) -> Vec<Repo> {
    let mut repos = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let canonical = match root.canonicalize() {
            Ok(c) => c,
            Err(_) => continue,
        };
        walk(&canonical, 0, &mut repos, &mut seen);
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    repos
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<Repo>, seen: &mut HashSet<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }

    // This directory is itself a git repo — record and stop descending.
    if dir.join(".git").exists() {
        if seen.insert(dir.to_path_buf()) {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.display().to_string());
            out.push(Repo {
                name,
                path: dir.to_path_buf(),
            });
        }
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || SKIP_NAMES.contains(&&*name_str) {
            continue;
        }
        walk(&entry.path(), depth + 1, out, seen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(args: &[&str], dir: &Path) {
        let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
        if !out.status.success() {
            panic!(
                "git {:?} in {} failed: {}",
                args,
                dir.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(&["init", "-q"], path);
        git(&["symbolic-ref", "HEAD", "refs/heads/main"], path);
        git(&["config", "user.email", "t@t.t"], path);
        git(&["config", "user.name", "t"], path);
        std::fs::write(path.join("f"), "x").unwrap();
        git(&["add", "-A"], path);
        git(&["commit", "-qm", "i"], path);
    }

    #[test]
    fn discovers_immediate_git_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        init_repo(&root.join("api"));
        init_repo(&root.join("web"));
        // a plain directory, not a repo -> ignored
        std::fs::create_dir_all(root.join("notes")).unwrap();

        let roots = [root];
        let repos = discover(&roots);
        let names: Vec<_> = repos.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["api".to_string(), "web".to_string()]);
    }

    #[test]
    fn discovers_nested_repos() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        init_repo(&root.join("group").join("svc"));

        let repos = discover(&[root]);
        let names: Vec<_> = repos.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["svc".to_string()]);
    }

    #[test]
    fn ignores_nonexistent_root() {
        let repos = discover(&[PathBuf::from("/agentws/definitely/does/not/exist")]);
        assert!(repos.is_empty());
    }

    #[test]
    fn dedupes_overlapping_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        init_repo(&root.join("api"));
        let canon = root.canonicalize().unwrap();
        // same dir via two paths -> counted once
        let repos = discover(&[root, canon]);
        assert_eq!(repos.len(), 1);
    }
}
