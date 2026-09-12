//! End-to-end check that `approvals --tmux` creates a dedicated watcher pane.

use agentws::manifest;
use chrono::Utc;
use std::process::Command;

struct TmuxServer {
    socket: String,
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .status();
    }
}

#[test]
fn tmux_mode_starts_a_dedicated_watcher_pane() {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("tmux is not installed; skipping tmux integration test");
        return;
    }

    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let story = "tmux-approval-test";
    let root = home.join(".agentws").join(story);
    std::fs::create_dir_all(&root).unwrap();
    let workspace = manifest::Workspace {
        story: story.into(),
        root: root.clone(),
        created: Utc::now(),
        repos: vec![],
        requests: vec![manifest::RepoRequest {
            id: "abcd".into(),
            repo: "payments".into(),
            reason: Some("exercise the inline prompt".into()),
            status: "pending".into(),
            by: "agent".into(),
            created: Utc::now(),
            resolved: None,
        }],
        archived: false,
        skills: vec![],
        agents_md: vec![],
        setup: manifest::WorkspaceSetup::default(),
    };
    let manifest_json = serde_json::to_string_pretty(&workspace).unwrap();
    std::fs::write(root.join("workspace.json"), manifest_json).unwrap();

    let unique = format!("{}-{}", std::process::id(), Utc::now().timestamp_micros());
    let socket = format!("aw-{unique}");
    let session = format!("aw-{unique}");
    let marker = format!("aw-done-{unique}");
    let output_path = home.join("launcher-output.txt");
    let _server = TmuxServer {
        socket: socket.clone(),
    };

    let started = Command::new("tmux")
        .args(["-L", &socket, "new-session", "-d", "-s", &session])
        .env("HOME", &home)
        .status()
        .unwrap();
    assert!(started.success(), "failed to start isolated tmux server");

    let binary = env!("CARGO_BIN_EXE_agentws");
    let launch = format!(
        "HOME={} {} approvals --story {} --tmux > {} 2>&1; tmux wait-for -S {}",
        shell_quote(&home.to_string_lossy()),
        shell_quote(binary),
        shell_quote(story),
        shell_quote(&output_path.to_string_lossy()),
        shell_quote(&marker),
    );
    let sent = Command::new("tmux")
        .args([
            "-L",
            &socket,
            "send-keys",
            "-t",
            &format!("{session}:0.0"),
            &launch,
            "Enter",
        ])
        .status()
        .unwrap();
    assert!(sent.success(), "failed to run agentws inside tmux");

    let waited = Command::new("tmux")
        .args(["-L", &socket, "wait-for", &marker])
        .status()
        .unwrap();
    assert!(waited.success(), "agentws launcher did not finish");

    let panes = Command::new("tmux")
        .args([
            "-L",
            &socket,
            "list-panes",
            "-t",
            &session,
            "-F",
            "#{pane_index}:#{pane_id}",
        ])
        .output()
        .unwrap();
    assert!(panes.status.success());
    let pane_lines: Vec<_> = String::from_utf8_lossy(&panes.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        pane_lines.len(),
        2,
        "expected the original pane plus one approval watcher pane"
    );

    let launcher_output = std::fs::read_to_string(output_path).unwrap();
    assert!(
        launcher_output.contains("approval watcher started"),
        "unexpected launcher output: {launcher_output}"
    );

    let watcher_pane = pane_lines
        .iter()
        .find_map(|line| line.strip_prefix("1:"))
        .expect("watcher pane should have index 1");
    let denied = Command::new("tmux")
        .args(["-L", &socket, "send-keys", "-t", watcher_pane, "d", "Enter"])
        .status()
        .unwrap();
    assert!(denied.success(), "failed to answer the approval prompt");

    let mut status = String::new();
    for _ in 0..50 {
        if let Ok(conn) = rusqlite::Connection::open(root.join("workspace.db")) {
            if let Ok(saved) =
                conn.query_row("SELECT status FROM requests WHERE id = 'abcd'", [], |row| {
                    row.get::<_, String>(0)
                })
            {
                status = saved;
            }
            if status == "denied" {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(status, "denied", "tmux prompt did not resolve the request");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
