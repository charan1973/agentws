use crate::{manifest, ops};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;

/// Record a pending repo request (agent-facing tool, but usable from CLI too).
pub fn request(story: Option<String>, repo: String, reason: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    ops::find_repo(&repo)?; // validate it exists
    let id = ops::new_id();
    manifest::mutate(&story, |ws| {
        if ws.repos.iter().any(|r| r.name == repo) {
            bail!("'{repo}' is already in workspace '{story}'");
        }
        ws.requests.push(manifest::RepoRequest {
            id: id.clone(),
            repo,
            reason,
            status: "pending".into(),
            by: "human".into(),
            created: Utc::now(),
            resolved: None,
        });
        Ok(())
    })?;
    println!("request {id} queued. Resolve with: agentws approve {id}");
    Ok(())
}

/// Approve a pending request (by id or repo name): create the worktree, mark approved.
pub fn approve(story: Option<String>, id_or_repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let (repo_name, path) = manifest::mutate(&story, |ws| {
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
            ops::add_repo_to_workspace(ws, &repo_name, None)?;
        }
        ws.requests[idx].status = "approved".into();
        ws.requests[idx].resolved = Some(Utc::now());
        let path = ws
            .repos
            .iter()
            .find(|r| r.name == repo_name)
            .map(|r| r.worktree.display().to_string())
            .unwrap_or_default();
        Ok((repo_name, path))
    })?;
    super::refresh::refresh_story(&story)?;
    let ws = manifest::load(&story)?;
    crate::vscode::write_workspace(&ws)?;
    let wired = ops::wire_env(&ws)?;
    if wired > 0 {
        println!("rewired {wired} cross-repo env file(s)");
    }
    println!("approved '{repo_name}'  ->  {path}");
    Ok(())
}

/// Deny a pending request (by id or repo name).
pub fn deny(story: Option<String>, id_or_repo: String) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let repo_name = manifest::mutate(&story, |ws| {
        let idx = ws
            .requests
            .iter()
            .position(|r| r.status == "pending" && (r.id == id_or_repo || r.repo == id_or_repo))
            .ok_or_else(|| anyhow!("no pending request matching '{id_or_repo}'"))?;
        let repo_name = ws.requests[idx].repo.clone();
        ws.requests[idx].status = "denied".into();
        ws.requests[idx].resolved = Some(Utc::now());
        Ok(repo_name)
    })?;
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
    println!("{:<8} {:<22} REASON", "ID", "REPO");
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

/// Show the append-only request event history stored in SQLite.
pub fn history(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let events = manifest::request_history(&story)?;
    if events.is_empty() {
        println!("no repo request history for '{story}'.");
        return Ok(());
    }

    println!(
        "{:<17} {:<8} {:<22} {:<9} ACTOR",
        "WHEN", "ID", "REPO", "STATUS"
    );
    for event in events {
        println!(
            "{:<17} {:<8} {:<22} {:<9} {}",
            event.occurred.format("%Y-%m-%d %H:%M"),
            event.request_id,
            event.repo,
            event.status,
            event.actor,
        );
        if let Some(reason) = event.reason.as_deref().filter(|reason| !reason.is_empty()) {
            println!("  reason: {reason}");
        }
    }
    Ok(())
}
