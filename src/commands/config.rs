//! `agentws config` — show the resolved configuration and key paths.

use crate::config;
use anyhow::Result;
use std::path::PathBuf;

pub fn run() -> Result<()> {
    let cfg = config::load()?;
    println!("agentws configuration\n");
    println!("  config file    : {}", config::config_path()?.display());
    println!(
        "  workspaces dir : {}",
        config::workspaces_root()?.display()
    );
    println!("  library dir    : {}", config::library_dir()?.display());
    println!("  cache dir      : {}", config::cache_dir()?.display());
    println!();
    println!("  repo_roots     : {}", fmt_paths(&cfg.repo_roots));
    println!("  library_dirs   : {}", fmt_paths(&cfg.library_dirs));
    println!(
        "  default_base   : {}",
        cfg.default_base
            .as_deref()
            .unwrap_or("(each repo's default)")
    );
    println!("  symlinks       : {}", fmt_strs(&cfg.symlinks));
    println!(
        "  default tmpl   : {}",
        cfg.effective_default_template().unwrap_or("(none)")
    );
    println!(
        "  post_create    : {}",
        cfg.post_create.as_deref().unwrap_or("(none)")
    );

    if cfg.repo_roots_expanded().is_empty() {
        println!();
        println!("  hint: no repo_roots configured. Edit the config file above, e.g.");
        println!("        repo_roots = [\"~/work\"]");
    }
    Ok(())
}

fn fmt_paths(v: &[PathBuf]) -> String {
    if v.is_empty() {
        return "(none)".into();
    }
    v.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_strs(v: &[String]) -> String {
    if v.is_empty() {
        return "(none)".into();
    }
    v.join(", ")
}
