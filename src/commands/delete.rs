use crate::{manifest, worktree};
use anyhow::{bail, Result};
use std::io::{self, Write};

pub fn run(story: &str, yes: bool) -> Result<()> {
    let ws = match manifest::load(story) {
        Ok(w) => w,
        Err(_) => bail!("no workspace named '{story}'"),
    };

    if !yes {
        println!(
            "about to delete workspace '{story}' at {root}\n\
             (removes {n} worktree(s); branches are kept).",
            root = ws.root.display(),
            n = ws.repos.len()
        );
        print!("proceed? [y/N] ");
        io::stdout().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("aborted.");
            return Ok(());
        }
    }

    for r in &ws.repos {
        match worktree::remove_worktree(&r.origin, &r.worktree) {
            Ok(_) => println!("  - removed worktree {}", r.name),
            Err(e) => eprintln!("  ! could not remove worktree {}: {}", r.name, e),
        }
    }

    if ws.root.exists() {
        std::fs::remove_dir_all(&ws.root).ok();
    }
    println!("deleted workspace '{story}'.");
    Ok(())
}
