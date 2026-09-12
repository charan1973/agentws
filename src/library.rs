//! Reusable skills, AGENTS.md snippets, and workspace templates.

use crate::{config, picker::Pickable};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Skill,
    AgentsMd,
    Template,
}

impl Kind {
    pub fn directory(self) -> &'static str {
        match self {
            Self::Skill => "skills",
            Self::AgentsMd => "agents",
            Self::Template => "templates",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::AgentsMd => "agents-md",
            Self::Template => "template",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "skill" | "skills" => Ok(Self::Skill),
            "agents" | "agents-md" | "snippet" | "snippets" => Ok(Self::AgentsMd),
            "template" | "templates" => Ok(Self::Template),
            _ => bail!("unknown library kind '{value}' (use skill, agents-md, or template)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub kind: Kind,
    pub path: PathBuf,
    pub library_root: PathBuf,
}

impl Pickable for Item {
    fn key(&self) -> String {
        format!("{}:{}", self.kind.label(), self.name)
    }

    fn label(&self) -> String {
        self.name.clone()
    }

    fn detail(&self) -> String {
        self.path.display().to_string()
    }
}

/// Discover effective library entries. The built-in library has highest
/// precedence, followed by `library_dirs` in configuration order. Duplicate
/// kind/name entries in lower-priority roots are intentionally shadowed.
pub fn discover(cfg: &config::Config) -> Result<Vec<Item>> {
    discover_roots(&cfg.library_roots_expanded()?)
}

pub fn discover_kind(cfg: &config::Config, kind: Kind) -> Result<Vec<Item>> {
    Ok(discover(cfg)?
        .into_iter()
        .filter(|item| item.kind == kind)
        .collect())
}

pub fn resolve(cfg: &config::Config, kind: Kind, name: &str) -> Result<Item> {
    validate_name(name)?;
    for root in cfg.library_roots_expanded()? {
        let path = match kind {
            Kind::Skill => root.join("skills").join(name),
            Kind::AgentsMd => root.join("agents").join(format!("{name}.md")),
            Kind::Template => root.join("templates").join(format!("{name}.toml")),
        };
        if !path.exists() && !path.is_symlink() {
            continue;
        }
        match kind {
            Kind::Skill => validate_skill(&path, name)?,
            _ if !path.is_file() => {
                bail!(
                    "{} '{}' is not a file at {}",
                    kind.label(),
                    name,
                    path.display()
                )
            }
            _ => {}
        }
        return Ok(Item {
            name: name.to_string(),
            kind,
            path: path.canonicalize().unwrap_or(path),
            library_root: root,
        });
    }
    Err(anyhow!(
        "no {} named '{name}' in the agentws library",
        kind.label()
    ))
}

pub fn discover_roots(roots: &[PathBuf]) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        for kind in [Kind::Skill, Kind::AgentsMd, Kind::Template] {
            let dir = root.join(kind.directory());
            let mut paths = match fs::read_dir(&dir) {
                Ok(entries) => entries
                    .collect::<std::io::Result<Vec<_>>>()?
                    .into_iter()
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("reading library directory {}", dir.display()))
                }
            };
            paths.sort();
            for path in paths {
                let Some(name) = item_name(kind, &path) else {
                    continue;
                };
                if seen.contains(&(kind, name.clone())) {
                    continue;
                }
                validate_name(&name)
                    .with_context(|| format!("invalid item at {}", path.display()))?;
                match kind {
                    Kind::Skill => validate_skill(&path, &name)?,
                    Kind::AgentsMd if !path.is_file() => continue,
                    Kind::Template if !path.is_file() => continue,
                    _ => {}
                }
                seen.insert((kind, name.clone()));
                items.push(Item {
                    name,
                    kind,
                    path: path.canonicalize().unwrap_or(path),
                    library_root: root.clone(),
                });
            }
        }
    }
    items.sort_by(|a, b| {
        kind_order(a.kind)
            .cmp(&kind_order(b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(items)
}

fn kind_order(kind: Kind) -> u8 {
    match kind {
        Kind::Skill => 0,
        Kind::AgentsMd => 1,
        Kind::Template => 2,
    }
}

fn item_name(kind: Kind, path: &Path) -> Option<String> {
    match kind {
        Kind::Skill => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Kind::AgentsMd => (path.extension()?.to_str()? == "md")
            .then(|| path.file_stem().unwrap().to_string_lossy().into_owned()),
        Kind::Template => (path.extension()?.to_str()? == "toml")
            .then(|| path.file_stem().unwrap().to_string_lossy().into_owned()),
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("'{name}' is not a safe library name");
    }
    Ok(())
}

/// Validate the portable subset required by the Agent Skills standard and by
/// every supported harness: a SKILL.md frontmatter block with matching `name`
/// and a non-empty `description`.
pub fn validate_skill(path: &Path, expected_name: &str) -> Result<()> {
    if !path.is_dir() {
        bail!("skill {} is not a directory", path.display());
    }
    let skill_file = path.join("SKILL.md");
    let text = fs::read_to_string(&skill_file)
        .with_context(|| format!("reading skill metadata at {}", skill_file.display()))?;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        bail!(
            "skill {} must start with YAML frontmatter",
            skill_file.display()
        );
    }
    let mut name = None;
    let mut description = None;
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = unquote(value.trim());
            match key.trim() {
                "name" => name = Some(value.to_string()),
                "description" => description = Some(value.to_string()),
                _ => {}
            }
        }
    }
    if !closed {
        bail!(
            "skill {} has unterminated YAML frontmatter",
            skill_file.display()
        );
    }
    let name = name.filter(|value| !value.is_empty()).ok_or_else(|| {
        anyhow!(
            "skill {} is missing frontmatter `name`",
            skill_file.display()
        )
    })?;
    if name != expected_name {
        bail!("skill directory '{expected_name}' does not match frontmatter name '{name}'");
    }
    if description.is_none_or(|value| value.is_empty()) {
        bail!(
            "skill {} is missing frontmatter `description`",
            skill_file.display()
        );
    }
    if name.len() > 64
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        bail!("skill name '{name}' must be <=64 lowercase letters, digits, or hyphens");
    }
    Ok(())
}

fn unquote(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// Add a file/directory to the built-in library as a symlink, preserving a
/// single source of truth. Returns the imported item.
pub fn add(path: &Path) -> Result<Item> {
    add_to(&config::library_dir()?, path)
}

pub fn add_to(root: &Path, path: &Path) -> Result<Item> {
    let source = path
        .canonicalize()
        .with_context(|| format!("resolving library source {}", path.display()))?;
    let (kind, name) = classify_source(&source)?;
    validate_name(&name)?;
    if kind == Kind::Skill {
        validate_skill(&source, &name)?;
    }
    if kind == Kind::Template {
        let text = fs::read_to_string(&source)?;
        let _: crate::templates::Template = toml::from_str(&text)
            .with_context(|| format!("parsing template {}", source.display()))?;
    }

    let parent = root.join(kind.directory());
    fs::create_dir_all(&parent)?;
    let destination = match kind {
        Kind::Skill => parent.join(&name),
        Kind::AgentsMd => parent.join(format!("{name}.md")),
        Kind::Template => parent.join(format!("{name}.toml")),
    };
    if destination.exists() || destination.is_symlink() {
        let existing = destination.canonicalize().ok();
        if existing.as_deref() == Some(source.as_path()) {
            return Ok(Item {
                name,
                kind,
                path: source,
                library_root: root.to_path_buf(),
            });
        }
        bail!(
            "{} '{}' already exists at {}",
            kind.label(),
            name,
            destination.display()
        );
    }
    link_or_copy(&source, &destination)?;
    Ok(Item {
        name,
        kind,
        path: source,
        library_root: root.to_path_buf(),
    })
}

fn classify_source(path: &Path) -> Result<(Kind, String)> {
    if path.is_dir() && path.join("SKILL.md").is_file() {
        return Ok((
            Kind::Skill,
            path.file_name()
                .context("skill path has no name")?
                .to_string_lossy()
                .into_owned(),
        ));
    }
    if path.is_file() {
        let name = path
            .file_stem()
            .context("library file has no name")?
            .to_string_lossy()
            .into_owned();
        return match path.extension().and_then(|ext| ext.to_str()) {
            Some("md") => Ok((Kind::AgentsMd, name)),
            Some("toml") => Ok((Kind::Template, name)),
            _ => bail!(
                "unsupported library file {}; expected .md or .toml",
                path.display()
            ),
        };
    }
    bail!(
        "{} is not a skill directory, Markdown snippet, or TOML template",
        path.display()
    )
}

/// Remove a built-in item. External library roots are read-only from agentws.
pub fn remove(name: &str, kind: Option<Kind>) -> Result<Vec<Kind>> {
    remove_from(&config::library_dir()?, name, kind)
}

pub fn remove_from(root: &Path, name: &str, kind: Option<Kind>) -> Result<Vec<Kind>> {
    validate_name(name)?;
    let kinds: Vec<Kind> = kind
        .map(|value| vec![value])
        .unwrap_or_else(|| vec![Kind::Skill, Kind::AgentsMd, Kind::Template]);
    let mut matches = Vec::new();
    for kind in kinds {
        let path = match kind {
            Kind::Skill => root.join("skills").join(name),
            Kind::AgentsMd => root.join("agents").join(format!("{name}.md")),
            Kind::Template => root.join("templates").join(format!("{name}.toml")),
        };
        if path.exists() || path.is_symlink() {
            matches.push((kind, path));
        }
    }
    if matches.is_empty() {
        bail!("no built-in library item named '{name}'");
    }
    if kind.is_none() && matches.len() > 1 {
        let labels = matches
            .iter()
            .map(|(kind, _)| kind.label())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("'{name}' matches multiple kinds ({labels}); pass --kind");
    }
    let mut removed = Vec::new();
    for (kind, path) in matches {
        remove_path(&path)?;
        removed.push(kind);
    }
    Ok(removed)
}

fn remove_path(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn link_or_copy(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, destination).with_context(|| {
            format!(
                "linking library item {} -> {}",
                destination.display(),
                source.display()
            )
        })?;
    }
    #[cfg(not(unix))]
    {
        if source.is_dir() {
            copy_dir(source, destination)?;
        } else {
            fs::copy(source, destination)?;
        }
    }
    Ok(())
}

pub fn copy_dir(source: &Path, destination: &Path) -> Result<()> {
    copy_dir_inner(source, destination, &mut Vec::new())
}

fn copy_dir_inner(source: &Path, destination: &Path, ancestors: &mut Vec<PathBuf>) -> Result<()> {
    let canonical = source
        .canonicalize()
        .with_context(|| format!("resolving copied skill directory {}", source.display()))?;
    if ancestors.contains(&canonical) {
        bail!(
            "skill copy encountered a symlink cycle at {}",
            source.display()
        );
    }
    ancestors.push(canonical);
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            let resolved = source_path.canonicalize().with_context(|| {
                format!("resolving copied skill symlink {}", source_path.display())
            })?;
            if resolved.is_dir() {
                copy_dir_inner(&resolved, &destination_path, ancestors)?;
            } else {
                fs::copy(&resolved, &destination_path)?;
            }
        } else if metadata.is_dir() {
            copy_dir_inner(&source_path, &destination_path, ancestors)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    ancestors.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test {name}\n---\n\nDo it.\n"),
        )
        .unwrap();
        path
    }

    #[test]
    fn discovers_all_kinds_and_first_root_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let one = tmp.path().join("one");
        let two = tmp.path().join("two");
        write_skill(&one.join("skills"), "review");
        write_skill(&two.join("skills"), "review");
        fs::create_dir_all(one.join("agents")).unwrap();
        fs::write(one.join("agents/house.md"), "Use house style.\n").unwrap();
        fs::create_dir_all(two.join("templates")).unwrap();
        fs::write(two.join("templates/full.toml"), "repos = [\"api\"]\n").unwrap();

        let items = discover_roots(&[one.clone(), two]).unwrap();
        assert_eq!(items.len(), 3);
        let skill = items.iter().find(|item| item.kind == Kind::Skill).unwrap();
        assert_eq!(skill.library_root, one);
    }

    #[test]
    fn add_and_remove_are_idempotent_for_same_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source_root = tmp.path().join("source");
        let skill = write_skill(&source_root, "review");
        let library = tmp.path().join("library");

        let first = add_to(&library, &skill).unwrap();
        let second = add_to(&library, &skill).unwrap();
        assert_eq!(first, second);
        assert!(library.join("skills/review").exists());
        assert_eq!(
            remove_from(&library, "review", None).unwrap(),
            vec![Kind::Skill]
        );
        assert!(!library.join("skills/review").exists());
    }

    #[test]
    fn rejects_invalid_skill_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), "No frontmatter\n").unwrap();
        assert!(validate_skill(&path, "bad").is_err());
    }
}
