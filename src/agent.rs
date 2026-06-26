use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
    Pi,
}

impl AgentKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "claude-code" => Ok(AgentKind::Claude),
            "codex" => Ok(AgentKind::Codex),
            "opencode" => Ok(AgentKind::Opencode),
            "pi" => Ok(AgentKind::Pi),
            other => bail!(
                "unknown agent '{other}' (expected: claude, codex, opencode, pi)"
            ),
        }
    }

    pub fn binary(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Opencode => "opencode",
            AgentKind::Pi => "pi",
        }
    }

    pub fn as_str(self) -> &'static str {
        self.binary()
    }
}

/// Launch the agent in `root` as a foreground process (inherits the TTY).
pub fn launch(kind: AgentKind, root: &Path) -> Result<()> {
    let bin = kind.binary();
    if which(bin).is_none() {
        bail!("agent binary '{bin}' not found on PATH");
    }
    let status = Command::new(bin).current_dir(root).status()?;
    if !status.success() {
        bail!("agent '{bin}' exited with status {:?}", status.code());
    }
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
