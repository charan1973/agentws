use crate::{manifest, picker, worktree};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::io::{self, Write};

#[derive(Debug, Clone)]
pub struct DeleteOptions {
    pub stories: Vec<String>,
    pub all: bool,
    pub dry_run: bool,
    pub force: bool,
    pub yes: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceDeletePlan {
    pub name: String,
    pub workspace: manifest::Workspace,
    pub dirty_worktrees: usize,
    pub delete: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DeletePlan {
    pub entries: Vec<WorkspaceDeletePlan>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DeleteResult {
    pub deleted: Vec<String>,
    pub failures: Vec<(String, String)>,
}

/// Delete one or more workspaces. Bare `delete` opens the fuzzy picker. A
/// single explicitly named workspace preserves the original force-on-delete
/// behavior; picker, multi-name, and `--all` operations keep dirty workspaces
/// unless `--force` is supplied.
pub fn run(options: DeleteOptions) -> Result<()> {
    if options.all && !options.stories.is_empty() {
        bail!("--all cannot be combined with workspace names");
    }

    let explicit_single = !options.all && options.stories.len() == 1;
    let targets = if options.all {
        let stories = manifest::list_stories()?;
        if stories.is_empty() {
            println!("no workspaces exist.");
            return Ok(());
        }
        stories
    } else if options.stories.is_empty() {
        let all = manifest::list_stories()?;
        if all.is_empty() {
            println!("no workspaces exist.");
            return Ok(());
        }
        match picker::pick_strings(all)? {
            Some(chosen) if !chosen.is_empty() => chosen,
            _ => {
                println!("nothing selected; aborting.");
                return Ok(());
            }
        }
    } else {
        options.stories
    };

    let plan = build_plan(targets, options.force || explicit_single)?;
    print!("{}", format_plan(&plan));
    io::stdout().flush().ok();

    if options.dry_run {
        println!("\n(dry run — nothing deleted)");
        return Ok(());
    }
    if delete_count(&plan) == 0 {
        return Ok(());
    }

    let confirmed = if options.all {
        confirm_all(delete_count(&plan))?
    } else if options.yes {
        true
    } else {
        confirm_yes()?
    };
    if !confirmed {
        println!("aborted.");
        return Ok(());
    }

    let result = execute_plan(&plan);
    print_result(&result);
    if !result.failures.is_empty() {
        bail!("{} workspace deletion(s) failed", result.failures.len());
    }
    Ok(())
}

/// Non-interactive deletion path used by MCP. Preview is the default at the
/// MCP boundary; actual deletion requires `confirmed=true`. `--all` remains a
/// CLI-only operation because its mandatory typed confirmation is human-only.
pub fn run_noninteractive(
    stories: Vec<String>,
    dry_run: bool,
    force: bool,
    confirmed: bool,
) -> Result<String> {
    if stories.is_empty() {
        bail!("provide at least one workspace name");
    }
    if !dry_run && !confirmed {
        bail!("actual deletion requires confirmed=true; preview with dry_run=true first");
    }
    let plan = build_plan(stories, force)?;
    let mut output = format_plan(&plan);
    if dry_run {
        output.push_str("\n(dry run — nothing deleted)\n");
        return Ok(output);
    }
    let result = execute_plan(&plan);
    append_result(&mut output, &result);
    if !result.failures.is_empty() {
        bail!(
            "{output}\n{} workspace deletion(s) failed",
            result.failures.len()
        );
    }
    Ok(output)
}

pub(crate) fn build_plan(stories: Vec<String>, force: bool) -> Result<DeletePlan> {
    let mut entries = Vec::new();
    let mut missing = Vec::new();
    let mut seen = HashSet::new();
    for name in stories {
        if !seen.insert(name.clone()) {
            continue;
        }
        match manifest::load(&name) {
            Ok(workspace) => {
                let dirty_worktrees = workspace
                    .repos
                    .iter()
                    .filter(|repo| worktree::is_dirty(&repo.worktree))
                    .count();
                entries.push(WorkspaceDeletePlan {
                    name,
                    workspace,
                    dirty_worktrees,
                    delete: force || dirty_worktrees == 0,
                });
            }
            Err(_) => missing.push(name),
        }
    }
    if entries.is_empty() {
        if missing.len() == 1 {
            bail!("no workspace named '{}'", missing[0]);
        }
        bail!("no valid workspaces to delete");
    }
    Ok(DeletePlan { entries, missing })
}

pub(crate) fn format_plan(plan: &DeletePlan) -> String {
    let mut output = String::new();
    for missing in &plan.missing {
        output.push_str(&format!("! no workspace named '{missing}' (skipped)\n"));
    }

    let deleting: Vec<_> = plan.entries.iter().filter(|entry| entry.delete).collect();
    let kept: Vec<_> = plan.entries.iter().filter(|entry| !entry.delete).collect();
    output.push('\n');
    if deleting.is_empty() {
        output.push_str("nothing to delete.\n");
    } else {
        output.push_str(&format!(
            "will delete {} workspace(s) — branches are kept:\n",
            deleting.len()
        ));
        for entry in deleting {
            output.push_str(&format_workspace(entry, ""));
        }
    }
    if !kept.is_empty() {
        output.push_str(&format!(
            "keeping {} dirty workspace(s) (pass --force to delete):\n",
            kept.len()
        ));
        for entry in kept {
            output.push_str(&format_workspace(entry, "  [kept]"));
        }
    }
    output
}

fn format_workspace(entry: &WorkspaceDeletePlan, suffix: &str) -> String {
    format!(
        "  - {} ({} worktree(s), {} dirty){}\n",
        entry.name,
        entry.workspace.repos.len(),
        entry.dirty_worktrees,
        suffix
    )
}

pub(crate) fn delete_count(plan: &DeletePlan) -> usize {
    plan.entries.iter().filter(|entry| entry.delete).count()
}

pub(crate) fn execute_plan(plan: &DeletePlan) -> DeleteResult {
    let mut result = DeleteResult::default();
    for entry in plan.entries.iter().filter(|entry| entry.delete) {
        match delete_workspace(&entry.workspace) {
            Ok(()) => result.deleted.push(entry.name.clone()),
            Err(error) => result
                .failures
                .push((entry.name.clone(), format!("{error:#}"))),
        }
    }
    result
}

fn print_result(result: &DeleteResult) {
    let mut output = String::new();
    append_result(&mut output, result);
    print!("{output}");
}

fn append_result(output: &mut String, result: &DeleteResult) {
    for name in &result.deleted {
        output.push_str(&format!("deleted workspace '{name}'.\n"));
    }
    for (name, error) in &result.failures {
        output.push_str(&format!("! could not delete workspace '{name}': {error}\n"));
    }
}

fn confirm_yes() -> Result<bool> {
    print!("proceed? [y/N] ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().eq_ignore_ascii_case("y"))
}

fn confirm_all(count: usize) -> Result<bool> {
    print!("type {count} to confirm deleting every eligible workspace: ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim() == count.to_string())
}

/// Remove every registered worktree, then the workspace directory and active
/// pointer. The directory is retained if any worktree removal fails, avoiding
/// a second destructive fallback that could discard files Git refused to
/// remove.
pub(crate) fn delete_workspace(ws: &manifest::Workspace) -> Result<()> {
    let mut failures = Vec::new();
    for repo in &ws.repos {
        if let Err(error) = worktree::remove_worktree(&repo.origin, &repo.worktree) {
            failures.push(format!("{}: {error:#}", repo.name));
        }
    }
    if !failures.is_empty() {
        bail!("worktree removal failed: {}", failures.join("; "));
    }
    if ws.root.exists() {
        std::fs::remove_dir_all(&ws.root)
            .with_context(|| format!("removing workspace directory {}", ws.root.display()))?;
    }
    manifest::clear_current_if(&ws.story)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_includes_per_workspace_dirty_counts() {
        let plan = DeletePlan {
            entries: vec![WorkspaceDeletePlan {
                name: "demo".into(),
                workspace: manifest::Workspace {
                    story: "demo".into(),
                    root: "/tmp/demo".into(),
                    created: chrono::Utc::now(),
                    repos: Vec::new(),
                    requests: Vec::new(),
                    archived: false,
                    skills: Vec::new(),
                    agents_md: Vec::new(),
                    setup: Default::default(),
                },
                dirty_worktrees: 2,
                delete: false,
            }],
            missing: Vec::new(),
        };
        let rendered = format_plan(&plan);
        assert!(rendered.contains("0 worktree(s), 2 dirty"));
        assert!(rendered.contains("[kept]"));
    }

    #[test]
    fn failed_git_removal_retains_workspace_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let worktree = root.join("repo");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join("important.txt"), "keep").unwrap();
        let workspace = manifest::Workspace {
            story: "demo".into(),
            root: root.clone(),
            created: chrono::Utc::now(),
            repos: vec![manifest::RepoEntry {
                name: "repo".into(),
                origin: temp.path().join("missing-origin"),
                worktree,
                branch: "feat/demo".into(),
                base: "main".into(),
            }],
            requests: Vec::new(),
            archived: false,
            skills: Vec::new(),
            agents_md: Vec::new(),
            setup: Default::default(),
        };
        assert!(delete_workspace(&workspace).is_err());
        assert!(root.join("repo/important.txt").exists());
    }
}
