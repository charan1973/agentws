use crate::manifest;
use anyhow::{bail, Result};

pub fn run(story: &str) -> Result<()> {
    if !manifest::exists(story) {
        bail!("no workspace named '{story}'");
    }
    let ws = manifest::load(story)?;
    println!("{}", ws.root.display());
    Ok(())
}
