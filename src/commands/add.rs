use crate::{manifest, ops};
use anyhow::Result;

pub fn run(story: Option<String>, repo: String, base: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let (entry, ws) = manifest::mutate(&story, |ws| {
        let entry = ops::add_repo_to_workspace(ws, &repo, base.as_deref())?;
        Ok((entry, ws.clone()))
    })?;
    crate::vscode::write_workspace(&ws)?;
    let wired = ops::wire_env(&ws)?;
    if wired > 0 {
        println!("rewired {wired} cross-repo env file(s)");
    }
    println!(
        "added '{}' to '{}'  ->  {}",
        entry.name,
        story,
        entry.worktree.display()
    );
    Ok(())
}
