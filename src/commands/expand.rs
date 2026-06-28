use crate::{manifest, ops};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;

/// Record a pending repo request (agent-facing tool, but usable from CLI too).
pub fn request(story: Option<String>, repo: String, reason: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;
    if ws.repos.iter().any(|r| r.name == repo) {
        bail!("'{repo}' is already in workspace '{story}'");
    }
    ops::find_repo(&repo)?; // validate it exists
    let id = ops::new_id();
    ws.requests.push(manifest::RepoRequest {
        id: id.clone(),
        repo,
        reason,
        status: "pending".into(),
        by: "human".into(),
        created: Utc::now(),
        resolved: None,
    });
    manifest::save(&ws)?;
    println!("request {id} queued. Resolve with: agentws approve {id}");
    Ok(())
}

/// Approve a pending request (by id or repo name): create the worktree, mark approved.
pub fn approve(story: Option<String>, id_or_repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;

    let idx = ws
        .requests
        .iter()
        .position(|r| r.status == "pending" && r.id == id_or_repo)
        .or_else(|| {
            ws.requests
                .iter()
                .position(|r| r.status == "pending" && r.repo == id_or_repo)
        })
        .ok_or_else(|| anyhow!("no pending request matching '{id_or_repo}'"))?;

    let repo_name = ws.requests[idx].repo.clone();
    if !ws.repos.iter().any(|r| r.name == repo_name) {
        ops::add_repo_to_workspace(&mut ws, &repo_name, None)?;
    }
    ws.requests[idx].status = "approved".into();
    ws.requests[idx].resolved = Some(Utc::now());
    manifest::save(&ws)?;

    let path = ws
        .repos
        .iter()
        .find(|r| r.name == repo_name)
        .map(|r| r.worktree.display().to_string())
        .unwrap_or_default();
    println!("approved '{repo_name}'  ->  {path}");
    Ok(())
}

/// Deny a pending request (by id or repo name).
pub fn deny(story: Option<String>, id_or_repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;
    let idx = ws
        .requests
        .iter()
        .position(|r| r.status == "pending" && (r.id == id_or_repo || r.repo == id_or_repo))
        .ok_or_else(|| anyhow!("no pending request matching '{id_or_repo}'"))?;
    let repo_name = ws.requests[idx].repo.clone();
    ws.requests[idx].status = "denied".into();
    ws.requests[idx].resolved = Some(Utc::now());
    manifest::save(&ws)?;
    println!("denied '{repo_name}'");
    Ok(())
}

/// List pending requests.
pub fn pending(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let ws = manifest::load(&story)?;
    let pend: Vec<_> = ws
        .requests
        .iter()
        .filter(|r| r.status == "pending")
        .collect();
    if pend.is_empty() {
        println!("no pending requests for '{story}'.");
        return Ok(());
    }
    println!("{:<8} {:<22} {}", "ID", "REPO", "REASON");
    for r in pend {
        println!(
            "{:<8} {:<22} {}",
            r.id,
            r.repo,
            r.reason.as_deref().unwrap_or("")
        );
    }
    Ok(())
}
