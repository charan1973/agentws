//! Shared workspace operations used by multiple commands
//! (adding/removing repos, symlink + hook application, request ids).

use crate::{config, discovery, manifest, worktree};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Short, unique-enough id for a repo request.
pub fn new_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
    format!("{:04x}", (t ^ n.rotate_left(8)) & 0xffff)
}

/// Resolve a repo name to its origin path by scanning configured roots.
pub fn find_repo(name: &str) -> Result<discovery::Repo> {
    let cfg = config::load()?;
    let roots = cfg.repo_roots_expanded();
    let all = discovery::discover(&roots);
    all.into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow!("no discovered repo named '{name}'"))
}

/// Create a worktree for `name` under the workspace root, apply symlinks/hooks,
/// and push a RepoEntry onto the workspace. Caller persists the manifest.
pub fn add_repo_to_workspace(
    ws: &mut manifest::Workspace,
    name: &str,
    base_override: Option<&str>,
) -> Result<manifest::RepoEntry> {
    if ws.repos.iter().any(|r| r.name == name) {
        bail!("repo '{name}' is already in workspace '{}'", ws.story);
    }
    let repo = find_repo(name)?;
    let dest = ws.root.join(name);
    let branch = format!("feat/{}", ws.story);
    let base = match base_override {
        Some(b) => b.to_string(),
        None => worktree::default_branch(&repo.path).unwrap_or_else(|_| "main".to_string()),
    };
    worktree::add_worktree(&repo.path, &dest, &branch, &base)?;
    apply_symlinks(&repo.path, &dest);
    run_post_create(&dest);
    let entry = manifest::RepoEntry {
        name: name.to_string(),
        origin: repo.path.clone(),
        worktree: dest,
        branch,
        base,
    };
    ws.repos.push(entry.clone());
    Ok(entry)
}

/// Remove a repo's worktree and drop its entry. Caller persists the manifest.
pub fn remove_repo_from_workspace(ws: &mut manifest::Workspace, name: &str) -> Result<()> {
    let idx = ws
        .repos
        .iter()
        .position(|r| r.name == name)
        .ok_or_else(|| anyhow!("repo '{name}' is not in workspace '{}'", ws.story))?;
    let entry = ws.repos.remove(idx);
    worktree::remove_worktree(&entry.origin, &entry.worktree)?;
    Ok(())
}

/// Symlink configured paths (node_modules, .env, …) from origin into the worktree.
pub fn apply_symlinks(origin: &Path, dest: &Path) {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    for pat in &cfg.symlinks {
        // Treat each configured entry as a literal name/glob in the origin root.
        for entry in match glob_in(origin, pat) {
            Ok(v) => v,
            Err(_) => continue,
        } {
            let name = match entry.file_name() {
                Some(n) => n,
                None => continue,
            };
            let link = dest.join(name);
            if link.exists() || link.is_symlink() {
                continue;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&entry, &link).ok();
            #[cfg(not(unix))]
            std::fs::copy(&entry, &link).ok();
        }
    }
}

/// Run the configured `post_create` hook inside the worktree after creation.
pub fn run_post_create(dest: &Path) {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    let Some(cmd) = cfg.post_create.as_deref() else {
        return;
    };
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dest)
        .status();
}

/// Re-read values exposed by one repository and upsert managed dotenv blocks
/// into consumer repositories. Returns the number of files changed.
pub fn wire_env(ws: &manifest::Workspace) -> Result<usize> {
    let cfg = config::load()?;
    wire_env_with_config(ws, &cfg)
}

fn wire_env_with_config(ws: &manifest::Workspace, cfg: &config::Config) -> Result<usize> {
    if cfg.env.is_empty() {
        return Ok(0);
    }

    let repos: HashMap<_, _> = ws
        .repos
        .iter()
        .map(|repo| (repo.name.as_str(), repo))
        .collect();
    let mut exposed = HashMap::new();

    for (repo_name, rule) in &cfg.env {
        let Some(repo) = repos.get(repo_name.as_str()) else {
            continue;
        };
        let env_path = safe_repo_path(&repo.worktree, &rule.file)?;
        let values = read_dotenv(&env_path)?;
        for (alias, env_key) in &rule.exposes {
            if let Some(value) = values.get(env_key) {
                exposed.insert(format!("{repo_name}.{alias}"), value.clone());
            }
        }
    }

    let mut changed = 0;
    for (repo_name, rule) in &cfg.env {
        let Some(repo) = repos.get(repo_name.as_str()) else {
            continue;
        };
        let mut resolved = BTreeMap::new();
        for (env_key, template) in &rule.consumes {
            validate_env_key(env_key)?;
            let value =
                render_env_template(template, &exposed, &rule.defaults).with_context(|| {
                    format!("resolving env wiring for repository '{repo_name}', key '{env_key}'")
                })?;
            if value.contains(['\n', '\r']) {
                bail!("resolved value for '{repo_name}.{env_key}' contains a newline");
            }
            resolved.insert(env_key.clone(), value);
        }

        let env_path = safe_repo_path(&repo.worktree, &rule.file)?;
        if upsert_env_block(&env_path, &ws.story, &resolved)? {
            changed += 1;
        }
    }
    Ok(changed)
}

fn safe_repo_path(repo: &Path, relative: &Path) -> Result<std::path::PathBuf> {
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "env file path '{}' must stay inside its repository",
            relative.display()
        );
    }
    let path = repo.join(relative);
    let mut component_path = repo.to_path_buf();
    for component in relative.components() {
        if matches!(component, Component::CurDir) {
            continue;
        }
        component_path.push(component.as_os_str());
        if component_path.is_symlink() {
            bail!(
                "env file path '{}' traverses symlink {}",
                relative.display(),
                component_path.display()
            );
        }
    }
    Ok(path)
}

fn read_dotenv(path: &Path) -> Result<HashMap<String, String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("reading env file {}", path.display()))
        }
    };
    let mut values = HashMap::new();
    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !is_env_key(key) {
            continue;
        }
        let value = unquote_dotenv(raw_value.trim());
        values.insert(key.to_string(), value.to_string());
    }
    Ok(values)
}

fn unquote_dotenv(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn render_env_template(
    template: &str,
    exposed: &HashMap<String, String>,
    defaults: &BTreeMap<String, String>,
) -> Result<String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let end = after_start
            .find('}')
            .ok_or_else(|| anyhow!("unclosed '{{' in template '{template}'"))?;
        let key = &after_start[..end];
        if key.is_empty() {
            bail!("empty placeholder in template '{template}'");
        }
        let value = exposed
            .get(key)
            .or_else(|| defaults.get(key))
            .ok_or_else(|| anyhow!("no exposed value or default for '{{{key}}}'"))?;
        output.push_str(value);
        rest = &after_start[end + 1..];
    }
    if rest.contains('}') {
        bail!("unmatched '}}' in template '{template}'");
    }
    output.push_str(rest);
    Ok(output)
}

fn upsert_env_block(path: &Path, story: &str, values: &BTreeMap<String, String>) -> Result<bool> {
    if path.is_symlink() {
        bail!(
            "cannot manage env file {} because it is a symlink; remove it from `symlinks` or from `[env]`",
            path.display()
        );
    }
    let existing_permissions = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("reading env file {}", path.display()))
        }
    };
    let start = format!("# >>> agentws-managed: {story} >>>");
    let end = format!("# <<< agentws-managed: {story} <<<");
    let mut kept = Vec::new();
    let mut in_block = false;
    let mut found_start = false;
    let mut found_end = false;
    for line in existing.lines() {
        if line.trim() == start {
            if in_block || found_start {
                bail!("env file {} has duplicate agentws blocks", path.display());
            }
            in_block = true;
            found_start = true;
            continue;
        }
        if line.trim() == end {
            if !in_block {
                bail!(
                    "env file {} has an unmatched agentws block end",
                    path.display()
                );
            }
            in_block = false;
            found_end = true;
            continue;
        }
        if !in_block {
            kept.push(line);
        }
    }
    if in_block || found_start != found_end {
        bail!(
            "env file {} has an unterminated agentws block",
            path.display()
        );
    }
    if !found_start && values.is_empty() {
        return Ok(false);
    }

    let mut updated = kept.join("\n").trim_end().to_string();
    if !values.is_empty() {
        if !updated.is_empty() {
            updated.push_str("\n\n");
        }
        updated.push_str(&start);
        updated.push('\n');
        for (key, value) in values {
            updated.push_str(key);
            updated.push('=');
            updated.push_str(&encode_dotenv_value(value));
            updated.push('\n');
        }
        updated.push_str(&end);
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    if updated == existing {
        return Ok(false);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("agentws-{}.tmp", new_id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("creating temporary env file {}", temp.display()))?;
    file.write_all(updated.as_bytes())
        .with_context(|| format!("writing temporary env file {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing temporary env file {}", temp.display()))?;
    drop(file);
    if let Some(permissions) = existing_permissions {
        std::fs::set_permissions(&temp, permissions)
            .with_context(|| format!("preserving permissions for {}", path.display()))?;
    }
    std::fs::rename(&temp, path)
        .with_context(|| format!("updating env file {}", path.display()))?;
    Ok(true)
}

fn validate_env_key(key: &str) -> Result<()> {
    if !is_env_key(key) {
        bail!("'{key}' is not a valid dotenv key");
    }
    Ok(())
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some('_') | Some('A'..='Z') | Some('a'..='z'))
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn encode_dotenv_value(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '#' || ch == '"' || ch == '\\')
    {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// Simple non-recursive glob over immediate children of `dir` matching `pat`.
fn glob_in(dir: &Path, pat: &str) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let prefix = pat.split_once('*').map(|(p, _)| p).unwrap_or(pat);
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if pat == name.as_ref() || (pat.contains('*') && name.starts_with(prefix)) {
            out.push(entry.path());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn new_id_is_four_hex_chars() {
        let id = new_id();
        assert_eq!(id.len(), 4, "id was {id}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "id was {id}");
    }

    #[test]
    fn new_ids_differ_across_calls() {
        // 4-hex space (65536); two consecutive calls colliding is ~1/65536 — fine for CI
        let mut ids = Vec::new();
        for _ in 0..8 {
            ids.push(new_id());
        }
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert!(unique.len() > 1, "ids should vary across calls");
    }

    #[test]
    fn wires_exposed_values_and_falls_back_after_repo_removal() {
        let tmp = tempfile::tempdir().unwrap();
        let api = tmp.path().join("api");
        let web = tmp.path().join("web");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(api.join(".env.local"), "PORT=51234\n").unwrap();
        std::fs::write(web.join(".env.local"), "KEEP=1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                web.join(".env.local"),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }

        let mut cfg = config::Config::default();
        cfg.env.insert(
            "api".into(),
            config::RepoEnv {
                file: ".env.local".into(),
                exposes: BTreeMap::from([("port".into(), "PORT".into())]),
                ..Default::default()
            },
        );
        cfg.env.insert(
            "web".into(),
            config::RepoEnv {
                file: ".env.local".into(),
                consumes: BTreeMap::from([(
                    "API_URL".into(),
                    "http://localhost:{api.port}".into(),
                )]),
                defaults: BTreeMap::from([("api.port".into(), "5000".into())]),
                ..Default::default()
            },
        );
        let repo = |name: &str, worktree: &Path| manifest::RepoEntry {
            name: name.into(),
            origin: tmp.path().join("origins").join(name),
            worktree: worktree.into(),
            branch: "feat/demo".into(),
            base: "main".into(),
        };
        let mut ws = manifest::Workspace {
            story: "demo".into(),
            root: tmp.path().into(),
            created: Utc::now(),
            repos: vec![repo("api", &api), repo("web", &web)],
            requests: vec![],
            archived: false,
        };

        assert_eq!(wire_env_with_config(&ws, &cfg).unwrap(), 1);
        let wired = std::fs::read_to_string(web.join(".env.local")).unwrap();
        assert!(wired.contains("KEEP=1"));
        assert!(wired.contains("API_URL=http://localhost:51234"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(web.join(".env.local"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(wire_env_with_config(&ws, &cfg).unwrap(), 0);

        ws.repos.retain(|entry| entry.name != "api");
        assert_eq!(wire_env_with_config(&ws, &cfg).unwrap(), 1);
        let rewired = std::fs::read_to_string(web.join(".env.local")).unwrap();
        assert!(rewired.contains("API_URL=http://localhost:5000"));
        assert_eq!(rewired.matches("agentws-managed").count(), 2);
    }

    #[test]
    fn env_file_cannot_escape_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let err = safe_repo_path(tmp.path(), Path::new("../secret.env")).unwrap_err();
        assert!(err.to_string().contains("must stay inside"));

        let outside = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("linked")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(outside.path(), tmp.path().join("linked")).unwrap();
        let err = safe_repo_path(tmp.path(), Path::new("linked/secret.env")).unwrap_err();
        assert!(err.to_string().contains("traverses symlink"));
    }
}
