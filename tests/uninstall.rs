//! End-to-end coverage for uninstall preview, dirty-worktree preservation, and
//! mandatory typed confirmation.

use agentws::commands;
use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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

fn make_repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(&["init", "-q"], path);
    git(&["symbolic-ref", "HEAD", "refs/heads/main"], path);
    git(&["config", "user.email", "t@t.t"], path);
    git(&["config", "user.name", "t"], path);
    std::fs::write(path.join("f.txt"), "hi").unwrap();
    git(&["add", "-A"], path);
    git(&["commit", "-qm", "init"], path);
}

fn run_cli(home: &Path, args: &[&str], input: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentws"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn uninstall_previews_requires_confirmation_and_preserves_dirty() -> Result<()> {
    let home = tempfile::tempdir()?;
    let home_path = home.path().canonicalize()?;
    std::env::set_var("HOME", &home_path);
    let repos = home_path.join("repos");
    let config = home_path.join(".config/agentws");
    std::fs::create_dir_all(&config)?;
    std::fs::write(
        config.join("config.toml"),
        format!("repo_roots = [\"{}\"]\n", repos.display()),
    )?;
    for name in ["a", "b"] {
        make_repo(&repos.join(name));
    }

    commands::new::run("clean", Some(vec!["a".into()]), None)?;
    commands::new::run("dirty", Some(vec!["b".into()]), None)?;
    let workspace_root = home_path.join(".agentws");
    std::fs::write(workspace_root.join("dirty/b/u.txt"), "dirty")?;

    let preview = run_cli(&home_path, &["uninstall", "--dry-run"], "");
    assert!(preview.status.success());
    let stdout = String::from_utf8_lossy(&preview.stdout);
    assert!(stdout.contains("clean (1 worktree(s), 0 dirty)"));
    assert!(stdout.contains("dirty (1 worktree(s), 1 dirty)"));
    assert!(workspace_root.join("clean").exists());

    let rejected = run_cli(&home_path, &["uninstall", "--yes"], "y\n");
    assert!(rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("cannot bypass"));
    assert!(workspace_root.join("clean").exists());
    assert!(workspace_root.join("dirty").exists());

    let partial = run_cli(&home_path, &["uninstall"], "uninstall\n");
    assert!(partial.status.success());
    assert!(!workspace_root.join("clean").exists());
    assert!(workspace_root.join("dirty").exists());
    assert!(!workspace_root.join(".current").exists());
    assert!(config.exists(), "configuration is opt-in");

    let forced = run_cli(&home_path, &["uninstall", "--force"], "uninstall\n");
    assert!(forced.status.success());
    assert!(!workspace_root.exists());
    assert!(config.exists());

    let with_config = run_cli(
        &home_path,
        &["uninstall", "--include-config"],
        "uninstall\n",
    );
    assert!(with_config.status.success());
    assert!(!config.exists());
    Ok(())
}
