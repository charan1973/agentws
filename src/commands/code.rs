use crate::{manifest, vscode};
use anyhow::{bail, Result};

/// Open the workspace in VS Code via a generated multi-root `.code-workspace`.
///
/// `story` is resolved with the same priority as every other command
/// (`--story` → `$AGENTWS_WORKSPACE` → cwd inside a workspace → global pointer
/// → single workspace), so a bare `agentws code` from inside an activated
/// workspace just works — no argument needed.
///
/// The `.code-workspace` is regenerated on demand if missing, so this command
/// is self-healing even if the file was deleted. If it already exists, it is
/// opened as-is (so any hand edits are respected until the next structural
/// change like `add`/`remove`).
pub fn run(story: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let ws = manifest::load(&story)?;
    if ws.archived {
        bail!(
            "workspace '{story}' is archived. Restore it first:\n  \
             agentws restore {story}"
        );
    }

    let file = vscode::workspace_file(&ws);
    if !file.exists() {
        vscode::write_workspace(&ws)?;
    }

    let status = std::process::Command::new("code").arg(&file).status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => bail!("'code' exited with a non-zero status"),
        Err(_) => bail!(
            "could not run 'code'. Make sure the VS Code shell command is on your PATH \
             (in VS Code: Command Palette → 'Shell Command: Install \"code\" command in PATH')."
        ),
    }
}
