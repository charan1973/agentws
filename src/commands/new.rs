use crate::{config, discovery, manifest, picker, worktree};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::Path;

pub fn run(story: &str, repos: Option<Vec<String>>, base: Option<String>) -> Result<()> {
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

    if story.contains('/') || story.chars().any(char::is_whitespace) {
        bail!("story name must not contain spaces or slashes (got '{story}')");
    }

    if manifest::exists(story) {
        bail!(
            "workspace '{story}' already exists. Use a different name, or \
             `agentws delete {story}` first."
        );
    }

    let roots = cfg.repo_roots_expanded();
    let all = discovery::discover(&roots);
    if all.is_empty() {
        bail!("no git repositories found under {roots:?}. Check repo_roots in config.");
    }

    let selected: Vec<discovery::Repo> = match repos {
        Some(names) => select_by_name(&all, &names)?,
        None => match picker::pick(all)? {
            Some(chosen) => chosen,
            None => {
                println!("no repos selected; aborting.");
                return Ok(());
            }
        },
    };
    if selected.is_empty() {
        println!("no repos selected; aborting.");
        return Ok(());
    }

    let root = config::workspace_dir(story)?;
    std::fs::create_dir_all(&root)?;

    let branch = format!("feat/{story}");
    let mut entries = Vec::new();
    println!("creating workspace '{story}' ...\n");
    for repo in &selected {
        let dest = root.join(&repo.name);
        let base_branch = match &base {
            Some(b) => b.clone(),
            None => worktree::default_branch(&repo.path).unwrap_or_else(|e| {
                eprintln!(
                    "warning: could not determine default branch for {name}: {e} — \
                     falling back to 'main'",
                    name = repo.name
                );
                "main".to_string()
            }),
        };
        worktree::add_worktree(&repo.path, &dest, &branch, &base_branch)
            .with_context(|| format!("creating worktree for {}", repo.name))?;
        crate::ops::apply_symlinks(&repo.path, &dest);
        crate::ops::run_post_create(&dest);
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

    write_agents_stub(&root, story, &entries)?;

    let ws = manifest::Workspace {
        story: story.to_string(),
        root: root.clone(),
        created: Utc::now(),
        repos: entries,
        requests: Vec::new(),
        archived: false,
    };
    manifest::save(&ws)?;
    manifest::set_current(story)?;
    crate::vscode::write_workspace(&ws)?;

    println!("\nworkspace '{story}' ready at {}\n", root.display());
    println!("activate it, then run any agent (pi / claude / codex / opencode):");
    println!("  agentws activate {story}          # cd in + set $AGENTWS_WORKSPACE");
    println!("  agentws activate {story} pi        # run pi in the workspace (one-shot)");
    println!();
    println!("the agent runs natively and resumes itself (e.g. `pi -c`) — keyed by this dir.");
    Ok(())
}

fn select_by_name(
    all: &[discovery::Repo],
    names: &[String],
) -> Result<Vec<discovery::Repo>> {
    let mut out = Vec::new();
    for n in names {
        let n = n.trim();
        if n.is_empty() {
            continue;
        }
        let found = all
            .iter()
            .find(|r| r.name == n)
            .cloned()
            .with_context(|| format!("repo '{n}' not found among discovered repos"))?;
        out.push(found);
    }
    Ok(out)
}

fn write_agents_stub(
    root: &Path,
    story: &str,
    entries: &[manifest::RepoEntry],
) -> Result<()> {
    let list = entries
        .iter()
        .map(|e| format!("- `{}`", e.name))
        .collect::<Vec<_>>()
        .join("\n");
    let content = format!(
        "# Workspace: {story}\n\n\
         This directory is an `agentws` workspace. It contains git worktrees of only the\n\
         repositories selected for this story:\n\n\
         {list}\n\n\
         ## Scope\n\n\
         Operate within this directory. Do **not** grep or read files outside this\n\
         workspace root unless explicitly asked — other repositories are intentionally\n\
         out of scope to keep context clean.\n\n\
         ## Need another repository?\n\n\
         If you discover this task requires a repository that isn't here yet, request it\n\
         via the `agentws` MCP tool (`request_repo`) or ask the user to run\n\
         `agentws add <repo>`. Do not attempt to read or clone it yourself.\n"
    );
    std::fs::write(root.join("AGENTS.md"), content)?;
    Ok(())
}
