use crate::manifest;
use anyhow::Result;

pub fn run() -> Result<()> {
    let stories = manifest::list_stories()?;
    if stories.is_empty() {
        println!("no workspaces yet. create one with `agentws new <story>`.");
        return Ok(());
    }
    println!("{:<24} {:>10}  {}", "WORKSPACE", "REPOS", "ROOT");
    for s in stories {
        let ws = manifest::load(&s).ok();
        let (count, root) = ws
            .as_ref()
            .map(|w| (w.repos.len(), w.root.display().to_string()))
            .unwrap_or((0, String::new()));
        println!("{:<24} {:>10}  {}", s, count, root);
    }
    Ok(())
}
