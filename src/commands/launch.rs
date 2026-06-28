use crate::{agent, config, manifest};
use anyhow::{bail, Result};

/// (Re)launch the agent for an existing workspace.
pub fn run(story: Option<String>, agent_override: Option<String>) -> Result<()> {
    let story = manifest::resolve_story(story)?;
    let mut ws = manifest::load(&story)?;

    let kind = match agent_override.as_deref() {
        Some(a) => agent::AgentKind::parse(a)?,
        None => match ws.agent.as_ref().and_then(|a| agent::AgentKind::parse(&a.kind).ok()) {
            Some(k) => k,
            None => match config::load()?.default_agent.as_deref() {
                Some(a) => agent::AgentKind::parse(a)?,
                None => agent::AgentKind::Claude,
            },
        },
    };

    if ws.archived {
        bail!(
            "workspace '{story}' is archived. Run `agentws restore {story}` first."
        );
    }

    ws.agent = Some(manifest::AgentRef {
        kind: kind.as_str().to_string(),
        pid: None,
    });
    manifest::save(&ws)?;

    println!("launching {} in {} ...\n", kind.as_str(), ws.root.display());
    agent::launch(kind, &ws.root)?;
    Ok(())
}
