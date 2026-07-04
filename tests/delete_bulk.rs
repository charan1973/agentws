//! Integration tests for bulk `delete`: dirty workspaces are kept by default
//! (removed with `--force`); `--dry-run` deletes nothing; a single explicitly
//! named workspace keeps the legacy force-on-delete behavior.
//!
//! Own test binary (separate process) so the HOME mutations here can't race
//! with unit tests elsewhere — same isolation strategy as `resolve_priority.rs`.

use agentws::{commands, manifest};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Serialize these tests within the binary: each mutates the global HOME via
/// `std::env::set_var`, so parallel threads would race on it. (Cross-binary
/// isolation is already handled — each test binary is its own process.)
static HOME_LOCK: Mutex<()> = Mutex::new(());

/// Point HOME at a throwaway tempdir so every `~/.agentws` path is sandboxed.
/// Canonicalized first so the cwd-prefix check in `resolve_story` matches (macOS
/// `/tmp` → `/private/tmp` symlink).
fn fresh_home() -> tempfile::TempDir {
    let td = tempfile::tempdir().unwrap();
    let real = td.path().canonicalize().unwrap();
    std::env::set_var("HOME", &real);
    td
}

fn ws_root(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap())
        .join(".agentws")
        .join(name)
}

fn git(args: &[&str], dir: &Path) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
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

/// Write the agentws config + three git repos (rA, rB, rC) under the sandbox HOME.
fn setup(home: &Path) {
    let cfg = home.join(".config").join("agentws");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(
        cfg.join("config.toml"),
        format!("repo_roots = [\"{}\"]\n", home.join("repos").display()),
    )
    .unwrap();
    for r in ["rA", "rB", "rC"] {
        make_repo(&home.join("repos").join(r));
    }
}

#[test]
fn bulk_delete_keeps_dirty_unless_force() -> Result<()> {
    let _guard = HOME_LOCK.lock().unwrap();
    let home = fresh_home();
    setup(home.path());

    commands::new::run("clean", Some(vec!["rA".into()]), None)?;
    commands::new::run("d1", Some(vec!["rB".into()]), None)?;
    commands::new::run("d2", Some(vec!["rC".into()]), None)?;

    // dirty d1 + d2 (untracked file in their worktrees)
    std::fs::write(ws_root("d1").join("rB").join("u.txt"), "x")?;
    std::fs::write(ws_root("d2").join("rC").join("u.txt"), "x")?;

    // dry-run deletes nothing
    commands::delete::run(vec!["clean".into()], true, false, true)?;
    assert!(ws_root("clean").exists(), "dry-run must not delete anything");

    // bulk, no --force: clean deleted, dirty ones kept
    commands::delete::run(
        vec!["clean".into(), "d1".into(), "d2".into()],
        false,
        false,
        true,
    )?;
    assert!(!ws_root("clean").exists(), "clean workspace should be deleted");
    assert!(ws_root("d1").exists(), "dirty d1 must be kept without --force");
    assert!(ws_root("d2").exists(), "dirty d2 must be kept without --force");

    // bulk with --force: dirty ones now deleted
    commands::delete::run(vec!["d1".into(), "d2".into()], false, true, true)?;
    assert!(!ws_root("d1").exists(), "dirty d1 should be deleted with --force");
    assert!(!ws_root("d2").exists(), "dirty d2 should be deleted with --force");

    Ok(())
}

#[test]
fn single_delete_forces_dirty_legacy_behavior() -> Result<()> {
    let _guard = HOME_LOCK.lock().unwrap();
    // A single explicitly-named workspace keeps legacy force-on-delete: even if
    // dirty, it's deleted without --force (unchanged from pre-bulk behavior).
    let home = fresh_home();
    setup(home.path());

    commands::new::run("solo", Some(vec!["rA".into()]), None)?;
    std::fs::write(ws_root("solo").join("rA").join("u.txt"), "x")?; // dirty

    commands::delete::run(vec!["solo".into()], false, false, true)?;
    assert!(
        !ws_root("solo").exists(),
        "single dirty workspace should be deleted (legacy force)"
    );

    // sanity: the origin repo still exists (delete only drops the worktree + dir)
    assert!(
        home.path().join("repos").join("rA").exists(),
        "origin repo must remain after workspace delete"
    );
    Ok(())
}

#[test]
fn delete_reports_missing_without_bailing() -> Result<()> {
    let _guard = HOME_LOCK.lock().unwrap();
    // Mixed valid + invalid names: valid ones proceed, invalid ones are skipped.
    let home = fresh_home();
    setup(home.path());
    commands::new::run("real", Some(vec!["rA".into()]), None)?;

    // "ghost" doesn't exist; with a valid target also present it's skipped, not fatal.
    commands::delete::run(vec!["real".into(), "ghost".into()], false, false, true)?;
    assert!(!ws_root("real").exists());
    assert!(manifest::load("ghost").is_err());
    Ok(())
}
