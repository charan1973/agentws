use crate::{manifest, worktree};
use anyhow::Result;

/// Remove all worktrees but keep the manifest + branches, so it can be restored.
pub fn archive(story: String) -> Result<()> {
    let ws = manifest::mutate(&story, |ws| {
        for r in &ws.repos {
            match worktree::remove_worktree(&r.origin, &r.worktree) {
                Ok(_) => println!("  - archived worktree {}", r.name),
                Err(e) => eprintln!("  ! could not remove {}: {}", r.name, e),
            }
        }
        ws.archived = true;
        Ok(ws.clone())
    })?;
    crate::vscode::remove_workspace(&ws)?;
    println!("archived '{story}' (manifest kept). Restore with: agentws restore {story}");
    Ok(())
}

/// Recreate worktrees from a (possibly archived) manifest.
pub fn restore(story: String) -> Result<()> {
    manifest::mutate(&story, |ws| {
        for r in &mut ws.repos {
            worktree::add_worktree(&r.origin, &r.worktree, &r.branch, &r.base)?;
        }
        for r in &ws.repos {
            crate::ops::apply_workspace_setup(ws, &r.origin, &r.worktree);
        }
        ws.archived = false;
        Ok(ws.clone())
    })?;
    super::refresh::refresh_story(&story)?;
    let ws = manifest::load(&story)?;
    crate::vscode::write_workspace(&ws)?;
    let wired = crate::ops::wire_env(&ws)?;
    if wired > 0 {
        println!("rewired {wired} cross-repo env file(s)");
    }
    println!("restored '{story}'.");
    Ok(())
}
