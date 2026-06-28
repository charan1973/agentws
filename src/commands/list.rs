use crate::manifest;
use anyhow::Result;

pub fn run() -> Result<()> {
    let stories = manifest::list_stories()?;
    if stories.is_empty() {
        println!("no workspaces yet. create one with `agentws new <story>`.");
        return Ok(());
    }
    let active = manifest::get_current().unwrap_or(None);
    println!("{:<2} {:<22} {:>10}  {}", "", "WORKSPACE", "REPOS", "ROOT");
    for s in stories {
        let mark = if active.as_deref() == Some(s.as_str()) { "*" } else { " " };
        let ws = manifest::load(&s).ok();
        let (count, root) = ws
            .as_ref()
            .map(|w| (w.repos.len(), w.root.display().to_string()))
            .unwrap_or((0, String::new()));
        let tag = if ws.as_ref().map(|w| w.archived).unwrap_or(false) {
            " (archived)"
        } else {
            ""
        };
        println!("{:<2} {:<22} {:>10}  {}{}", mark, s, count, root, tag);
    }
    if active.is_some() {
        eprintln!("\n* = active workspace");
    }
    Ok(())
}
