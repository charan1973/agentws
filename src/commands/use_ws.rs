use crate::manifest;
use anyhow::{bail, Result};

/// Set the active workspace (the default target for commands run from outside
/// a workspace directory).
pub fn run(story: &str) -> Result<()> {
    if !manifest::exists(story) {
        bail!("no workspace named '{story}'");
    }
    manifest::set_current(story)?;
    println!("active workspace is now '{story}'.");
    Ok(())
}
