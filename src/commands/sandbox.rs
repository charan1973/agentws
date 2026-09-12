use crate::{manifest, util};
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(
    story: Option<String>,
    allow_network: bool,
    allow_read: Vec<PathBuf>,
    allow_write: Vec<PathBuf>,
    command: Vec<String>,
) -> Result<()> {
    if command.is_empty() {
        bail!("provide a command after `--`");
    }
    let story = manifest::resolve_story(story)?;
    let ws = manifest::load(&story)?;

    #[cfg(target_os = "macos")]
    return run_macos(&ws, allow_network, &allow_read, &allow_write, &command);

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (ws, allow_network, allow_read, allow_write, command);
        bail!(
            "the hard-sandbox escape hatch currently requires macOS `sandbox-exec`; \
             use the native permission controls of your agent on this platform"
        )
    }
}

#[cfg(target_os = "macos")]
fn run_macos(
    ws: &manifest::Workspace,
    allow_network: bool,
    allow_read: &[PathBuf],
    allow_write: &[PathBuf],
    command: &[String],
) -> Result<()> {
    let sandbox = Path::new("/usr/bin/sandbox-exec");
    if !sandbox.exists() {
        bail!("/usr/bin/sandbox-exec is unavailable on this Mac");
    }

    let sandbox_temp = create_sandbox_temp()?;
    let mut readable = vec![absolute(&ws.root)?, sandbox_temp.clone()];
    let mut writable = vec![absolute(&ws.root)?, sandbox_temp.clone()];
    for repo in &ws.repos {
        let git_dir = repo.origin.join(".git");
        if git_dir.exists() {
            let git_dir = absolute(&git_dir)?;
            readable.push(git_dir.clone());
            writable.push(git_dir);
        }
    }
    for path in allow_read {
        readable.push(absolute(&util::expand_tilde(path))?);
    }
    for path in allow_write {
        let path = absolute(&util::expand_tilde(path))?;
        readable.push(path.clone());
        writable.push(path);
    }
    readable.sort();
    readable.dedup();
    writable.sort();
    writable.dedup();

    let profile = sandbox_profile(readable.len(), writable.len(), allow_network);
    let mut process = Command::new(sandbox);
    process.arg("-p").arg(profile);
    for (index, path) in readable.iter().enumerate() {
        process
            .arg("-D")
            .arg(format!("READ_{index}={}", path.display()));
    }
    for (index, path) in writable.iter().enumerate() {
        process
            .arg("-D")
            .arg(format!("WRITE_{index}={}", path.display()));
    }
    process
        .arg(&command[0])
        .args(&command[1..])
        .current_dir(&ws.root)
        .env("AGENTWS_WORKSPACE", &ws.story)
        .env("TMPDIR", &sandbox_temp)
        .env("TMP", &sandbox_temp)
        .env("TEMP", &sandbox_temp);
    let result = process.status();
    let cleanup = std::fs::remove_dir_all(&sandbox_temp);
    let status = result.context("starting sandboxed command")?;
    if let Err(error) = cleanup {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error).with_context(|| {
                format!(
                    "removing sandbox temporary directory {}",
                    sandbox_temp.display()
                )
            });
        }
    }
    if !status.success() {
        return Err(anyhow!("sandboxed command exited with {status}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn create_sandbox_temp() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..16 {
        let path = base.join(format!(
            "agentws-sandbox-{}-{}",
            std::process::id(),
            crate::ops::new_id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return absolute(&path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("creating sandbox temp directory {}", path.display()))
            }
        }
    }
    bail!("could not allocate a unique sandbox temporary directory")
}

#[cfg(target_os = "macos")]
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("resolving sandbox path {}", path.display()));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(target_os = "macos")]
fn sandbox_profile(read_count: usize, write_count: usize, allow_network: bool) -> String {
    let mut profile = String::from(
        "(version 1)\n\
         (deny default)\n\
         (allow process*)\n\
         (allow signal)\n\
         (allow sysctl-read)\n\
         (allow mach-lookup)\n\
         (allow ipc-posix-shm)\n\
         (allow file-read-metadata)\n\
         (allow file-read*\n\
           (literal \"/\")\n\
           (subpath \"/System\")\n\
           (subpath \"/Library\")\n\
           (subpath \"/usr\")\n\
           (subpath \"/bin\")\n\
           (subpath \"/sbin\")\n\
           (subpath \"/opt/homebrew\")\n\
           (subpath \"/dev\")\n\
           (subpath \"/private/etc\"))\n\
         (allow file-write*\n\
           (subpath \"/dev\"))\n",
    );
    for index in 0..read_count {
        profile.push_str(&format!(
            "(allow file-read* (subpath (param \"READ_{index}\")))\n"
        ));
    }
    for index in 0..write_count {
        profile.push_str(&format!(
            "(allow file-write* (subpath (param \"WRITE_{index}\")))\n"
        ));
    }
    if allow_network {
        profile.push_str("(allow network*)\n");
    }
    profile
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn profile_is_deny_by_default_and_network_is_opt_in() {
        let offline = sandbox_profile(1, 1, false);
        assert!(offline.contains("(deny default)"));
        assert!(!offline.contains("(allow network*)"));
        assert!(offline.contains("READ_0"));
        assert!(offline.contains("WRITE_0"));

        let online = sandbox_profile(0, 0, true);
        assert!(online.contains("(allow network*)"));
    }
}
