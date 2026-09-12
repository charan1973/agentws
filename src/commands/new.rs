use crate::{composition, config, discovery, library, manifest, picker, templates, worktree};
use anyhow::{bail, Context, Result};
use chrono::Utc;

#[allow(clippy::too_many_arguments)]
pub fn run_with_options(
    story: &str,
    repos: Option<Vec<String>>,
    base: Option<String>,
    skills: Option<Vec<String>>,
    agents_md: Option<Vec<String>>,
    template: Option<String>,
    copy: bool,
) -> Result<()> {
    let cfg = config::load()?;
    if cfg.repo_roots_expanded().is_empty() {
        let path = config::ensure_example()?;
        bail!(
            "no repo_roots configured.\n\
             Add your repo roots to {cfg_path} and retry, e.g.:\n  \
             repo_roots = [\"~/work\"]",
            cfg_path = path.display()
        );
    }

    validate_story(story)?;
    if manifest::exists(story) {
        bail!(
            "workspace '{story}' already exists. Use a different name, or \
             `agentws delete {story}` first."
        );
    }

    let bare = repos.is_none()
        && base.is_none()
        && skills.is_none()
        && agents_md.is_none()
        && template.is_none()
        && !copy;
    let template_name = template.or_else(|| {
        if bare {
            cfg.effective_default_template().map(str::to_string)
        } else {
            None
        }
    });
    let preset = template_name
        .as_deref()
        .map(|name| templates::load(&cfg, name).map(|(template, _)| template))
        .transpose()?
        .unwrap_or_default();

    let roots = cfg.repo_roots_expanded();
    let all = discovery::discover(&roots);
    if all.is_empty() {
        bail!("no git repositories found under {roots:?}. Check repo_roots in config.");
    }

    let selected_repos = select_repos(&all, repos, preset.repos.as_deref())?;
    if selected_repos.is_empty() {
        println!("no repos selected; aborting.");
        return Ok(());
    }
    let selected_skills = select_library_items(
        &cfg,
        library::Kind::Skill,
        skills,
        preset.skills.as_deref(),
        "skills",
        "Skills",
    )?;
    let selected_snippets = select_library_items(
        &cfg,
        library::Kind::AgentsMd,
        agents_md,
        preset.agents_md.as_deref(),
        "AGENTS.md snippets",
        "AGENTS.md snippets",
    )?;

    let base = base.or(preset.base);
    let setup = manifest::WorkspaceSetup {
        initialized: true,
        template: template_name,
        symlinks: preset.symlinks.unwrap_or_else(|| cfg.symlinks.clone()),
        post_create: preset.post_create.or_else(|| cfg.post_create.clone()),
    };
    let copy_skills = copy || preset.copy_skills.unwrap_or(false);
    let root = config::workspace_dir(story)?;
    std::fs::create_dir_all(&root)?;

    let branch = format!("feat/{story}");
    let mut entries = Vec::new();
    println!("creating workspace '{story}' ...\n");
    for repo in &selected_repos {
        let dest = root.join(&repo.name);
        let base_branch = match &base {
            Some(branch) => branch.clone(),
            None => worktree::default_branch(&repo.path).unwrap_or_else(|error| {
                eprintln!(
                    "warning: could not determine default branch for {name}: {error} — \
                     falling back to 'main'",
                    name = repo.name
                );
                "main".to_string()
            }),
        };
        worktree::add_worktree(&repo.path, &dest, &branch, &base_branch)
            .with_context(|| format!("creating worktree for {}", repo.name))?;
        crate::ops::apply_symlinks_with_patterns(&repo.path, &dest, &setup.symlinks);
        crate::ops::run_post_create_command(&dest, setup.post_create.as_deref());
        println!(
            "  + {name:<20} {branch}  (base {base_branch})  -> {dest}",
            name = repo.name,
            dest = dest.display()
        );
        entries.push(manifest::RepoEntry {
            name: repo.name.clone(),
            origin: repo.path.clone(),
            worktree: dest,
            branch: branch.clone(),
            base: base_branch,
        });
    }

    let mut ws = manifest::Workspace {
        story: story.to_string(),
        root: root.clone(),
        created: Utc::now(),
        repos: entries,
        requests: Vec::new(),
        archived: false,
        skills: selected_skills
            .into_iter()
            .map(|item| manifest::SkillSelection {
                name: item.name,
                source: "library".into(),
                path: item.path,
                copied: copy_skills,
            })
            .collect(),
        agents_md: selected_snippets
            .into_iter()
            .map(|item| manifest::AgentsMdSelection {
                name: item.name,
                source: "library".into(),
                path: item.path,
            })
            .collect(),
        setup,
    };
    let composition = composition::refresh_with_config(&mut ws, &cfg)?;
    manifest::save(&ws)?;
    manifest::set_current(story)?;
    crate::vscode::write_workspace(&ws)?;
    let wired = crate::ops::wire_env(&ws)?;
    if wired > 0 {
        println!("  ~ rewired {wired} cross-repo env file(s)");
    }
    println!(
        "  ~ composed {} shared + {} repository skill(s), {} guidance snippet(s)",
        composition.library_skills, composition.repo_skills, composition.snippets
    );

    println!("\nworkspace '{story}' ready at {}\n", root.display());
    println!("activate it, then run any agent (pi / claude / codex / opencode):");
    println!("  agentws activate {story}          # cd in + set $AGENTWS_WORKSPACE");
    println!("  agentws activate {story} pi        # run pi in the workspace (one-shot)");
    println!();
    println!("Pi will ask you to trust the project before loading its project settings/skills.");
    Ok(())
}

/// Backward-compatible programmatic entry point used by existing callers.
pub fn run(story: &str, repos: Option<Vec<String>>, base: Option<String>) -> Result<()> {
    run_with_options(story, repos, base, None, None, None, false)
}

fn validate_story(story: &str) -> Result<()> {
    if story.is_empty()
        || story.contains('/')
        || story.contains('\\')
        || story.chars().any(char::is_whitespace)
        || story == "."
        || story == ".."
    {
        bail!("story name must not contain spaces or path separators (got '{story}')");
    }
    Ok(())
}

fn select_repos(
    all: &[discovery::Repo],
    explicit: Option<Vec<String>>,
    from_template: Option<&[String]>,
) -> Result<Vec<discovery::Repo>> {
    if let Some(names) = explicit {
        return select_by_name(all, &names);
    }
    if let Some(selectors) = from_template {
        let names = all.iter().map(|repo| repo.name.clone()).collect::<Vec<_>>();
        let expanded = templates::expand_repo_selectors(selectors, &names)?;
        return select_by_name(all, &expanded);
    }
    match picker::pick(all.to_vec())? {
        Some(chosen) => Ok(chosen),
        None => Ok(Vec::new()),
    }
}

fn select_library_items(
    cfg: &config::Config,
    kind: library::Kind,
    explicit: Option<Vec<String>>,
    from_template: Option<&[String]>,
    noun: &str,
    title: &str,
) -> Result<Vec<library::Item>> {
    if let Some(names) = explicit {
        return resolve_items(cfg, kind, &names);
    }
    if let Some(names) = from_template {
        return resolve_items(cfg, kind, names);
    }
    let available = library::discover_kind(cfg, kind)?;
    if available.is_empty() {
        return Ok(Vec::new());
    }
    Ok(picker::pick_items(available, noun, title)?.unwrap_or_default())
}

fn resolve_items(
    cfg: &config::Config,
    kind: library::Kind,
    names: &[String],
) -> Result<Vec<library::Item>> {
    let mut output = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty() || output.iter().any(|item: &library::Item| item.name == name) {
            continue;
        }
        output.push(library::resolve(cfg, kind, name)?);
    }
    Ok(output)
}

fn select_by_name(all: &[discovery::Repo], names: &[String]) -> Result<Vec<discovery::Repo>> {
    let mut output = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty()
            || output
                .iter()
                .any(|repo: &discovery::Repo| repo.name == name)
        {
            continue;
        }
        let found = all
            .iter()
            .find(|repo| repo.name == name)
            .cloned()
            .with_context(|| format!("repo '{name}' not found among discovered repos"))?;
        output.push(found);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_story_as_a_single_safe_component() {
        assert!(validate_story("PROJ-42").is_ok());
        assert!(validate_story("../escape").is_err());
        assert!(validate_story("two words").is_err());
        assert!(validate_story("").is_err());
    }
}
