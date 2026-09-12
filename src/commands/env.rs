use crate::{manifest, ops};
use anyhow::Result;

/// Re-read exposed dotenv values and update every configured consumer.
pub fn rewire(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let ws = manifest::load(&story)?;
    let changed = ops::wire_env(&ws)?;
    println!("rewired '{story}': {changed} env file(s) updated.");
    Ok(())
}
