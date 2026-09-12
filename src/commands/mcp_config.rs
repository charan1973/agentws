use anyhow::{bail, Result};

/// Print the MCP server config snippet for a given agent.
pub fn run(agent: &str) -> Result<()> {
    let bin = which_agentws().unwrap_or_else(|| "agentws".to_string());
    let snippet = match agent.to_lowercase().as_str() {
        "claude" | "claude-code" => format!(
            "# Add to ~/.claude.json (global) or .mcp.json (per-project):\n\
             {{\n  \"mcpServers\": {{\n    \"agentws\": {{\n      \"command\": \"{bin}\",\n      \"args\": [\"mcp\"]\n    }}\n  }}\n}}"
        ),
        "codex" => format!(
            "# Add to ~/.codex/config.toml:\n\
             [mcp_servers.agentws]\ncommand = \"{bin}\"\nargs = [\"mcp\"]"
        ),
        "opencode" => format!(
            "# Add to opencode.json (\"mcp\" key):\n\
             {{\n  \"mcp\": {{\n    \"agentws\": {{\n      \"type\": \"local\",\n      \"command\": [\"{bin}\", \"mcp\"]\n    }}\n  }}\n}}"
        ),
        "pi" => format!(
            "# Install the native project extension (repo tools + scope guard):\n\
             agentws integrate pi\n\
             # The generated extension invokes the agentws tool bridge via: {bin}"
        ),
        other => bail!(
            "unknown agent '{other}' (expected: claude, codex, opencode, pi)"
        ),
    };
    println!("{snippet}");
    Ok(())
}

fn which_agentws() -> Option<String> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path).find_map(|dir| {
            let candidate = dir.join("agentws");
            candidate.is_file().then(|| candidate.display().to_string())
        })
    })
}
