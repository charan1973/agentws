//! Interactive approval supervisor, designed to live in a small tmux pane.

use crate::{commands::expand, manifest};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::io::{self, Write};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Approve,
    Deny,
    Skip,
    Quit,
}

/// Watch a workspace for pending repo requests, or launch that watcher in a
/// dedicated tmux pane. agentws still does not launch or own the agent process.
pub fn run(story: Option<String>, tmux: bool, poll_ms: u64) -> Result<()> {
    if poll_ms == 0 {
        bail!("--poll-ms must be greater than zero");
    }

    let story = manifest::resolve_story(story)?;
    if tmux {
        launch_tmux_pane(&story, poll_ms)
    } else {
        watch(&story, Duration::from_millis(poll_ms))
    }
}

fn launch_tmux_pane(story: &str, poll_ms: u64) -> Result<()> {
    if std::env::var_os("TMUX").is_none_or(|value| value.is_empty()) {
        bail!(
            "--tmux must be run inside tmux. Start tmux first, or run \
             `agentws approvals --story {story}` in another terminal."
        );
    }

    let ws = manifest::load(story)?;
    let exe = std::env::current_exe().context("locating the agentws executable")?;
    let shell_command = watcher_shell_command(&exe, story, poll_ms);
    let output = Command::new("tmux")
        .args([
            "split-window",
            "-d",
            "-v",
            "-l",
            "10",
            "-P",
            "-F",
            "#{pane_id}",
            "-c",
        ])
        .arg(&ws.root)
        .arg(&shell_command)
        .output()
        .context("starting the tmux approval pane")?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        bail!("tmux could not create the approval pane: {}", detail.trim());
    }

    let pane = String::from_utf8_lossy(&output.stdout);
    println!(
        "approval watcher started for '{story}' in tmux pane {}.",
        pane.trim()
    );
    Ok(())
}

fn watch(story: &str, interval: Duration) -> Result<()> {
    let mut skipped = HashSet::new();
    let mut waiting_announced = false;

    set_tmux_pane_title(story);
    println!("agentws approvals: watching '{story}' (Ctrl-C to stop; q at a prompt)");

    loop {
        let ws = match manifest::load(story) {
            Ok(ws) => ws,
            Err(error) if manifest::exists(story) => {
                eprintln!("could not read workspace manifest; retrying: {error:#}");
                std::thread::sleep(interval);
                continue;
            }
            Err(error) => return Err(error),
        };
        let pending: Vec<_> = ws
            .requests
            .iter()
            .filter(|request| request.status == "pending" && !skipped.contains(&request.id))
            .cloned()
            .collect();

        if pending.is_empty() {
            if !waiting_announced {
                println!("waiting for repo requests...");
                waiting_announced = true;
            }
            std::thread::sleep(interval);
            continue;
        }

        waiting_announced = false;
        focus_tmux_pane();
        print!("\x07");
        io::stdout().flush().ok();

        for request in pending {
            match prompt(&request)? {
                Decision::Approve => {
                    if let Err(error) = expand::approve(Some(story.to_string()), request.id.clone())
                    {
                        eprintln!("could not approve '{}': {error:#}", request.repo);
                    }
                }
                Decision::Deny => {
                    if let Err(error) = expand::deny(Some(story.to_string()), request.id.clone()) {
                        eprintln!("could not deny '{}': {error:#}", request.repo);
                    }
                }
                Decision::Skip => {
                    skipped.insert(request.id);
                    println!("skipped for this watcher session.");
                }
                Decision::Quit => return Ok(()),
            }
        }
    }
}

fn prompt(request: &manifest::RepoRequest) -> Result<Decision> {
    println!();
    println!("repo request: {}", request.repo);
    println!("request id : {}", request.id);
    if let Some(reason) = request
        .reason
        .as_deref()
        .filter(|reason| !reason.is_empty())
    {
        println!("reason     : {reason}");
    }

    loop {
        print!("[a]pprove / [d]eny / [s]kip / [q]uit: ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(Decision::Quit);
        }
        if let Some(decision) = parse_decision(&line) {
            return Ok(decision);
        }
        println!("enter a, d, s, or q.");
    }
}

fn parse_decision(input: &str) -> Option<Decision> {
    match input.trim().to_ascii_lowercase().as_str() {
        "a" | "approve" | "y" | "yes" => Some(Decision::Approve),
        "d" | "deny" | "n" | "no" => Some(Decision::Deny),
        "s" | "skip" => Some(Decision::Skip),
        "q" | "quit" => Some(Decision::Quit),
        _ => None,
    }
}

fn focus_tmux_pane() {
    let Some(pane) = std::env::var_os("TMUX_PANE") else {
        return;
    };
    let _ = Command::new("tmux")
        .args(["select-pane", "-t"])
        .arg(pane)
        .status();
}

fn set_tmux_pane_title(story: &str) {
    let Some(pane) = std::env::var_os("TMUX_PANE") else {
        return;
    };
    let _ = Command::new("tmux")
        .args(["select-pane", "-t"])
        .arg(pane)
        .args(["-T", &format!("agentws approvals: {story}")])
        .status();
}

fn watcher_shell_command(exe: &std::path::Path, story: &str, poll_ms: u64) -> String {
    format!(
        "exec {} approvals --story {} --poll-ms {poll_ms}",
        shell_quote(&exe.to_string_lossy()),
        shell_quote(story)
    )
}

/// Quote one argument for tmux's shell-command string.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_approval_choices_and_aliases() {
        assert_eq!(parse_decision("a\n"), Some(Decision::Approve));
        assert_eq!(parse_decision("YES"), Some(Decision::Approve));
        assert_eq!(parse_decision("d"), Some(Decision::Deny));
        assert_eq!(parse_decision("no"), Some(Decision::Deny));
        assert_eq!(parse_decision("skip"), Some(Decision::Skip));
        assert_eq!(parse_decision("q"), Some(Decision::Quit));
        assert_eq!(parse_decision("maybe"), None);
    }

    #[test]
    fn watcher_command_shell_quotes_executable_and_story() {
        let command = watcher_shell_command(
            std::path::Path::new("/tmp/agent ws/bin/agentws"),
            "customer's-login",
            250,
        );
        assert_eq!(
            command,
            "exec '/tmp/agent ws/bin/agentws' approvals --story 'customer'\\''s-login' --poll-ms 250"
        );
    }
}
