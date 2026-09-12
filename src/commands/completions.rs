use anyhow::{anyhow, Result};
use clap::CommandFactory;
use std::io;

pub fn run(shell: &str) -> Result<()> {
    let shell: clap_complete::Shell = shell.parse().map_err(|_| {
        anyhow!("unknown shell '{shell}' (try: bash, zsh, fish, elvish, powershell)")
    })?;
    let mut cmd = crate::cli::Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(shell, &mut cmd, name, &mut io::stdout());
    Ok(())
}
