use crate::{commands::delete, config, manifest};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct UninstallOptions {
    pub dry_run: bool,
    pub force: bool,
    pub include_config: bool,
    pub include_binary: bool,
    /// Deliberately does not bypass the mandatory typed confirmation.
    pub yes: bool,
}

#[derive(Debug)]
struct UninstallPlan {
    workspaces: delete::DeletePlan,
    workspace_root: PathBuf,
    current_pointer: Option<PathBuf>,
    remove_workspace_root: bool,
    config_dir: Option<PathBuf>,
    binary: Option<PathBuf>,
}

pub fn run(options: UninstallOptions) -> Result<()> {
    let workspace_root = config::workspaces_root()?;
    let config_dir = options
        .include_config
        .then(config::config_dir)
        .transpose()?;
    let binary = options
        .include_binary
        .then(std::env::current_exe)
        .transpose()
        .context("locating the running agentws executable")?;
    let plan = build_plan(workspace_root, config_dir, binary, options.force)?;
    println!("{}", format_plan(&plan));

    if options.dry_run {
        println!("(dry run — nothing removed)");
        return Ok(());
    }
    if !has_removals(&plan) {
        println!("nothing to remove.");
        return Ok(());
    }

    if options.yes {
        println!("note: --yes cannot bypass uninstall's mandatory confirmation.");
    }
    print!("type 'uninstall' to confirm: ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    if !confirmation_matches(&line) {
        println!("aborted.");
        return Ok(());
    }

    execute(plan)?;
    println!("agentws uninstall complete.");
    Ok(())
}

fn build_plan(
    workspace_root: PathBuf,
    config_dir: Option<PathBuf>,
    binary: Option<PathBuf>,
    force: bool,
) -> Result<UninstallPlan> {
    validate_named_dir(&workspace_root, ".agentws")?;
    if let Some(path) = &config_dir {
        validate_named_dir(path, "agentws")?;
    }
    if let Some(path) = &binary {
        validate_binary(path)?;
    }

    let stories = manifest::list_stories()?;
    let workspaces = if stories.is_empty() {
        delete::DeletePlan {
            entries: Vec::new(),
            missing: Vec::new(),
        }
    } else {
        delete::build_plan(stories, force)?
    };
    let current = workspace_root.join(".current");
    let current_pointer = current.exists().then_some(current);
    let remove_workspace_root = root_is_removable_after(&workspace_root, &workspaces)?;

    Ok(UninstallPlan {
        workspaces,
        workspace_root,
        current_pointer,
        remove_workspace_root,
        config_dir: config_dir.filter(|path| path.exists() || path.is_symlink()),
        binary: binary.filter(|path| path.exists() || path.is_symlink()),
    })
}

fn format_plan(plan: &UninstallPlan) -> String {
    let mut output = String::from("agentws uninstall plan:\n");
    if plan.workspaces.entries.is_empty() {
        output.push_str("\nno registered workspaces.\n");
    } else {
        output.push_str(&delete::format_plan(&plan.workspaces));
    }
    output.push_str("\nadditional paths:\n");
    let mut any = false;
    if let Some(path) = &plan.current_pointer {
        output.push_str(&format!("  - remove active pointer {}\n", path.display()));
        any = true;
    }
    if plan.remove_workspace_root {
        output.push_str(&format!(
            "  - remove workspace root {} after it is empty\n",
            plan.workspace_root.display()
        ));
        any = true;
    }
    if let Some(path) = &plan.config_dir {
        output.push_str(&format!(
            "  - remove configuration and library {}\n",
            path.display()
        ));
        any = true;
    }
    if let Some(path) = &plan.binary {
        output.push_str(&format!("  - remove executable {}\n", path.display()));
        any = true;
    }
    if !any {
        output.push_str("  (none)\n");
    }
    if !plan.remove_workspace_root && plan.workspace_root.exists() {
        output.push_str(&format!(
            "  - keep workspace root {} because content will remain\n",
            plan.workspace_root.display()
        ));
    }
    output
}

fn has_removals(plan: &UninstallPlan) -> bool {
    delete::delete_count(&plan.workspaces) > 0
        || plan.current_pointer.is_some()
        || plan.remove_workspace_root
        || plan.config_dir.is_some()
        || plan.binary.is_some()
}

fn execute(plan: UninstallPlan) -> Result<()> {
    let result = delete::execute_plan(&plan.workspaces);
    for name in &result.deleted {
        println!("deleted workspace '{name}'.");
    }
    if !result.failures.is_empty() {
        let details = result
            .failures
            .iter()
            .map(|(name, error)| format!("{name}: {error}"))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("workspace cleanup failed; configuration and executable were kept: {details}");
    }

    if let Some(path) = plan.current_pointer {
        if path.exists() || path.is_symlink() {
            fs::remove_file(&path)
                .with_context(|| format!("removing active pointer {}", path.display()))?;
        }
    }
    if plan.remove_workspace_root && plan.workspace_root.exists() {
        fs::remove_dir(&plan.workspace_root).with_context(|| {
            format!(
                "removing empty workspace root {}",
                plan.workspace_root.display()
            )
        })?;
    }
    if let Some(path) = plan.config_dir {
        remove_file_or_dir(&path)
            .with_context(|| format!("removing configuration {}", path.display()))?;
    }
    if let Some(path) = plan.binary {
        if path.exists() || path.is_symlink() {
            fs::remove_file(&path)
                .with_context(|| format!("removing executable {}", path.display()))?;
        }
    }
    Ok(())
}

fn root_is_removable_after(root: &Path, plan: &delete::DeletePlan) -> Result<bool> {
    if !root.exists() {
        return Ok(false);
    }
    let deleted_roots: HashSet<_> = plan
        .entries
        .iter()
        .filter(|entry| entry.delete)
        .map(|entry| entry.workspace.root.clone())
        .collect();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name == ".current") || deleted_roots.contains(&path)
        {
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn remove_file_or_dir(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn validate_named_dir(path: &Path, expected_name: &str) -> Result<()> {
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name)
        || path.parent().is_none()
    {
        bail!(
            "refusing unsafe uninstall directory target {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_binary(path: &Path) -> Result<()> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        bail!("refusing unsafe executable target {}", path.display());
    };
    if name != "agentws" && name != "agentws.exe" {
        bail!(
            "refusing to remove executable not named agentws: {}",
            path.display()
        );
    }
    Ok(())
}

fn confirmation_matches(input: &str) -> bool {
    input.trim() == "uninstall"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_is_exact() {
        assert!(confirmation_matches("uninstall\n"));
        assert!(!confirmation_matches("y"));
        assert!(!confirmation_matches("UNINSTALL"));
    }

    #[test]
    fn root_removal_accounts_for_preserved_content() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(".agentws");
        fs::create_dir_all(root.join("kept")).unwrap();
        fs::write(root.join(".current"), "kept").unwrap();
        let plan = delete::DeletePlan {
            entries: Vec::new(),
            missing: Vec::new(),
        };
        assert!(!root_is_removable_after(&root, &plan).unwrap());
        fs::remove_dir(root.join("kept")).unwrap();
        assert!(root_is_removable_after(&root, &plan).unwrap());
    }

    #[test]
    fn execution_removes_only_explicit_config_and_binary_targets() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("agentws");
        let binary = temp.path().join("bin/agentws");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(config_dir.join("config.toml"), "").unwrap();
        fs::write(&binary, "binary").unwrap();
        let plan = UninstallPlan {
            workspaces: delete::DeletePlan {
                entries: Vec::new(),
                missing: Vec::new(),
            },
            workspace_root: temp.path().join(".agentws"),
            current_pointer: None,
            remove_workspace_root: false,
            config_dir: Some(config_dir.clone()),
            binary: Some(binary.clone()),
        };
        execute(plan).unwrap();
        assert!(!config_dir.exists());
        assert!(!binary.exists());
    }
}
