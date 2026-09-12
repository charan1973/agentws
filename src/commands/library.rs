use crate::{config, library};
use anyhow::Result;
use std::path::PathBuf;

pub fn list() -> Result<()> {
    let cfg = config::load()?;
    let items = library::discover(&cfg)?;
    if items.is_empty() {
        println!(
            "library is empty. Add an item with `agentws library add <path>`\nroot: {}",
            config::library_dir()?.display()
        );
        return Ok(());
    }
    println!("{:<12} {:<24} PATH", "KIND", "NAME");
    for item in items {
        println!(
            "{:<12} {:<24} {}",
            item.kind.label(),
            item.name,
            item.path.display()
        );
    }
    Ok(())
}

pub fn add(path: PathBuf) -> Result<()> {
    let item = library::add(&path)?;
    println!(
        "added {} '{}' -> {}",
        item.kind.label(),
        item.name,
        item.path.display()
    );
    super::refresh::refresh_all_best_effort();
    Ok(())
}

pub fn remove(name: String, kind: Option<String>) -> Result<()> {
    let kind = kind.as_deref().map(library::Kind::parse).transpose()?;
    let removed = library::remove(&name, kind)?;
    for kind in removed {
        println!("removed {} '{name}'", kind.label());
    }
    super::refresh::refresh_all_best_effort();
    Ok(())
}
