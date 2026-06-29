//! `agentws discover` — scan configured roots and list discovered repositories.

use crate::{config, discovery};
use anyhow::Result;

pub fn run() -> Result<()> {
    let cfg = config::load()?;
    let roots = cfg.repo_roots_expanded();
    if roots.is_empty() {
        let path = config::ensure_example()?;
        anyhow::bail!(
            "no repo_roots configured. Add roots to {} and retry.\nExample:\n  repo_roots = [\"~/work\"]",
            path.display()
        );
    }

    let repos = discovery::discover(&roots);
    println!("discovered {} repo(s) under {} root(s):\n", repos.len(), roots.len());
    if repos.is_empty() {
        for r in &roots {
            println!("  (none under {})", r.display());
        }
        return Ok(());
    }

    let name_w = repos
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!("  {:<width$}  PATH", "NAME", width = name_w);
    for r in &repos {
        println!("  {:<width$}  {}", r.name, r.path.display(), width = name_w);
    }
    Ok(())
}
