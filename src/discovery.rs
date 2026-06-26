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
