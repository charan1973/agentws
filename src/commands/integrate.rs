use anyhow::Result;

pub fn run(story: Option<String>, agents: Vec<String>, force: bool) -> Result<()> {
    crate::integrations::install(story, agents, force)
}
