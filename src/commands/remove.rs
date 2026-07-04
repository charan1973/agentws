use crate::{manifest, ops};
use anyhow::Result;

pub fn run(story: Option<String>, repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;
    ops::remove_repo_from_workspace(&mut ws, &repo)?;
    manifest::save(&ws)?;
    crate::vscode::write_workspace(&ws)?;
    println!("removed '{repo}' from '{story}'");
    Ok(())
}
