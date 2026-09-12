//! Integration test for `manifest::resolve_story` — the "run from any dir" chain.
//!
//! Lives in its OWN test binary (separate process) so the HOME / $AGENTWS_WORKSPACE
//! / cwd mutations here can't race with unit tests elsewhere.
//!
//! Priority order under test:
//!   1. explicit `--story` arg
//!   2. `$AGENTWS_WORKSPACE` env var (set by `activate`)
//!   3. cwd inside `~/.agentws/<story>`
//!   4. `~/.agentws/.current` pointer
//!   5. exactly one workspace exists
//!   6. otherwise: error

use agentws::manifest;
use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;

/// Point HOME at a throwaway tempdir so every `~/.agentws` path is sandboxed.
///
/// We canonicalize the path first: on macOS, tempdirs live under `/var/folders`,
/// a symlink to `/private/var/folders`. `resolve_story`'s cwd check compares
/// `std::env::current_dir()` (resolved) against `~/.agentws` (joined from $HOME),
/// so $HOME must already be canonical for the prefix match to work — exactly
/// like a real user's `/Users/<me>` home.
fn fresh_home() -> tempfile::TempDir {
    let td = tempfile::tempdir().unwrap();
    let real = td.path().canonicalize().unwrap();
    std::env::set_var("HOME", &real);
    td
}

/// Write a minimal valid workspace manifest for `name` under the sandboxed HOME.
fn make_story(name: &str) -> PathBuf {
    let root = std::env::var("HOME").unwrap();
    let root = PathBuf::from(root).join(".agentws").join(name);
    std::fs::create_dir_all(&root).unwrap();
    let ws = manifest::Workspace {
        story: name.into(),
        root: root.clone(),
        created: Utc::now(),
        repos: vec![],
        requests: vec![],
        archived: false,
        skills: vec![],
        agents_md: vec![],
        setup: Default::default(),
    };
    manifest::save(&ws).unwrap();
    root
}

#[test]
fn resolve_story_priority_chain() -> Result<()> {
    let home = fresh_home();
    std::env::remove_var("AGENTWS_WORKSPACE");
    std::env::set_current_dir(home.path())?; // not inside any workspace

    // step 6: nothing exists yet -> error
    assert!(manifest::resolve_story(None).is_err());

    let alpha = make_story("alpha");
    make_story("beta");

    // step 1: explicit arg wins
    assert_eq!(manifest::resolve_story(Some("beta".into()))?, "beta");

    // step 2: $AGENTWS_WORKSPACE wins when set & exists
    std::env::set_var("AGENTWS_WORKSPACE", "alpha");
    assert_eq!(manifest::resolve_story(None)?, "alpha");
    // a bogus env value (non-existent) is skipped, not trusted
    std::env::set_var("AGENTWS_WORKSPACE", "ghost");
    std::env::set_current_dir(home.path())?;
    assert!(
        manifest::resolve_story(None).is_err(),
        "bogus env + 2 workspaces + no other hint should error"
    );
    std::env::remove_var("AGENTWS_WORKSPACE");

    // step 4: .current pointer (cwd still not in a workspace)
    std::env::set_current_dir(home.path())?;
    manifest::set_current("beta")?;
    assert_eq!(manifest::resolve_story(None)?, "beta");
    manifest::clear_current_if("beta")?;

    // step 3 beats step 4: cwd inside a workspace wins over .current
    manifest::set_current("beta")?; // pointer says beta...
    std::env::set_current_dir(&alpha)?; // ...but we're sitting inside alpha
    assert_eq!(manifest::resolve_story(None)?, "alpha");
    manifest::clear_current_if("beta")?;

    // step 5: exactly one workspace -> use it. Remove alpha's manifest so only
    // beta remains.
    std::env::set_current_dir(home.path())?;
    std::fs::remove_file(alpha.join("workspace.db"))?;
    assert_eq!(manifest::resolve_story(None)?, "beta");

    Ok(())
}
