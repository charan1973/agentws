use agentws::manifest::{self, Workspace};
use chrono::Utc;
use std::path::Path;
use std::process::Command;

#[test]
fn fish_activation_changes_and_restores_shell_state() {
    let fish = match which("fish") {
        Some(path) => path,
        None => {
            eprintln!("skipping fish activation E2E: fish is not installed");
            return;
        }
    };

    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let story = "fish-activation";
    let root = home.join(".agentws").join(story);
    manifest::save(&Workspace {
        story: story.into(),
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

    let binary = Path::new(env!("CARGO_BIN_EXE_agentws"));
    let binary_dir = binary.parent().unwrap();
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(binary_dir.to_path_buf()).chain(std::env::split_paths(&inherited_path)),
    )
    .unwrap();

    let script = r#"
agentws init-shell fish | source
set before $PWD

agentws activate fish-activation; or exit 11
test "$PWD" = "$HOME/.agentws/fish-activation"; or exit 12
test "$AGENTWS_WORKSPACE" = "fish-activation"; or exit 13
agentws deactivate; or exit 14
test "$PWD" = "$before"; or exit 15
not set -q AGENTWS_WORKSPACE; or exit 16

agentws activate fish-activation /bin/sh -c 'test "$PWD" = "$HOME/.agentws/fish-activation" && test "$AGENTWS_WORKSPACE" = fish-activation'; or exit 17
test "$PWD" = "$before"; or exit 18
not set -q AGENTWS_WORKSPACE; or exit 19
"#;

    let output = Command::new(fish)
        .arg("--no-config")
        .arg("--command")
        .arg(script)
        .env("HOME", &home)
        .env("PATH", path)
        .env_remove("AGENTWS_WORKSPACE")
        .current_dir(&home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "fish activation E2E failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn which(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        let path = directory.join(name);
        path.is_file().then_some(path)
    })
}
