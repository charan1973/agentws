use crate::{config, library, manifest, templates};
use anyhow::Result;

pub fn list() -> Result<()> {
    let cfg = config::load()?;
    let items = library::discover_kind(&cfg, library::Kind::Template)?;
    if items.is_empty() {
        println!("no templates. Save one with `agentws template save <name> --from <story>`");
        return Ok(());
    }
    println!("{:<24} PATH", "TEMPLATE");
    for item in items {
        println!("{:<24} {}", item.name, item.path.display());
    }
    Ok(())
}

pub fn show(name: String) -> Result<()> {
    let cfg = config::load()?;
    let (_, item) = templates::load(&cfg, &name)?;
    print!("{}", std::fs::read_to_string(item.path)?);
    Ok(())
}

pub fn save(name: String, from: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(from)?;
    let ws = manifest::load(&story)?;
    let cfg = config::load()?;
    let template = templates::from_workspace(&ws, &cfg);
    let path = templates::save(&name, &template)?;
    println!(
        "saved template '{name}' from '{story}' -> {}",
        path.display()
    );
    if template.base.is_none() && !ws.repos.is_empty() {
        eprintln!(
            "note: repositories use different base branches; the template leaves `base` unset"
        );
    }
    Ok(())
}

pub fn delete(name: String) -> Result<()> {
    templates::remove(&name)?;
    println!("deleted template '{name}'");
    Ok(())
}

pub fn edit(name: String) -> Result<()> {
    templates::edit(&config::load()?, &name)?;
    println!("updated template '{name}'");
    Ok(())
}
