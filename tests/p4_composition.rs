//! End-to-end coverage for P4 library/template selection and automatic refresh.

use agentws::{commands, config, manifest, templates};
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(args: &[&str], dir: &Path) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_skill(parent: &Path, name: &str, instruction: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Exercise {name}\n---\n\n{instruction}\n"),
    )
    .unwrap();
    path
}

fn make_repo(root: &Path, name: &str, skill_name: &str) {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    git(&["init", "-q", "-b", "main"], &path);
    git(&["config", "user.email", "p4@example.invalid"], &path);
    git(&["config", "user.name", "P4 Test"], &path);
    fs::write(path.join("README.md"), format!("# {name}\n")).unwrap();
    fs::write(path.join("AGENTS.md"), format!("Rules for {name}.\n")).unwrap();
    write_skill(
        &path.join(".agents/skills"),
        skill_name,
        &format!("Use the {name} workflow."),
    );
    git(&["add", "-A"], &path);
    git(&["commit", "-qm", "initial"], &path);
}

#[test]
fn template_new_and_lifecycle_refresh_full_composition() -> Result<()> {
    let home = tempfile::tempdir()?;
    let home = home.path().canonicalize()?;
    std::env::set_var("HOME", &home);
    std::env::remove_var("AGENTWS_WORKSPACE");

    let repos = home.join("repos");
    make_repo(&repos, "api", "database");
    make_repo(&repos, "web", "frontend");
    fs::create_dir_all(repos.join("api/node_modules"))?;
    fs::write(repos.join("api/node_modules/cache.txt"), "cached")?;

    let library = home.join(".config/agentws/library");
    write_skill(
        &library.join("skills"),
        "review",
        "Review the whole change.",
    );
    fs::create_dir_all(library.join("agents"))?;
    fs::write(
        library.join("agents/house-style.md"),
        "- Prefer small public APIs.\n",
    )?;
    templates::save_at(
        &library.join("templates"),
        "stack",
        &templates::Template {
            repos: Some(vec!["api".into()]),
            skills: Some(vec!["review".into()]),
            agents_md: Some(vec!["house-style".into()]),
            base: Some("main".into()),
            symlinks: Some(vec!["node_modules".into()]),
            post_create: Some("printf created > .agentws-hook".into()),
            copy_skills: Some(false),
        },
    )?;
    fs::create_dir_all(home.join(".config/agentws"))?;
    fs::write(
        home.join(".config/agentws/config.toml"),
        format!(
            "repo_roots = [\"{}\"]\n\n[default]\ntemplate = \"stack\"\n",
            repos.display()
        ),
    )?;

    commands::new::run_with_options(
        "p4-e2e",
        None,
        None,
        None,
        None,
        Some("stack".into()),
        false,
    )?;
    let root = home.join(".agentws/p4-e2e");
    let ws = manifest::load("p4-e2e")?;
    assert_eq!(ws.setup.template.as_deref(), Some("stack"));
    assert_eq!(ws.setup.symlinks, vec!["node_modules"]);
    assert_eq!(ws.repos.len(), 1);
    assert!(root.join("api/node_modules").is_symlink());
    assert_eq!(
        fs::read_to_string(root.join("api/.agentws-hook"))?,
        "created"
    );
    assert!(root.join(".agents/skills/review").is_symlink());
    assert!(root.join(".agents/skills/api-database").is_dir());
    assert!(root.join(".claude/skills/review").is_symlink());
    let agents = fs::read_to_string(root.join("AGENTS.md"))?;
    assert!(agents.contains("Prefer small public APIs"));
    assert!(agents.contains("./api/AGENTS.md"));
    assert_eq!(agents, fs::read_to_string(root.join("CLAUDE.md"))?);
    let pi: Value = serde_json::from_str(&fs::read_to_string(root.join(".pi/settings.json"))?)?;
    assert_eq!(pi["skills"].as_array().unwrap().len(), 2);

    // `add` automatically refreshes repo pointers, namespaced skills, and Pi.
    commands::add::run(Some("p4-e2e".into()), "web".into(), None)?;
    let added = manifest::load("p4-e2e")?;
    assert_eq!(added.repos.len(), 2);
    assert!(root.join(".agents/skills/web-frontend").is_dir());
    assert!(fs::read_to_string(root.join("AGENTS.md"))?.contains("./web/AGENTS.md"));

    // `remove` prunes only paths previously managed by agentws.
    commands::remove::run(Some("p4-e2e".into()), "web".into())?;
    assert!(!root.join(".agents/skills/web-frontend").exists());
    assert!(!root.join(".claude/skills/web-frontend").exists());
    assert!(!fs::read_to_string(root.join("AGENTS.md"))?.contains("./web/AGENTS.md"));

    // A central snippet edit is picked up by explicit refresh.
    fs::write(
        library.join("agents/house-style.md"),
        "- Prefer stable public APIs.\n",
    )?;
    commands::refresh::run(Some("p4-e2e".into()))?;
    assert!(fs::read_to_string(root.join("AGENTS.md"))?.contains("stable public APIs"));

    commands::template::save("snapshot".into(), Some("p4-e2e".into()))?;
    let cfg = config::load()?;
    let (snapshot, _) = templates::load(&cfg, "snapshot")?;
    assert_eq!(snapshot.repos.unwrap(), vec!["api"]);
    assert_eq!(snapshot.skills.unwrap(), vec!["review"]);
    assert_eq!(snapshot.agents_md.unwrap(), vec!["house-style"]);
    assert_eq!(snapshot.symlinks.unwrap(), vec!["node_modules"]);

    // The structured [default] template makes bare new non-interactive.
    commands::new::run("p4-default", None, None)?;
    let defaulted = manifest::load("p4-default")?;
    assert_eq!(defaulted.setup.template.as_deref(), Some("stack"));
    assert_eq!(defaulted.repos[0].name, "api");

    commands::delete::run(commands::delete::DeleteOptions {
        stories: vec!["p4-e2e".into(), "p4-default".into()],
        all: false,
        dry_run: false,
        force: true,
        yes: true,
    })?;
    Ok(())
}
