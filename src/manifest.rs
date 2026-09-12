use crate::config;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub story: String,
    pub root: PathBuf,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub requests: Vec<RepoRequest>,
    /// Set true after `archive` removes the worktrees.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub origin: PathBuf,
    pub worktree: PathBuf,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRequest {
    pub id: String,
    pub repo: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// pending | approved | denied
    pub status: String,
    /// agent | human
    pub by: String,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub resolved: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestEvent {
    pub request_id: String,
    pub repo: String,
    pub status: String,
    pub actor: String,
    pub occurred: DateTime<Utc>,
    pub reason: Option<String>,
}

pub fn root_for(story: &str) -> Result<PathBuf> {
    config::workspace_dir(story)
}

pub fn manifest_path_for(story: &str) -> Result<PathBuf> {
    Ok(root_for(story)?.join("workspace.db"))
}

pub fn exists(story: &str) -> bool {
    root_for(story)
        .map(|root| database_path(&root).exists() || legacy_path(&root).exists())
        .unwrap_or(false)
}

pub fn save(ws: &Workspace) -> Result<()> {
    fs::create_dir_all(&ws.root)?;
    let mut conn = open_database(&database_path(&ws.root))?;
    let tx = conn
        .transaction()
        .context("starting workspace database transaction")?;
    save_transaction(&tx, ws)?;
    tx.commit().context("committing workspace database")
}

pub fn load(story: &str) -> Result<Workspace> {
    load_root(&root_for(story)?)
}

/// Serialize a read-modify-write operation with an immediate SQLite
/// transaction. This prevents concurrent CLI/MCP writers from losing updates.
pub fn mutate<T>(story: &str, f: impl FnOnce(&mut Workspace) -> Result<T>) -> Result<T> {
    let root = root_for(story)?;
    // Ensure a legacy JSON manifest is migrated before opening the transaction.
    let _ = load_root(&root)?;
    let path = database_path(&root);
    let mut conn = open_database(&path)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("starting immediate workspace database transaction")?;
    let mut ws = load_connection(&tx, &path)?;
    let value = f(&mut ws)?;
    save_transaction(&tx, &ws)?;
    tx.commit().context("committing workspace database")?;
    Ok(value)
}

/// Load a legacy JSON manifest directly. Kept public for compatibility and
/// migration tests; normal callers should use [`load`].
pub fn load_path(path: &Path) -> Result<Workspace> {
    let text =
        fs::read_to_string(path).with_context(|| format!("reading manifest {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing manifest {}", path.display()))
}

/// Return the append-only request history for a workspace, oldest event first.
pub fn request_history(story: &str) -> Result<Vec<RequestEvent>> {
    let root = root_for(story)?;
    // Loading performs the one-time JSON -> SQLite migration when needed.
    let _ = load_root(&root)?;
    request_history_root(&root)
}

fn database_path(root: &Path) -> PathBuf {
    root.join("workspace.db")
}

fn legacy_path(root: &Path) -> PathBuf {
    root.join("workspace.json")
}

fn open_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("opening workspace database {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS workspace (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           schema_version INTEGER NOT NULL,
           story TEXT NOT NULL,
           root TEXT NOT NULL,
           created TEXT NOT NULL,
           archived INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS repos (
           name TEXT PRIMARY KEY,
           origin TEXT NOT NULL,
           worktree TEXT NOT NULL,
           branch TEXT NOT NULL,
           base TEXT NOT NULL,
           ordinal INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS requests (
           id TEXT PRIMARY KEY,
           repo TEXT NOT NULL,
           reason TEXT,
           status TEXT NOT NULL,
           requested_by TEXT NOT NULL,
           created TEXT NOT NULL,
           resolved TEXT,
           ordinal INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS request_events (
           event_id INTEGER PRIMARY KEY AUTOINCREMENT,
           request_id TEXT NOT NULL,
           repo TEXT NOT NULL,
           status TEXT NOT NULL,
           actor TEXT NOT NULL,
           occurred TEXT NOT NULL,
           reason TEXT
         );
         CREATE INDEX IF NOT EXISTS request_events_request_id
           ON request_events(request_id, event_id);",
    )
    .context("initializing workspace database schema")?;
    Ok(conn)
}

fn save_transaction(tx: &Transaction<'_>, ws: &Workspace) -> Result<()> {
    let old_statuses: HashMap<String, String> = {
        let mut stmt = tx.prepare("SELECT id, status FROM requests")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };

    tx.execute(
        "INSERT INTO workspace
           (singleton, schema_version, story, root, created, archived)
         VALUES (1, 1, ?1, ?2, ?3, ?4)
         ON CONFLICT(singleton) DO UPDATE SET
           schema_version = excluded.schema_version,
           story = excluded.story,
           root = excluded.root,
           created = excluded.created,
           archived = excluded.archived",
        params![
            ws.story,
            ws.root.to_string_lossy(),
            timestamp(ws.created),
            i64::from(ws.archived),
        ],
    )?;

    tx.execute("DELETE FROM repos", [])?;
    for (ordinal, repo) in ws.repos.iter().enumerate() {
        tx.execute(
            "INSERT INTO repos (name, origin, worktree, branch, base, ordinal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                repo.name,
                repo.origin.to_string_lossy(),
                repo.worktree.to_string_lossy(),
                repo.branch,
                repo.base,
                ordinal as i64,
            ],
        )?;
    }

    // Mark existing rows and remove any that are no longer present after the
    // upsert loop. Their append-only event history intentionally remains.
    tx.execute("UPDATE requests SET ordinal = -1", [])?;
    for (ordinal, request) in ws.requests.iter().enumerate() {
        tx.execute(
            "INSERT INTO requests
               (id, repo, reason, status, requested_by, created, resolved, ordinal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
               repo = excluded.repo,
               reason = excluded.reason,
               status = excluded.status,
               requested_by = excluded.requested_by,
               created = excluded.created,
               resolved = excluded.resolved,
               ordinal = excluded.ordinal",
            params![
                request.id,
                request.repo,
                request.reason,
                request.status,
                request.by,
                timestamp(request.created),
                request.resolved.map(timestamp),
                ordinal as i64,
            ],
        )?;

        match old_statuses.get(&request.id) {
            None => seed_request_history(tx, request)?,
            Some(old) if old != &request.status => insert_request_event(
                tx,
                request,
                &request.status,
                if request.status == "pending" {
                    &request.by
                } else {
                    "human"
                },
                request.resolved.unwrap_or_else(Utc::now),
            )?,
            _ => {}
        }
    }
    tx.execute("DELETE FROM requests WHERE ordinal = -1", [])?;
    Ok(())
}

fn seed_request_history(tx: &Transaction<'_>, request: &RepoRequest) -> Result<()> {
    insert_request_event(tx, request, "pending", &request.by, request.created)?;
    if request.status != "pending" {
        insert_request_event(
            tx,
            request,
            &request.status,
            "human",
            request.resolved.unwrap_or(request.created),
        )?;
    }
    Ok(())
}

fn insert_request_event(
    tx: &Transaction<'_>,
    request: &RepoRequest,
    status: &str,
    actor: &str,
    occurred: DateTime<Utc>,
) -> Result<()> {
    tx.execute(
        "INSERT INTO request_events
           (request_id, repo, status, actor, occurred, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            request.id,
            request.repo,
            status,
            actor,
            timestamp(occurred),
            request.reason,
        ],
    )?;
    Ok(())
}

fn load_root(root: &Path) -> Result<Workspace> {
    let db = database_path(root);
    if db.exists() {
        return load_database(&db);
    }

    let legacy = legacy_path(root);
    if legacy.exists() {
        let ws = load_path(&legacy)?;
        save(&ws).with_context(|| {
            format!(
                "migrating legacy manifest {} to {}",
                legacy.display(),
                db.display()
            )
        })?;
        return Ok(ws);
    }

    Err(anyhow!(
        "no workspace manifest found under {}",
        root.display()
    ))
}

fn load_database(path: &Path) -> Result<Workspace> {
    let conn = open_database(path)?;
    load_connection(&conn, path)
}

fn load_connection(conn: &Connection, path: &Path) -> Result<Workspace> {
    let metadata: (String, String, String, i64) = conn
        .query_row(
            "SELECT story, root, created, archived FROM workspace WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("workspace database {} has no metadata", path.display()))?;

    let repos = {
        let mut stmt = conn
            .prepare("SELECT name, origin, worktree, branch, base FROM repos ORDER BY ordinal")?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoEntry {
                name: row.get(0)?,
                origin: PathBuf::from(row.get::<_, String>(1)?),
                worktree: PathBuf::from(row.get::<_, String>(2)?),
                branch: row.get(3)?,
                base: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let request_rows = {
        let mut stmt = conn.prepare(
            "SELECT id, repo, reason, status, requested_by, created, resolved
             FROM requests ORDER BY ordinal",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let requests = request_rows
        .into_iter()
        .map(|(id, repo, reason, status, by, created, resolved)| {
            Ok(RepoRequest {
                id,
                repo,
                reason,
                status,
                by,
                created: parse_timestamp(&created)?,
                resolved: resolved.as_deref().map(parse_timestamp).transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Workspace {
        story: metadata.0,
        root: PathBuf::from(metadata.1),
        created: parse_timestamp(&metadata.2)?,
        repos,
        requests,
        archived: metadata.3 != 0,
    })
}

fn request_history_root(root: &Path) -> Result<Vec<RequestEvent>> {
    let conn = open_database(&database_path(root))?;
    let mut stmt = conn.prepare(
        "SELECT request_id, repo, status, actor, occurred, reason
         FROM request_events ORDER BY event_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    rows.map(|row| {
        let (request_id, repo, status, actor, occurred, reason) = row?;
        Ok(RequestEvent {
            request_id,
            repo,
            status,
            actor,
            occurred: parse_timestamp(&occurred)?,
            reason,
        })
    })
    .collect()
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid timestamp '{value}' in workspace database"))?
        .with_timezone(&Utc))
}

/// Infer the workspace story from the current working directory, if it is
/// (or is inside) `~/.agentws/<story>`.
pub fn infer_story_from_cwd() -> Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    let base = config::workspaces_root()?;
    if let Ok(rel) = cwd.strip_prefix(&base) {
        if let Some(first) = rel.components().next() {
            return Ok(Some(first.as_os_str().to_string_lossy().to_string()));
        }
    }
    Ok(None)
}

/// Resolve which workspace a command targets, in priority order:
///   1. an explicit `--story` argument,
///   2. the per-shell `$AGENTWS_WORKSPACE` env var (set by `activate`),
///   3. the cwd is inside a workspace dir (`~/.agentws/<story>`),
///   4. the active-workspace pointer (`~/.agentws/.current`),
///   5. there is exactly one workspace — use it,
///   6. otherwise: error with a helpful hint.
pub fn resolve_story(given: Option<String>) -> Result<String> {
    // 1. explicit
    if let Some(s) = given.filter(|s| !s.is_empty()) {
        return Ok(s);
    }
    // 2. activated shell ($AGENTWS_WORKSPACE env var) — per-shell, authoritative
    if let Ok(s) = std::env::var("AGENTWS_WORKSPACE") {
        if !s.is_empty() && exists(&s) {
            return Ok(s);
        }
    }
    // 3. cwd inside a workspace
    if let Some(s) = infer_story_from_cwd()? {
        return Ok(s);
    }
    // 4. active pointer (global fallback)
    if let Some(s) = get_current()? {
        if exists(&s) {
            return Ok(s);
        }
    }
    // 5. exactly one workspace
    let stories = list_stories().unwrap_or_default();
    if stories.len() == 1 {
        return Ok(stories[0].clone());
    }
    // 6. error
    let hint = if stories.is_empty() {
        "No workspaces exist yet. Create one with `agentws new <story>`.".to_string()
    } else {
        format!(
            "Specify one with `agentws <cmd> --story <name>`, or set an active one with
  `agentws use <name>`.
Available workspaces: {}",
            stories.join(", ")
        )
    };
    anyhow::bail!("could not determine which workspace to use.\n{hint}")
}

/// Path to the active-workspace pointer file: `~/.agentws/.current`.
fn current_pointer_path() -> Result<PathBuf> {
    Ok(config::workspaces_root()?.join(".current"))
}

/// Set the active workspace (used as a fallback by `resolve_story`).
pub fn set_current(story: &str) -> Result<()> {
    let path = current_pointer_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, story)?;
    Ok(())
}

/// Read the active workspace, if any. Stale pointers (pointing at a deleted
/// workspace) are treated as absent.
pub fn get_current() -> Result<Option<String>> {
    let path = current_pointer_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&path)?.trim().to_string();
    if s.is_empty() || !exists(&s) {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// Clear the active-workspace pointer if (and only if) it points at `story`.
/// Reads the raw file content so deletion still works once the manifest is gone.
pub fn clear_current_if(story: &str) -> Result<()> {
    let path = current_pointer_path()?;
    if !path.exists() {
        return Ok(());
    }
    let s = fs::read_to_string(&path)?.trim().to_string();
    if s == story {
        fs::remove_file(path).ok();
    }
    Ok(())
}

pub fn list_stories() -> Result<Vec<String>> {
    let root = config::workspaces_root()?;
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && (p.join("workspace.db").exists() || p.join("workspace.json").exists())
            {
                if let Some(name) = p.file_name() {
                    out.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample(story: &str, root: std::path::PathBuf) -> Workspace {
        Workspace {
            story: story.into(),
            root: root.clone(),
            created: Utc::now(),
            repos: vec![RepoEntry {
                name: "api".into(),
                origin: root.join("origin/api"),
                worktree: root.join("api"),
                branch: format!("feat/{story}"),
                base: "main".into(),
            }],
            requests: vec![RepoRequest {
                id: "ab12".into(),
                repo: "payments".into(),
                reason: Some("need the client".into()),
                status: "pending".into(),
                by: "agent".into(),
                created: Utc::now(),
                resolved: None,
            }],
            archived: false,
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let ws = sample("demo", root.clone());

        save(&ws).unwrap();
        let loaded = load_database(&root.join("workspace.db")).unwrap();

        assert_eq!(loaded.story, "demo");
        assert_eq!(loaded.repos.len(), 1);
        assert_eq!(loaded.repos[0].name, "api");
        assert_eq!(loaded.repos[0].branch, "feat/demo");
        assert_eq!(loaded.requests.len(), 1);
        assert_eq!(loaded.requests[0].status, "pending");
        assert!(!loaded.archived);
        assert!(root.join("workspace.db").exists());
    }

    #[test]
    fn request_history_records_status_transitions() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let mut ws = sample("demo", root.clone());

        save(&ws).unwrap();
        ws.requests[0].status = "approved".into();
        ws.requests[0].resolved = Some(Utc::now());
        save(&ws).unwrap();
        save(&ws).unwrap(); // unchanged saves must not duplicate history

        let events = request_history_root(&root).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].status, "pending");
        assert_eq!(events[0].actor, "agent");
        assert_eq!(events[1].status, "approved");
        assert_eq!(events[1].actor, "human");
    }

    #[test]
    fn legacy_json_is_migrated_but_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("legacy");
        std::fs::create_dir_all(&root).unwrap();
        let ws = sample("legacy", root.clone());
        let json = serde_json::to_string_pretty(&ws).unwrap();
        std::fs::write(root.join("workspace.json"), json).unwrap();

        let loaded = load_root(&root).unwrap();
        assert_eq!(loaded.story, "legacy");
        assert!(root.join("workspace.db").exists());
        assert!(root.join("workspace.json").exists());

        let events = request_history_root(&root).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, "pending");
    }

    #[test]
    fn legacy_manifest_with_agent_field_still_loads() {
        // manifests written before the activation pivot had an `agent` field;
        // since there's no deny_unknown_fields, serde must ignore it.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("workspace.json");
        std::fs::write(
            &path,
            r#"{
  "story": "legacy",
  "root": "/tmp/legacy",
  "created": "2026-01-01T00:00:00Z",
  "agent": { "kind": "claude", "pid": 123 },
  "repos": [],
  "requests": [],
  "archived": false
}"#,
        )
        .unwrap();
        let ws = load_path(&path).unwrap();
        assert_eq!(ws.story, "legacy");
        assert!(ws.repos.is_empty());
    }
}
