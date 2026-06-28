use crate::manifest;
use crate::worktree;
use anyhow::Result;

pub fn run(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;

    let ws = manifest::load(&story)?;
    println!("workspace : {}", ws.story);
    println!("root      : {}", ws.root.display());
    println!("created   : {}", ws.created.format("%Y-%m-%d %H:%M"));
    println!("repos:");
    for r in &ws.repos {
        let mark = if worktree::is_dirty(&r.worktree) {
            "*"
        } else {
            " "
        };
        println!(
            "  {mark} {name:<22} {branch:<30} -> {dest}",
            name = r.name,
            branch = r.branch,
            dest = r.worktree.display()
        );
    }
    if !ws.requests.is_empty() {
        println!("requests:");
        for rq in &ws.requests {
            println!(
                "  [{status:>8}] {repo:<22} ({by}) {reason}",
                status = rq.status,
                repo = rq.repo,
                by = rq.by,
                reason = rq.reason.as_deref().unwrap_or("")
            );
        }
    }
    Ok(())
}
