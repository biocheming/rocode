use rocode_config::{Config, ConfigStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub content: String,
    pub location: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub location: PathBuf,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SkillError {
    #[error("Unknown skill: {requested}. Available skills: {available}")]
    UnknownSkill {
        requested: String,
        available: String,
    },
}

/// Skill domain authority. This crate owns discovery, parsing, and skill
/// context rendering so adapters do not duplicate skill semantics.
#[derive(Clone)]
pub struct SkillAuthority {
    base: PathBuf,
    config_store: Option<Arc<ConfigStore>>,
}

impl SkillAuthority {
    pub fn new(base: impl Into<PathBuf>, config_store: Option<Arc<ConfigStore>>) -> Self {
        Self {
            base: base.into(),
            config_store,
        }
    }

    pub fn list_skills(&self) -> Vec<SkillSummary> {
        self.discover_skills()
            .into_iter()
            .map(|skill| SkillSummary {
                name: skill.name,
                description: skill.description,
                location: skill.location,
            })
            .collect()
    }

    pub fn discover_skills(&self) -> Vec<SkillDefinition> {
        discover_skills_with_config_store(&self.base, self.config_store.as_deref())
    }

    pub fn find_skill_by_name_ci(&self, name: &str) -> Result<SkillDefinition, SkillError> {
        let skills = self.discover_skills();
        find_skill_by_name_ci(&skills, name)
            .cloned()
            .ok_or_else(|| unknown_skill_error(name, &skills))
    }

    pub fn render_loaded_skills_context(
        &self,
        requested_names: &[String],
    ) -> Result<(String, Vec<String>), SkillError> {
        render_loaded_skills_context_with_config_store(
            &self.base,
            requested_names,
            self.config_store.as_deref(),
        )
    }
}

fn resolve_skill_path(base: &Path, raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn configured_skill_roots(base: &Path, config: &Config) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(skills) = &config.skills {
        for raw in &skills.paths {
            roots.push(resolve_skill_path(base, raw));
        }
    }

    let mut names: Vec<&String> = config.skill_paths.keys().collect();
    names.sort();
    for name in names {
        if let Some(raw) = config.skill_paths.get(name) {
            roots.push(resolve_skill_path(base, raw));
        }
    }

    roots
}

fn collect_skill_roots(base: &Path, config: Option<&Config>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(config_dir) = dirs::config_dir() {
        roots.push(config_dir.join("rocode/skill"));
        roots.push(config_dir.join("rocode/skills"));
    }

    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".rocode/skill"));
        roots.push(home.join(".rocode/skills"));
        roots.push(home.join(".agents/skills"));
        roots.push(home.join(".claude/skills"));
    }

    roots.push(base.join(".rocode/skill"));
    roots.push(base.join(".rocode/skills"));
    roots.push(base.join(".agents/skills"));
    roots.push(base.join(".claude/skills"));

    if let Some(config) = config {
        roots.extend(configured_skill_roots(base, config));
    }

    let mut deduped = Vec::new();
    for root in roots {
        if !deduped.contains(&root) {
            deduped.push(root);
        }
    }
    deduped
}

fn config_for_skill_discovery(
    base: &Path,
    config_store: Option<&ConfigStore>,
) -> Option<Arc<Config>> {
    config_store.map(ConfigStore::config).or_else(|| {
        ConfigStore::from_project_dir(base)
            .ok()
            .map(|store| store.config())
    })
}

fn parse_frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix(&format!("{key}:")) {
            let value = value.trim();
            if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                return Some(value[1..value.len() - 1].to_string());
            }
            return Some(value.to_string());
        }
    }
    None
}

fn parse_skill_file(path: &Path) -> Option<SkillDefinition> {
    let raw = fs::read_to_string(path).ok()?;
    let normalized = raw.replace("\r\n", "\n");
    let mut lines = normalized.lines();

    if lines.next()?.trim() != "---" {
        return None;
    }

    let mut frontmatter_lines = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        frontmatter_lines.push(line);
    }
    if !closed {
        return None;
    }

    let frontmatter = frontmatter_lines.join("\n");
    let content = lines.collect::<Vec<_>>().join("\n");
    let name = parse_frontmatter_value(&frontmatter, "name")?;
    let description = parse_frontmatter_value(&frontmatter, "description")?;

    Some(SkillDefinition {
        name,
        description,
        content: content.trim().to_string(),
        location: path.to_path_buf(),
    })
}

fn scan_skill_root(root: &Path) -> Vec<SkillDefinition> {
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }

    let mut skill_files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == "SKILL.md")
                .unwrap_or(false)
        })
        .collect();
    skill_files.sort();

    skill_files
        .into_iter()
        .filter_map(|path| parse_skill_file(&path))
        .collect()
}

fn discover_skills_with_config_store(
    base: &Path,
    config_store: Option<&ConfigStore>,
) -> Vec<SkillDefinition> {
    let mut by_name: HashMap<String, SkillDefinition> = HashMap::new();
    let config = config_for_skill_discovery(base, config_store);
    for root in collect_skill_roots(base, config.as_deref()) {
        for skill in scan_skill_root(&root) {
            by_name.insert(skill.name.clone(), skill);
        }
    }

    let mut skills: Vec<SkillDefinition> = by_name.into_values().collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn normalize_requested_skill_names(raw_names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for raw in raw_names {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out
            .iter()
            .any(|seen: &String| seen.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn find_skill_by_name_ci<'a>(
    skills: &'a [SkillDefinition],
    name: &str,
) -> Option<&'a SkillDefinition> {
    skills.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

fn unknown_skill_error(requested: &str, skills: &[SkillDefinition]) -> SkillError {
    let available = skills
        .iter()
        .map(|s| s.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    SkillError::UnknownSkill {
        requested: requested.to_string(),
        available,
    }
}

pub fn render_loaded_skills_context(
    base: &Path,
    requested_names: &[String],
) -> Result<(String, Vec<String>), SkillError> {
    render_loaded_skills_context_with_config_store(base, requested_names, None)
}

pub fn render_loaded_skills_context_with_config_store(
    base: &Path,
    requested_names: &[String],
    config_store: Option<&ConfigStore>,
) -> Result<(String, Vec<String>), SkillError> {
    let requested = normalize_requested_skill_names(requested_names);
    if requested.is_empty() {
        return Ok((String::new(), Vec::new()));
    }

    let skills = discover_skills_with_config_store(base, config_store);
    let mut selected: Vec<&SkillDefinition> = Vec::new();

    for name in &requested {
        let Some(skill) = find_skill_by_name_ci(&skills, name) else {
            return Err(unknown_skill_error(name, &skills));
        };
        selected.push(skill);
    }

    let mut context = String::new();
    context.push_str("<loaded_skills>\n");
    for skill in &selected {
        context.push_str(&format!("<skill name=\"{}\">\n\n", skill.name));
        context.push_str(&format!("# Skill: {}\n\n", skill.name));
        context.push_str(&skill.content);
        context.push_str("\n\n");
        context.push_str(&format!(
            "Base directory: {}\n",
            skill.location.parent().unwrap_or(base).to_string_lossy()
        ));
        context.push_str("</skill>\n");
    }
    context.push_str("</loaded_skills>");

    Ok((
        context,
        selected.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
    ))
}

pub fn list_available_skills() -> Vec<(String, String)> {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let config_store = ConfigStore::from_project_dir(&base).ok().map(Arc::new);
    SkillAuthority::new(base, config_store)
        .list_skills()
        .into_iter()
        .map(|skill| (skill.name, skill.description))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_skill_file_reads_frontmatter_and_body() {
        let dir = tempdir().unwrap();
        let skill_path = dir.path().join("SKILL.md");
        fs::write(
            &skill_path,
            r#"---
name: reviewer
description: "Review code changes"
---

# Reviewer

Do a thorough review.
"#,
        )
        .unwrap();

        let parsed = parse_skill_file(&skill_path).unwrap();
        assert_eq!(parsed.name, "reviewer");
        assert_eq!(parsed.description, "Review code changes");
        assert!(parsed.content.contains("Do a thorough review."));
    }

    #[test]
    fn discover_skills_loads_default_and_configured_skill_paths() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let rocode_skill = root.join(".rocode/skills/local/SKILL.md");
        fs::create_dir_all(rocode_skill.parent().unwrap()).unwrap();
        fs::write(
            &rocode_skill,
            r#"---
name: local-skill
description: local
---
project content
"#,
        )
        .unwrap();

        let claude_skill = root.join(".claude/skills/claude/SKILL.md");
        fs::create_dir_all(claude_skill.parent().unwrap()).unwrap();
        fs::write(
            &claude_skill,
            r#"---
name: claude-skill
description: claude
---
claude content
"#,
        )
        .unwrap();

        let extra_root = root.join("custom-skills");
        let extra_skill = extra_root.join("remote/SKILL.md");
        fs::create_dir_all(extra_skill.parent().unwrap()).unwrap();
        fs::write(
            &extra_skill,
            r#"---
name: custom-skill
description: custom
---
custom content
"#,
        )
        .unwrap();

        let mut config = Config::default();
        config
            .skill_paths
            .insert("custom".to_string(), "custom-skills".to_string());
        let authority = SkillAuthority::new(root, Some(Arc::new(ConfigStore::new(config))));
        let discovered = authority.discover_skills();
        let names: Vec<String> = discovered.into_iter().map(|s| s.name).collect();

        assert!(names.contains(&"local-skill".to_string()));
        assert!(names.contains(&"claude-skill".to_string()));
        assert!(names.contains(&"custom-skill".to_string()));
    }

    #[test]
    fn render_loaded_skills_context_resolves_and_renders_requested_skills() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let skill_path = root.join(".rocode/skills/review/SKILL.md");
        fs::create_dir_all(skill_path.parent().unwrap()).unwrap();

        fs::write(
            &skill_path,
            r#"---
name: rocode-test-review-skill
description: review
---
Check correctness first.
"#,
        )
        .unwrap();

        let (context, loaded) = render_loaded_skills_context(
            root,
            &[
                "rocode-test-review-skill".to_string(),
                "ROCODE-TEST-REVIEW-SKILL".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(loaded, vec!["rocode-test-review-skill".to_string()]);
        assert!(context.contains("<loaded_skills>"));
        assert!(context.contains("Check correctness first."));
    }

    #[test]
    fn render_loaded_skills_context_returns_error_for_unknown_skill() {
        let dir = tempdir().unwrap();
        let err =
            render_loaded_skills_context(dir.path(), &["missing-skill".to_string()]).unwrap_err();
        assert!(err.to_string().contains("Unknown skill"));
    }
}
