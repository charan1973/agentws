//! Minimal MCP (Model Context Protocol) server over stdio.
//!
//! Speaks newline-delimited JSON-RPC 2.0. Exposes three tools that let an agent
//! discover, request, and check the status of repos for its current workspace.
//! The current workspace is inferred from the process cwd (the agent's cwd),
//! which is `~/.agentws/<story>`.

use crate::{config, discovery, manifest, ops};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";

pub fn run() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // ignore malformed lines
        };
        if let Some(resp) = handle(&msg)? {
            writeln!(out, "{resp}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Handle one JSON-RPC message. Returns Some(response) if the message is a
/// request (has an id), or None for notifications.
fn handle(msg: &Value) -> Result<Option<Value>> {
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();

    // It's a notification (no id) — nothing to return.
    let id = match id {
        Some(id) => id,
        None => return Ok(None),
    };

    let result: Result<Value> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": { "name": "agentws", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "tools": {} }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(msg),
        _ => Ok(json!({})), // acknowledge unknown requests gracefully
    };

    let resp = match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32603, "message": format!("{e:#}") }
        }),
    };
    Ok(Some(resp))
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_available_repos",
            "description": "List repositories available to add to this workspace (not already present). Optionally fuzzy-filter by `query`.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string" } }
            }
        }),
        json!({
            "name": "request_repo",
            "description": "Request permission to add another repository to this workspace. The human must approve via `agentws approve`. Returns a request id to poll with `check_request`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "repository name" },
                    "reason": { "type": "string" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "check_request",
            "description": "Check the status of a repo request. If approved, includes the path to the now-available repository.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
    ]
}

fn call_tool(msg: &Value) -> Result<Value> {
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing tool name"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let result: Result<String> = match name {
        "list_available_repos" => list_available_repos(&args),
        "request_repo" => request_repo(&args),
        "check_request" => check_request(&args),
        other => Err(anyhow::anyhow!("unknown tool '{other}'")),
    };

    Ok(match result {
        Ok(text) => json!({ "content": [ { "type": "text", "text": text } ] }),
        Err(e) => json!({
            "content": [ { "type": "text", "text": format!("Error: {e:#}") } ],
            "isError": true
        }),
    })
}

fn current_workspace() -> Result<manifest::Workspace> {
    let story = manifest::resolve_story(None)?;
    manifest::load(&story)
}

fn list_available_repos(args: &Value) -> Result<String> {
    let ws = current_workspace()?;
    let have: std::collections::HashSet<String> =
        ws.repos.iter().map(|r| r.name.clone()).collect();

    let cfg = config::load()?;
    let mut repos: Vec<_> = discovery::discover(&cfg.repo_roots_expanded())
        .into_iter()
        .filter(|r| !have.contains(&r.name))
        .collect();

    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    if !query.is_empty() {
        let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
        use fuzzy_matcher::FuzzyMatcher;
        let mut scored: Vec<(i64, discovery::Repo)> = repos
            .into_iter()
            .filter_map(|r| matcher.fuzzy_match(&r.name, query).map(|s| (s, r)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        repos = scored.into_iter().map(|(_, r)| r).collect();
    }

    if repos.is_empty() {
        return Ok("No additional repositories available.".into());
    }
    let body = repos
        .iter()
        .map(|r| format!("- {} ({})", r.name, r.path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("Available repositories to request:\n{body}"))
}

fn request_repo(args: &Value) -> Result<String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
    let reason = args.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());

    let mut ws = current_workspace()?;
    if ws.repos.iter().any(|r| r.name == name) {
        return Ok(format!("'{name}' is already in this workspace at ./{}", name));
    }
    // Validate it exists, but don't create it yet.
    ops::find_repo(name)?;

    let id = ops::new_id();
    let req = manifest::RepoRequest {
        id: id.clone(),
        repo: name.to_string(),
        reason: reason.clone(),
        status: "pending".into(),
        by: "agent".into(),
        created: chrono::Utc::now(),
        resolved: None,
    };
    ws.requests.push(req);
    manifest::save(&ws)?;

    notify(
        "agentws",
        &format!(
            "Agent requests '{name}'. Approve: agentws approve {id}",
        ),
    );

    Ok(format!(
        "Request queued (id {id}). Awaiting human approval. \
         Poll with check_request(id=\"{id}\")."
    ))
}

fn check_request(args: &Value) -> Result<String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'id'"))?;
    let ws = current_workspace()?;
    let req = ws
        .requests
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow::anyhow!("no request with id '{id}'"))?;
    match req.status.as_str() {
        "pending" => Ok(format!(
            "Request {} for '{}' is still pending human approval.",
            req.id, req.repo
        )),
        "denied" => Ok(format!(
            "Request {} for '{}' was denied by the human.",
            req.id, req.repo
        )),
        "approved" => {
            let path = ws
                .repos
                .iter()
                .find(|r| r.name == req.repo)
                .map(|r| r.worktree.display().to_string())
                .unwrap_or_else(|| format!("./{}", req.repo));
            Ok(format!(
                "Request {} for '{}' approved. It is now available at: {path}",
                req.id, req.repo
            ))
        }
        other => Ok(format!("Request {}: {other}", req.id)),
    }
}

/// Fire-and-forget desktop notification + stderr line.
fn notify(title: &str, body: &str) {
    // Strip characters that would break an AppleScript string literal.
    let clean = |s: &str| s.replace(['"', '\\'], "");
    let t = clean(title);
    let b = clean(body);
    #[cfg(target_os = "macos")]
    {
        let script =
            format!("display notification \"{b}\" with title \"{t}\"");
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn();
    }
    eprintln!("[agentws] {t}: {b}");
}
