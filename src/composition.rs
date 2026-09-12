//! Root-level instruction and skill composition for multi-repository workspaces.

use crate::{config, library, manifest};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const LIBRARY_SOURCE: &str = "library";
const REPO_SOURCE: &str = "repo";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshReport {
    pub library_skills: usize,
    pub repo_skills: usize,
    pub snippets: usize,
    pub changed_files: usize,
}

pub fn refresh(ws: &mut manifest::Workspace) -> Result<RefreshReport> {
    let cfg = config::load()?;
    refresh_with_config(ws, &cfg)
}

pub fn refresh_with_config(
    ws: &mut manifest::Workspace,
    cfg: &config::Config,
) -> Result<RefreshReport> {
    fs::create_dir_all(&ws.root)?;
    let old_skills = ws.skills.clone();
    let mut desired = resolve_library_skills(&old_skills, cfg)?;
    let library_count = desired.len();
    desired.extend(discover_repo_skills(ws)?);
    ensure_unique_names(&desired)?;

    let mut changed_files = sync_skill_pool(&ws.root, &old_skills, &desired)?;
    changed_files += sync_claude_skill_pool(&ws.root, &old_skills, &desired)?;
    changed_files += usize::from(sync_pi_settings(&ws.root, &old_skills, &desired)?);

    ws.skills = desired;
    ws.agents_md = resolve_snippets(&ws.agents_md, cfg)?;
    let instructions = render_agents_md(ws)?;
    changed_files += usize::from(write_if_changed(&ws.root.join("AGENTS.md"), &instructions)?);
    // Claude Code's native project instruction filename is CLAUDE.md. Keeping
    // the same generated content here makes the universal root policy explicit
    // without requiring a user-level fallback configuration.
    changed_files += usize::from(write_if_changed(&ws.root.join("CLAUDE.md"), &instructions)?);

    Ok(RefreshReport {
        library_skills: library_count,
        repo_skills: ws.skills.len().saturating_sub(library_count),
        snippets: ws.agents_md.len(),
        changed_files,
    })
}

fn resolve_library_skills(
    existing: &[manifest::SkillSelection],
    cfg: &config::Config,
) -> Result<Vec<manifest::SkillSelection>> {
    existing
        .iter()
        .filter(|skill| skill.source == LIBRARY_SOURCE)
        .map(|skill| {
            if skill.copied {
                return Ok(skill.clone());
            }
            let item = library::resolve(cfg, library::Kind::Skill, &skill.name).with_context(|| {
                format!(
                    "workspace selected library skill '{}'; restore it or update the workspace template",
                    skill.name
                )
            })?;
            Ok(manifest::SkillSelection {
                name: skill.name.clone(),
                source: LIBRARY_SOURCE.into(),
                path: item.path,
                copied: skill.copied,
            })
        })
        .collect()
}

fn resolve_snippets(
    existing: &[manifest::AgentsMdSelection],
    cfg: &config::Config,
) -> Result<Vec<manifest::AgentsMdSelection>> {
    existing
        .iter()
        .filter(|snippet| snippet.source == LIBRARY_SOURCE)
        .map(|snippet| {
            let item = library::resolve(cfg, library::Kind::AgentsMd, &snippet.name)
                .with_context(|| {
                    format!(
                        "workspace selected AGENTS.md snippet '{}'; restore it or update the workspace template",
                        snippet.name
                    )
                })?;
            Ok(manifest::AgentsMdSelection {
                name: snippet.name.clone(),
                source: LIBRARY_SOURCE.into(),
                path: item.path,
            })
        })
        .collect()
}

fn discover_repo_skills(ws: &manifest::Workspace) -> Result<Vec<manifest::SkillSelection>> {
    let mut output = Vec::new();
    let mut seen_sources = HashSet::new();
    for repo in &ws.repos {
        for relative in [
            ".agents/skills",
            ".pi/skills",
            ".claude/skills",
            ".opencode/skills",
        ] {
            let pool = repo.worktree.join(relative);
            let entries = match fs::read_dir(&pool) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("reading repository skills {}", pool.display()))
                }
            };
            let mut paths = entries
                .collect::<std::io::Result<Vec<_>>>()?
                .into_iter()
                .map(|entry| entry.path())
                .filter(|path| path.join("SKILL.md").is_file())
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                let canonical = path.canonicalize().unwrap_or(path.clone());
                if !seen_sources.insert(canonical.clone()) {
                    continue;
                }
                let local_name = path
                    .file_name()
                    .context("repository skill path has no name")?
                    .to_string_lossy();
                output.push(manifest::SkillSelection {
                    name: namespaced_skill_name(&repo.name, &local_name),
                    source: REPO_SOURCE.into(),
                    path: canonical,
                    copied: false,
                });
            }
        }
    }
    output.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(output)
}

fn namespaced_skill_name(repo: &str, skill: &str) -> String {
    let mut base = format!("{}-{}", safe_component(repo), safe_component(skill));
    base.make_ascii_lowercase();
    if base.len() <= 64 {
        return base;
    }
    let hash = fnv1a(base.as_bytes());
    base.truncate(55);
    format!("{}-{hash:08x}", base.trim_end_matches('-'))
}

fn safe_component(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            previous_dash = false;
        } else if !previous_dash && !output.is_empty() {
            output.push('-');
            previous_dash = true;
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        "item".into()
    } else {
        output.to_string()
    }
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn ensure_unique_names(skills: &[manifest::SkillSelection]) -> Result<()> {
    let mut names = HashSet::new();
    for skill in skills {
        if skill.name.is_empty() || !names.insert(&skill.name) {
            bail!(
                "multiple workspace skills resolve to '{}'; rename one source",
                skill.name
            );
        }
    }
    Ok(())
}

fn sync_skill_pool(
    root: &Path,
    old: &[manifest::SkillSelection],
    desired: &[manifest::SkillSelection],
) -> Result<usize> {
    let pool = root.join(".agents/skills");
    fs::create_dir_all(&pool)?;
    let mut changed = 0;
    let desired_names: HashSet<_> = desired.iter().map(|skill| skill.name.as_str()).collect();
    for previous in old {
        if !desired_names.contains(previous.name.as_str()) {
            remove_managed_path(&pool.join(&previous.name))?;
            changed += 1;
        }
    }

    let old_by_name = old
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect::<std::collections::HashMap<_, _>>();
    for skill in desired {
        let destination = pool.join(&skill.name);
        let was_managed = old_by_name.get(skill.name.as_str()).copied();
        if skill.source == LIBRARY_SOURCE && skill.copied {
            if was_managed.is_some_and(|old| old.copied)
                && destination.is_dir()
                && !destination.is_symlink()
            {
                continue;
            }
            prepare_destination(&destination, was_managed.is_some())?;
            library::copy_dir(&skill.path, &destination)?;
            changed += 1;
        } else if skill.source == REPO_SOURCE {
            if was_managed.is_some()
                && repo_materialization_is_current(&skill.path, &destination, &skill.name)
            {
                continue;
            }
            prepare_destination(&destination, was_managed.is_some())?;
            materialize_repo_skill(&skill.path, &destination, &skill.name)?;
            changed += 1;
        } else {
            if symlink_points_to(&destination, &skill.path) {
                continue;
            }
            prepare_destination(&destination, was_managed.is_some())?;
            create_dir_symlink(&skill.path, &destination)?;
            changed += 1;
        }
    }
    Ok(changed)
}

fn sync_claude_skill_pool(
    root: &Path,
    old: &[manifest::SkillSelection],
    desired: &[manifest::SkillSelection],
) -> Result<usize> {
    let pool = root.join(".claude/skills");
    fs::create_dir_all(&pool)?;
    let mut changed = 0;
    let desired_names: HashSet<_> = desired.iter().map(|skill| skill.name.as_str()).collect();
    for previous in old {
        if !desired_names.contains(previous.name.as_str()) {
            remove_managed_path(&pool.join(&previous.name))?;
            changed += 1;
        }
    }
    let old_names: HashSet<_> = old.iter().map(|skill| skill.name.as_str()).collect();
    for skill in desired {
        let destination = pool.join(&skill.name);
        let source = root.join(".agents/skills").join(&skill.name);
        if symlink_points_to(&destination, &source) {
            continue;
        }
        prepare_destination(&destination, old_names.contains(skill.name.as_str()))?;
        create_dir_symlink(&source, &destination)?;
        changed += 1;
    }
    Ok(changed)
}

fn prepare_destination(path: &Path, was_managed: bool) -> Result<()> {
    if path.exists() || path.is_symlink() {
        if !was_managed {
            bail!(
                "refusing to replace unmanaged skill path {}; move it or rename the selected skill",
                path.display()
            );
        }
        remove_managed_path(path)?;
    }
    Ok(())
}

fn remove_managed_path(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn symlink_points_to(link: &Path, expected: &Path) -> bool {
    if !link.is_symlink() {
        return false;
    }
    let Ok(target) = fs::read_link(link) else {
        return false;
    };
    let target = if target.is_absolute() {
        target
    } else {
        link.parent().unwrap_or_else(|| Path::new(".")).join(target)
    };
    if target == expected {
        return true;
    }
    match (target.canonicalize(), expected.canonicalize()) {
        (Ok(target), Ok(expected)) => target == expected,
        _ => false,
    }
}

fn create_dir_symlink(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination).with_context(|| {
        format!(
            "linking workspace skill {} -> {}",
            destination.display(),
            source.display()
        )
    })?;
    #[cfg(not(unix))]
    std::os::windows::fs::symlink_dir(source, destination).with_context(|| {
        format!(
            "linking workspace skill {} -> {}",
            destination.display(),
            source.display()
        )
    })?;
    Ok(())
}

/// Repository skills are namespaced to avoid collisions. The skill standard
/// exposes the frontmatter name to agents, so a plain directory symlink would
/// retain the unqualified name. Materialize a tiny managed view that rewrites
/// only that field and symlinks every supporting resource back to the source.
fn materialize_repo_skill(source: &Path, destination: &Path, name: &str) -> Result<()> {
    fs::create_dir_all(destination)?;
    let rewritten = rendered_repo_skill(source, name)?;
    fs::write(destination.join("SKILL.md"), rewritten)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == "SKILL.md" || entry.file_name() == ".agentws-source" {
            continue;
        }
        let target = destination.join(entry.file_name());
        link_any(&entry.path(), &target)?;
    }
    // The fallback wrapper needs a stable link to the entire source. Keeping it
    // for valid skills too makes audits and troubleshooting straightforward.
    create_dir_symlink(source, &destination.join(".agentws-source"))?;
    Ok(())
}

fn rendered_repo_skill(source: &Path, name: &str) -> Result<String> {
    let skill_file = source.join("SKILL.md");
    let original = fs::read_to_string(&skill_file)
        .with_context(|| format!("reading repository skill {}", skill_file.display()))?;
    Ok(rewrite_skill_name(&original, name).unwrap_or_else(|| {
        format!(
            "---\nname: {name}\ndescription: Repository-local skill {name}\n---\n\nRead and follow [the source skill](.agentws-source/SKILL.md). Resolve its relative paths from `.agentws-source/`.\n"
        )
    }))
}

fn repo_materialization_is_current(source: &Path, destination: &Path, name: &str) -> bool {
    if !destination.is_dir() || destination.is_symlink() {
        return false;
    }
    let Ok(expected_skill) = rendered_repo_skill(source, name) else {
        return false;
    };
    if fs::read_to_string(destination.join("SKILL.md"))
        .ok()
        .as_deref()
        != Some(expected_skill.as_str())
        || !symlink_points_to(&destination.join(".agentws-source"), source)
    {
        return false;
    }

    let mut expected = HashSet::from(["SKILL.md".to_string(), ".agentws-source".to_string()]);
    let Ok(entries) = fs::read_dir(source) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "SKILL.md" || name == ".agentws-source" {
            continue;
        }
        if !symlink_points_to(&destination.join(&name), &entry.path()) {
            return false;
        }
        expected.insert(name);
    }
    let Ok(actual) = fs::read_dir(destination) else {
        return false;
    };
    actual
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<HashSet<_>>()
        == expected
}

fn rewrite_skill_name(text: &str, name: &str) -> Option<String> {
    let mut output = Vec::new();
    let mut in_frontmatter = false;
    let mut closed = false;
    let mut replaced = false;
    let mut has_description = false;
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            if line.trim() != "---" {
                return None;
            }
            in_frontmatter = true;
            output.push(line.to_string());
            continue;
        }
        if in_frontmatter && line.trim() == "---" {
            if !replaced {
                output.push(format!("name: {name}"));
            }
            in_frontmatter = false;
            closed = true;
            output.push(line.to_string());
            continue;
        }
        if in_frontmatter && line.starts_with("description:") {
            has_description = !line["description:".len()..].trim().is_empty();
            output.push(line.to_string());
        } else if in_frontmatter && line.starts_with("name:") {
            output.push(format!("name: {name}"));
            replaced = true;
        } else {
            output.push(line.to_string());
        }
    }
    if !closed || !has_description {
        return None;
    }
    let mut rendered = output.join("\n");
    if text.ends_with('\n') {
        rendered.push('\n');
    }
    Some(rendered)
}

fn link_any(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination)?;
    #[cfg(not(unix))]
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, destination)?;
    } else {
        std::os::windows::fs::symlink_file(source, destination)?;
    }
    Ok(())
}

fn pi_skill_path(name: &str) -> String {
    format!("../.agents/skills/{name}")
}

fn sync_pi_settings(
    root: &Path,
    old: &[manifest::SkillSelection],
    desired: &[manifest::SkillSelection],
) -> Result<bool> {
    let path = root.join(".pi/settings.json");
    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let mut document = if original.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(&original)
            .with_context(|| format!("parsing existing Pi settings {}", path.display()))?
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| anyhow!("Pi settings {} must contain a JSON object", path.display()))?;
    let old_managed: HashSet<_> = old.iter().map(|skill| pi_skill_path(&skill.name)).collect();
    let mut values = match object.remove("skills") {
        Some(Value::Array(values)) => values,
        Some(_) => bail!(
            "Pi settings {} has a non-array `skills` value",
            path.display()
        ),
        None => Vec::new(),
    };
    values.retain(|value| {
        value
            .as_str()
            .is_none_or(|path| !old_managed.contains(path))
    });
    for skill in desired {
        let path = pi_skill_path(&skill.name);
        if !values.iter().any(|value| value.as_str() == Some(&path)) {
            values.push(Value::String(path));
        }
    }
    object.insert("skills".into(), Value::Array(values));
    let rendered = format!("{}\n", serde_json::to_string_pretty(&document)?);
    if rendered == original {
        return Ok(false);
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, rendered).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

fn render_agents_md(ws: &manifest::Workspace) -> Result<String> {
    let mut output = format!(
        "# Workspace: {}\n\n\
         This directory is an `agentws` workspace containing only the repositories selected for this story.\n\n\
         ## Scope\n\n\
         Operate within this workspace root. Do not search or read outside it unless explicitly asked. If another repository is required, use the `request_repo` MCP tool or ask the user to run `agentws add <repo>`.\n\n\
         ## Repositories\n\n",
        ws.story
    );
    for repo in &ws.repos {
        output.push_str(&format!("- `{}` at `./{}`\n", repo.name, repo.name));
    }

    if !ws.agents_md.is_empty() {
        output.push_str("\n## Shared guidance\n");
        for snippet in &ws.agents_md {
            let text = fs::read_to_string(&snippet.path)
                .with_context(|| format!("reading AGENTS.md snippet {}", snippet.path.display()))?;
            output.push_str(&format!("\n### {}\n\n{}", snippet.name, text.trim()));
            output.push('\n');
        }
    }

    output.push_str(
        "\n## Repository-specific guidance\n\n\
         Before editing a repository, read the instruction files listed for it. More specific repository instructions override this workspace guidance. Use its namespaced skills from `.agents/skills/` when relevant.\n",
    );
    for repo in &ws.repos {
        output.push_str(&format!("\n### {}\n", repo.name));
        let mut found = false;
        for file in ["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"] {
            if repo.worktree.join(file).is_file() {
                output.push_str(&format!(
                    "- Read `./{}/{file}` before editing.\n",
                    repo.name
                ));
                found = true;
            }
        }
        for dir in [
            ".agents/skills",
            ".pi/skills",
            ".claude/skills",
            ".opencode/skills",
        ] {
            if repo.worktree.join(dir).is_dir() {
                output.push_str(&format!(
                    "- Repository skills originate in `./{}/{dir}` and are exposed under the root skill pool with the `{}` prefix.\n",
                    repo.name,
                    safe_component(&repo.name).to_ascii_lowercase()
                ));
                found = true;
            }
        }
        if !found {
            output.push_str(
                "- No repository-local instruction files or skill pools were detected.\n",
            );
        }
    }
    output.push_str(
        "\n## Refresh and Pi trust\n\n\
         Run `agentws refresh` after pulling repository changes that add or remove skills or instructions. Pi requires approving its project-trust prompt before it loads `.pi/settings.json` and project skills; agentws cannot pre-approve that trust.\n",
    );
    Ok(output)
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\n\n{body}\n"),
        )
        .unwrap();
        path
    }

    fn workspace(root: &Path) -> manifest::Workspace {
        manifest::Workspace {
            story: "demo".into(),
            root: root.into(),
            created: Utc::now(),
            repos: vec![],
            requests: vec![],
            archived: false,
            skills: vec![],
            agents_md: vec![],
            setup: manifest::WorkspaceSetup::default(),
        }
    }

    #[test]
    fn refresh_composes_library_and_repository_sources_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let library_root = tmp.path().join("library");
        let shared = skill(&library_root.join("skills"), "review", "Review carefully.");
        fs::create_dir_all(library_root.join("agents")).unwrap();
        fs::write(library_root.join("agents/house.md"), "- Keep APIs small.\n").unwrap();

        let workspace_root = tmp.path().join("workspace");
        let repo_root = workspace_root.join("api");
        let local = skill(
            &repo_root.join(".agents/skills"),
            "database",
            "Check migrations.",
        );
        fs::write(repo_root.join("AGENTS.md"), "Repo rules\n").unwrap();
        let mut ws = workspace(&workspace_root);
        ws.repos.push(manifest::RepoEntry {
            name: "api".into(),
            origin: tmp.path().join("origin/api"),
            worktree: repo_root,
            branch: "feat/demo".into(),
            base: "main".into(),
        });
        ws.skills.push(manifest::SkillSelection {
            name: "review".into(),
            source: LIBRARY_SOURCE.into(),
            path: shared,
            copied: false,
        });
        ws.agents_md.push(manifest::AgentsMdSelection {
            name: "house".into(),
            source: LIBRARY_SOURCE.into(),
            path: library_root.join("agents/house.md"),
        });
        let cfg = config::Config {
            library_dirs: vec![library_root],
            ..Default::default()
        };

        let first = refresh_with_config(&mut ws, &cfg).unwrap();
        assert_eq!(first.library_skills, 1);
        assert_eq!(first.repo_skills, 1);
        assert!(workspace_root.join(".agents/skills/review").is_symlink());
        assert!(workspace_root.join(".agents/skills/api-database").is_dir());
        let repo_skill =
            fs::read_to_string(workspace_root.join(".agents/skills/api-database/SKILL.md"))
                .unwrap();
        assert!(repo_skill.contains("name: api-database"));
        assert!(workspace_root.join(".claude/skills/review").is_symlink());
        assert_eq!(ws.skills[1].path, local.canonicalize().unwrap());
        let agents = fs::read_to_string(workspace_root.join("AGENTS.md")).unwrap();
        assert!(agents.contains("Keep APIs small"));
        assert!(agents.contains("./api/AGENTS.md"));
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(workspace_root.join(".pi/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["skills"].as_array().unwrap().len(), 2);

        #[cfg(unix)]
        let repo_skill_inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(workspace_root.join(".agents/skills/api-database"))
                .unwrap()
                .ino()
        };

        let second = refresh_with_config(&mut ws, &cfg).unwrap();
        assert_eq!(second.changed_files, 0);
        assert_eq!(ws.skills.len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(workspace_root.join(".agents/skills/api-database"))
                    .unwrap()
                    .ino(),
                repo_skill_inode
            );
        }

        fs::write(
            local.join("SKILL.md"),
            "---\nname: database\ndescription: Updated test skill\n---\n\nCheck new migrations.\n",
        )
        .unwrap();
        refresh_with_config(&mut ws, &cfg).unwrap();
        let updated =
            fs::read_to_string(workspace_root.join(".agents/skills/api-database/SKILL.md"))
                .unwrap();
        assert!(updated.contains("Check new migrations"));
    }

    #[test]
    fn pi_merge_preserves_user_entries_and_removes_old_managed_entries() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".pi")).unwrap();
        fs::write(
            tmp.path().join(".pi/settings.json"),
            r#"{"theme":"dark","skills":["/custom", "../.agents/skills/old"]}"#,
        )
        .unwrap();
        let old = vec![manifest::SkillSelection {
            name: "old".into(),
            source: LIBRARY_SOURCE.into(),
            path: PathBuf::from("/old"),
            copied: false,
        }];
        let desired = vec![manifest::SkillSelection {
            name: "new".into(),
            source: LIBRARY_SOURCE.into(),
            path: PathBuf::from("/new"),
            copied: false,
        }];
        assert!(sync_pi_settings(tmp.path(), &old, &desired).unwrap());
        let value: Value = serde_json::from_str(
            &fs::read_to_string(tmp.path().join(".pi/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(
            value["skills"],
            serde_json::json!(["/custom", "../.agents/skills/new"])
        );
    }

    #[test]
    fn copy_snapshot_is_not_rewritten_by_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let library_root = tmp.path().join("library");
        let source = skill(&library_root.join("skills"), "review", "version one");
        let mut ws = workspace(&tmp.path().join("workspace"));
        ws.skills.push(manifest::SkillSelection {
            name: "review".into(),
            source: LIBRARY_SOURCE.into(),
            path: source.clone(),
            copied: true,
        });
        let cfg = config::Config {
            library_dirs: vec![library_root],
            ..Default::default()
        };
        refresh_with_config(&mut ws, &cfg).unwrap();
        fs::write(source.join("SKILL.md"), "version two\n").unwrap();
        refresh_with_config(&mut ws, &cfg).unwrap();
        let copied = fs::read_to_string(ws.root.join(".agents/skills/review/SKILL.md")).unwrap();
        assert!(copied.contains("version one"));
    }
}
