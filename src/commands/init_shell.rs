use anyhow::Result;

/// Print a shell helper that lets you `cd` into a workspace directly.
pub fn run(shell: &str) -> Result<()> {
    let body = r#"agentws_cd() {
  [ -z "$1" ] && { echo "usage: agentws_cd <story>"; return 1; }
  cd "$(agentws open "$1")" || return 1;
}"#;
    match shell {
        "bash" | "zsh" => {
            println!("# Add to your ~/.{shell}rc:\n{body}");
        }
        "fish" => {
            println!(
                "# Add to ~/.config/fish/functions/agentws_cd.fish:\n\
                 function agentws_cd\n  cd (agentws open $argv[1])\nend"
            );
        }
        other => {
            println!("# shell '{other}': define a wrapper around `cd \"$(agentws open <story>)\"`\n{body}");
        }
    }
    Ok(())
}
