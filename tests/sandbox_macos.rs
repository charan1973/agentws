#![cfg(target_os = "macos")]

use agentws::{commands, manifest};
use chrono::Utc;

#[test]
fn hard_sandbox_allows_workspace_io_and_blocks_outside_io() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("AGENTWS_WORKSPACE");

    let root = home.join(".agentws/hard-sandbox-test");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("inside.txt"), "inside").unwrap();
    let outside = home.join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    manifest::save(&manifest::Workspace {
        story: "hard-sandbox-test".into(),
        root: root.clone(),
        created: Utc::now(),
        repos: vec![],
        requests: vec![],
        archived: false,
        skills: vec![],
        agents_md: vec![],
        setup: Default::default(),
    })
    .unwrap();

    commands::sandbox::run(
        Some("hard-sandbox-test".into()),
        false,
        vec![],
        vec![],
        vec!["/bin/cat".into(), "inside.txt".into()],
    )
    .expect("workspace file should be readable");

    commands::sandbox::run(
        Some("hard-sandbox-test".into()),
        false,
        vec![],
        vec![],
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf writable > created.txt".into(),
        ],
    )
    .expect("workspace should be writable");
    assert_eq!(
        std::fs::read_to_string(root.join("created.txt")).unwrap(),
        "writable"
    );

    commands::sandbox::run(
        Some("hard-sandbox-test".into()),
        false,
        vec![],
        vec![],
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "test -d \"$TMPDIR\" && touch \"$TMPDIR/probe\"".into(),
        ],
    )
    .expect("private sandbox temp directory should be writable");

    let denied = commands::sandbox::run(
        Some("hard-sandbox-test".into()),
        false,
        vec![],
        vec![],
        vec!["/bin/cat".into(), outside.to_string_lossy().into_owned()],
    );
    assert!(denied.is_err(), "outside file read should be denied");

    let denied = commands::sandbox::run(
        Some("hard-sandbox-test".into()),
        false,
        vec![],
        vec![],
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf escaped > \"$1\"".into(),
            "agentws-sandbox-test".into(),
            outside.to_string_lossy().into_owned(),
        ],
    );
    assert!(denied.is_err(), "outside file write should be denied");
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "outside");
}
