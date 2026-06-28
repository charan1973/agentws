use crate::{manifest, ops};
use anyhow::Result;

pub fn run(story: Option<String>, repo: String, base: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;
    let entry = ops::add_repo_to_workspace(&mut ws, &repo, base.as_deref())?;
    manifest::save(&ws)?;
    println!(
        "added '{}' to '{}'  ->  {}",
        entry.name,
        story,
        entry.worktree.display()
    );
    Ok(())
}
