use crate::{config, manifest, worktree};
use anyhow::{bail, Result};

pub fn run(story: Option<String>) -> Result<()> {
    let story = match story {
        Some(s) => s,
        None => match infer_story_from_cwd()? {
            Some(s) => s,
            None => bail!("pass a story name: `agentws status <story>`"),
        },
    };

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

fn infer_story_from_cwd() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let base = config::workspaces_root()?;
    if let Ok(rel) = cwd.strip_prefix(&base) {
        if let Some(first) = rel.components().next() {
            return Ok(Some(first.as_os_str().to_string_lossy().to_string()));
        }
    }
    Ok(None)
}
