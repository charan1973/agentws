//! Named presets for the complete `agentws new` selection set.

use crate::{config, library, manifest};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repos: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_md: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlinks: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_create: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_skills: Option<bool>,
}

pub fn load(cfg: &config::Config, name: &str) -> Result<(Template, library::Item)> {
    let item = library::resolve(cfg, library::Kind::Template, name)?;
    let text = fs::read_to_string(&item.path)
        .with_context(|| format!("reading template {}", item.path.display()))?;
    let template = toml::from_str(&text)
        .with_context(|| format!("parsing template {}", item.path.display()))?;
    Ok((template, item))
}

pub fn from_workspace(ws: &manifest::Workspace, fallback: &config::Config) -> Template {
    let bases: BTreeSet<_> = ws.repos.iter().map(|repo| repo.base.clone()).collect();
    let base = (bases.len() == 1).then(|| bases.into_iter().next().unwrap());
    let library_skills: Vec<_> = ws
        .skills
        .iter()
        .filter(|skill| skill.source == "library")
        .collect();
    let setup_initialized = ws.setup.initialized;
    Template {
        repos: Some(ws.repos.iter().map(|repo| repo.name.clone()).collect()),
        skills: Some(
            library_skills
                .iter()
                .map(|skill| skill.name.clone())
                .collect(),
        ),
        agents_md: Some(
            ws.agents_md
                .iter()
                .filter(|snippet| snippet.source == "library")
                .map(|snippet| snippet.name.clone())
                .collect(),
        ),
        base,
        symlinks: Some(if setup_initialized {
            ws.setup.symlinks.clone()
        } else {
            fallback.symlinks.clone()
        }),
        post_create: if setup_initialized {
            // Empty string is the TOML representation of an explicitly disabled
            // hook; omission means "inherit current global config".
            Some(ws.setup.post_create.clone().unwrap_or_default())
        } else {
            fallback.post_create.clone()
        },
        copy_skills: Some(library_skills.iter().any(|skill| skill.copied)),
    }
}

pub fn save(name: &str, template: &Template) -> Result<PathBuf> {
    let root = config::library_dir()?.join("templates");
    save_at(&root, name, template)
}

pub fn save_at(root: &Path, name: &str, template: &Template) -> Result<PathBuf> {
    library::validate_name(name)?;
    fs::create_dir_all(root)?;
    let path = root.join(format!("{name}.toml"));
    let text = toml::to_string_pretty(template)?;
    fs::write(&path, text).with_context(|| format!("writing template {}", path.display()))?;
    Ok(path)
}

pub fn remove(name: &str) -> Result<()> {
    library::remove(name, Some(library::Kind::Template))?;
    Ok(())
}

pub fn edit(cfg: &config::Config, name: &str) -> Result<()> {
    let (_, item) = load(cfg, name)?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .context("set $VISUAL or $EDITOR before using `agentws template edit`")?;
    let mut parts = editor.split_whitespace();
    let executable = parts.next().context("$VISUAL/$EDITOR is empty")?;
    let status = std::process::Command::new(executable)
        .args(parts)
        .arg(&item.path)
        .status()
        .with_context(|| format!("starting editor '{editor}'"))?;
    if !status.success() {
        bail!("editor exited with {status}");
    }
    // Validate after editing so a broken preset is caught immediately.
    load(cfg, name)?;
    Ok(())
}

/// Match template repository selectors. `*` matches any sequence and `?`
/// matches one character. Literal selectors retain the same errors as
/// `--repos`, while a glob matching nothing is also an actionable error.
pub fn expand_repo_selectors(selectors: &[String], names: &[String]) -> Result<Vec<String>> {
    let mut output = Vec::new();
    for selector in selectors {
        let matches: Vec<_> = names
            .iter()
            .filter(|name| wildcard_match(selector, name))
            .cloned()
            .collect();
        if matches.is_empty() {
            bail!("template repository selector '{selector}' matched no discovered repos");
        }
        for name in matches {
            if !output.contains(&name) {
                output.push(name);
            }
        }
    }
    Ok(output)
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let p = pattern.as_bytes();
    let v = value.as_bytes();
    let mut dp = vec![vec![false; v.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == b'*' {
            dp[i][0] = dp[i - 1][0];
        }
        for j in 1..=v.len() {
            dp[i][j] = match p[i - 1] {
                b'*' => dp[i - 1][j] || dp[i][j - 1],
                b'?' => dp[i - 1][j - 1],
                byte => byte == v[j - 1] && dp[i - 1][j - 1],
            };
        }
    }
    dp[p.len()][v.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn partial_template_roundtrips_without_inventing_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let template = Template {
            repos: Some(vec!["svc-*".into()]),
            skills: None,
            base: Some("main".into()),
            ..Default::default()
        };
        let path = save_at(tmp.path(), "services", &template).unwrap();
        let decoded: Template = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(decoded, template);
        assert!(decoded.agents_md.is_none());
    }

    #[test]
    fn expands_globs_in_discovery_order_and_dedupes() {
        let names = vec!["api".into(), "svc-auth".into(), "svc-web".into()];
        let expanded =
            expand_repo_selectors(&["svc-*".into(), "api".into(), "svc-web".into()], &names)
                .unwrap();
        assert_eq!(expanded, vec!["svc-auth", "svc-web", "api"]);
    }

    #[test]
    fn snapshot_records_only_library_selections() {
        let root = PathBuf::from("/tmp/demo");
        let ws = manifest::Workspace {
            story: "demo".into(),
            root: root.clone(),
            created: Utc::now(),
            repos: vec![manifest::RepoEntry {
                name: "api".into(),
                origin: root.join("origin/api"),
                worktree: root.join("api"),
                branch: "feat/demo".into(),
                base: "main".into(),
            }],
            requests: vec![],
            archived: false,
            skills: vec![
                manifest::SkillSelection {
                    name: "review".into(),
                    source: "library".into(),
                    path: root.join("library/review"),
                    copied: true,
                },
                manifest::SkillSelection {
                    name: "api-local".into(),
                    source: "repo".into(),
                    path: root.join("api/.agents/skills/local"),
                    copied: false,
                },
            ],
            agents_md: vec![],
            setup: manifest::WorkspaceSetup {
                initialized: true,
                template: None,
                symlinks: vec!["node_modules".into()],
                post_create: Some("npm ci".into()),
            },
        };
        let template = from_workspace(&ws, &config::Config::default());
        assert_eq!(template.skills.unwrap(), vec!["review"]);
        assert_eq!(template.base.as_deref(), Some("main"));
        assert_eq!(template.copy_skills, Some(true));
    }
}
