use crate::{manifest, picker, worktree};
use anyhow::{bail, Result};
use std::io::{self, Write};

/// Delete one or more workspaces.
///
/// Target resolution:
/// - `stories` non-empty → those names (missing ones are reported + skipped).
/// - `stories` empty → open the fuzzy multi-select picker over all workspaces.
///
/// Safety:
/// - A workspace with any uncommitted ("dirty") worktree is **kept by default**
///   in bulk mode; pass `force` to remove it anyway. A single explicitly-named
///   workspace keeps the legacy force-on-delete behavior (so existing
///   `agentws delete foo` usage is unchanged).
/// - `dry_run` prints the plan and deletes nothing.
/// - `yes` skips the aggregate `[y/N]` confirmation.
pub fn run(stories: Vec<String>, dry_run: bool, force: bool, yes: bool) -> Result<()> {
    // 1. Resolve targets: explicit names, or the fuzzy picker when none given.
    let mut targets = stories;
    if targets.is_empty() {
        let all = manifest::list_stories()?;
        if all.is_empty() {
            println!("no workspaces exist.");
            return Ok(());
        }
        match picker::pick_strings(all)? {
            Some(chosen) if !chosen.is_empty() => targets = chosen,
            _ => {
                println!("nothing selected; aborting.");
                return Ok(());
            }
        }
    }

    // A single explicitly-named target keeps legacy behavior (force, including
    // dirty trees). Multiple targets / picker get the safer "keep dirty unless
    // --force" default.
    let single = targets.len() == 1;
    let effective_force = force || single;

    // 2. Load + validate.
    let mut loaded: Vec<(String, manifest::Workspace, bool)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for name in &targets {
        match manifest::load(name) {
            Ok(ws) => {
                let dirty = ws.repos.iter().any(|r| worktree::is_dirty(&r.worktree));
                loaded.push((name.clone(), ws, dirty));
            }
            Err(_) => missing.push(name.clone()),
        }
    }
    if single && loaded.is_empty() {
        bail!("no workspace named '{}'", targets[0]);
    }
    for m in &missing {
        eprintln!("! no workspace named '{m}' (skipped)");
    }
    if loaded.is_empty() {
        bail!("no valid workspaces to delete");
    }

    // 3. Partition: dirty → kept unless effective_force.
    let mut to_delete: Vec<(String, manifest::Workspace, bool)> = Vec::new();
    let mut kept_dirty: Vec<String> = Vec::new();
    for (name, ws, dirty) in loaded {
        if dirty && !effective_force {
            kept_dirty.push(name.clone());
            eprintln!(
                "! '{name}' has uncommitted work — keeping it (pass --force to delete anyway)"
            );
        } else {
            to_delete.push((name, ws, dirty));
        }
    }

    // 4. Print the plan.
    println!();
    if to_delete.is_empty() {
        if kept_dirty.is_empty() {
            println!("nothing to delete.");
        } else {
            println!(
                "nothing to delete — {} dirty workspace(s) kept (use --force to remove): {}",
                kept_dirty.len(),
                kept_dirty.join(", ")
            );
        }
        return Ok(());
    }
    println!("will delete {} workspace(s) — branches are kept:", to_delete.len());
    for (name, ws, dirty) in &to_delete {
        let tag = if *dirty { "  [dirty — forced]" } else { "" };
        println!("  - {name}{tag}  ({} worktree(s))", ws.repos.len());
    }
    if !kept_dirty.is_empty() {
        println!(
            "keeping {} dirty workspace(s): {}",
            kept_dirty.len(),
            kept_dirty.join(", ")
        );
    }

    // 5. Dry run stops here.
    if dry_run {
        println!("\n(dry run — nothing deleted)");
        return Ok(());
    }

    // 6. Confirm (unless --yes).
    if !yes {
        print!("proceed? [y/N] ");
        io::stdout().flush().ok();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            println!("aborted.");
            return Ok(());
        }
    }

    // 7. Delete.
    for (name, ws, _) in &to_delete {
        delete_one(ws);
        println!("deleted workspace '{name}'.");
    }
    Ok(())
}

/// Remove every worktree, the workspace dir, and clear the current pointer.
fn delete_one(ws: &manifest::Workspace) {
    for r in &ws.repos {
        match worktree::remove_worktree(&r.origin, &r.worktree) {
            Ok(_) => println!("  - {}: removed worktree {}", ws.story, r.name),
            Err(e) => eprintln!(
                "  ! {}: could not remove worktree {}: {}",
                ws.story, r.name, e
            ),
        }
    }
    if ws.root.exists() {
        std::fs::remove_dir_all(&ws.root).ok();
    }
    if let Err(e) = manifest::clear_current_if(&ws.story) {
        eprintln!("  ! {}: could not clear current pointer: {e}", ws.story);
    }
}
