use crate::{composition, manifest};
use anyhow::Result;

pub fn run(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let report = refresh_story(&story)?;
    println!(
        "refreshed '{story}': {} library skill(s), {} repo skill(s), {} snippet(s)",
        report.library_skills, report.repo_skills, report.snippets
    );
    Ok(())
}

pub fn refresh_story(story: &str) -> Result<composition::RefreshReport> {
    manifest::mutate(story, composition::refresh)
}

/// Refresh every existing workspace after a central-library change. One stale
/// workspace must not prevent other workspaces from being updated, so failures
/// are reported as warnings and processing continues.
pub fn refresh_all_best_effort() {
    for story in manifest::list_stories().unwrap_or_default() {
        if let Err(error) = refresh_story(&story) {
            eprintln!("warning: could not refresh workspace '{story}': {error:#}");
        }
    }
}
