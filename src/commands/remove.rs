use crate::{manifest, ops};
use anyhow::Result;

pub fn run(story: Option<String>, repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let ws = manifest::mutate(&story, |ws| {
        ops::remove_repo_from_workspace(ws, &repo)?;
        Ok(ws.clone())
    })?;
    crate::vscode::write_workspace(&ws)?;
    let wired = ops::wire_env(&ws)?;
    if wired > 0 {
        println!("rewired {wired} cross-repo env file(s)");
    }
    println!("removed '{repo}' from '{story}'");
    Ok(())
}
