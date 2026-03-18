use async_trait::async_trait;
use rocode_core::contracts::output_blocks::keys as output_keys;
use rocode_core::contracts::tools::BuiltinToolName;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::{PermissionRequest, Tool, ToolContext, ToolError, ToolResult};
use rocode_config::load_config;

pub struct SkillTool;

#[derive(Debug, Serialize, Deserialize)]
struct SkillInput {
    #[serde(rename = "skill_name")]
    skill_name: String,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Debug, Clone)]
struct SkillInfo {
    name: String,
    description: String,
    content: String,
    location: PathBuf,
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

fn collect_skill_roots(base: &Path) -> Vec<PathBuf> {
    // Scan from lower-precedence locations to higher-precedence ones so later
    // roots override earlier ones when names collide.
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

    if let Ok(config) = load_config(base) {
        if let Some(skills) = config.skills {
            for raw in skills.paths {
                roots.push(resolve_skill_path(base, &raw));
            }
        }
        let mut names: Vec<&String> = config.skill_paths.keys().collect();
        names.sort();
        for name in names {
            if let Some(raw) = config.skill_paths.get(name) {
                roots.push(resolve_skill_path(base, raw));
            }
        }
    }

    let mut deduped = Vec::new();
    for root in roots {
        if !deduped.contains(&root) {
            deduped.push(root);
        }
    }
    deduped
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

fn parse_skill_file(path: &Path) -> Option<SkillInfo> {
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

    Some(SkillInfo {
        name,
        description,
        content: content.trim().to_string(),
        location: path.to_path_buf(),
    })
}

fn scan_skill_root(root: &Path) -> Vec<SkillInfo> {
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

fn discover_skills(base: &Path) -> Vec<SkillInfo> {
    let mut by_name: HashMap<String, SkillInfo> = HashMap::new();
    for root in collect_skill_roots(base) {
        for skill in scan_skill_root(&root) {
            by_name.insert(skill.name.clone(), skill);
        }
    }

    let mut skills: Vec<SkillInfo> = by_name.into_values().collect();
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

fn find_skill_by_name_ci<'a>(skills: &'a [SkillInfo], name: &str) -> Option<&'a SkillInfo> {
    skills.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Resolve and render selected skills as prompt context for delegated task execution.
pub fn render_loaded_skills_context(
    base: &Path,
    requested_names: &[String],
) -> Result<(String, Vec<String>), ToolError> {
    let requested = normalize_requested_skill_names(requested_names);
    if requested.is_empty() {
        return Ok((String::new(), Vec::new()));
    }

    let skills = discover_skills(base);
    let mut selected: Vec<&SkillInfo> = Vec::new();

    for name in &requested {
        let Some(skill) = find_skill_by_name_ci(&skills, name) else {
            let available = skills
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ToolError::InvalidArguments(format!(
                "Unknown skill: {}. Available skills: {}",
                name, available
            )));
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

fn sample_skill_files(skill: &SkillInfo, limit: usize) -> Vec<PathBuf> {
    let Some(base_dir) = skill.location.parent() else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = WalkDir::new(base_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name != "SKILL.md")
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.truncate(limit);
    files
}

#[async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        BuiltinToolName::Skill.as_str()
    }

    fn description(&self) -> &str {
        "Load and execute a skill (predefined expertise module). Skills provide specialized knowledge for specific tasks."
    }

    fn parameters(&self) -> serde_json::Value {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let skills = discover_skills(&base);
        let skill_names: Vec<String> = skills.into_iter().map(|s| s.name).collect();

        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "Name of the skill to load",
                    "enum": skill_names
                },
                "arguments": {
                    "type": "object",
                    "description": "Arguments to pass to the skill"
                },
                "prompt": {
                    "type": "string",
                    "description": "Additional prompt/instructions for the skill"
                }
            },
            "required": ["skill_name"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: SkillInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;

        let skills = discover_skills(Path::new(&ctx.directory));

        let skill = skills
            .iter()
            .find(|s| s.name == input.skill_name)
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!(
                    "Unknown skill: {}. Available skills: {}",
                    input.skill_name,
                    skills
                        .iter()
                        .map(|s| &s.name)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;

        ctx.ask_permission(
            PermissionRequest::new(BuiltinToolName::Skill.as_str())
                .with_pattern(&skill.name)
                .with_always(&skill.name)
                .with_metadata("description", serde_json::json!(&skill.description)),
        )
        .await?;

        let mut output = format!("<skill_content name=\"{}\">\n\n", skill.name);
        output.push_str(&format!("# Skill: {}\n\n", skill.name));
        output.push_str(&skill.content);
        output.push_str("\n\n");
        output.push_str(&format!(
            "Base directory for this skill: {}\n",
            skill
                .location
                .parent()
                .unwrap_or(Path::new(&ctx.directory))
                .display()
        ));
        output.push_str(
            "Relative paths in this skill (e.g., scripts/, references/) are relative to this base directory.\n",
        );
        output.push_str("Note: file list is sampled.\n\n");

        let sampled_files = sample_skill_files(skill, 10);
        output.push_str("<skill_files>\n");
        for file in sampled_files {
            output.push_str(&format!("<file>{}</file>\n", file.display()));
        }
        output.push_str("</skill_files>\n");

        if let Some(ref args) = input.arguments {
            output.push_str(&format!(
                "**Arguments:**\n```json\n{}\n```\n\n",
                serde_json::to_string_pretty(args).unwrap_or_default()
            ));
        }

        if let Some(ref prompt) = input.prompt {
            output.push_str(&format!("**Additional Instructions:**\n{}\n\n", prompt));
        }

        output.push_str("\n</skill_content>");

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("name".to_string(), serde_json::json!(&skill.name));
        metadata.insert(
            "dir".to_string(),
            serde_json::json!(skill
                .location
                .parent()
                .unwrap_or(Path::new(&ctx.directory))
                .to_string_lossy()
                .to_string()),
        );
        metadata.insert(
            "location".to_string(),
            serde_json::json!(skill.location.to_string_lossy().to_string()),
        );
        metadata.insert(
            "description".to_string(),
            serde_json::json!(&skill.description),
        );
        metadata.insert(
            output_keys::DISPLAY_SUMMARY.to_string(),
            serde_json::json!(format!("{}", skill.description)),
        );

        Ok(ToolResult {
            title: format!("Loaded skill: {}", skill.name),
            output,
            metadata,
            truncated: false,
        })
    }
}

impl Default for SkillTool {
    fn default() -> Self {
        Self
    }
}

pub fn list_available_skills() -> Vec<(String, String)> {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    discover_skills(&base)
        .into_iter()
        .map(|s| (s.name, s.description))
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

        fs::write(
            root.join("rocode.json"),
            r#"{
  "skill_paths": {
    "custom": "custom-skills"
  }
}"#,
        )
        .unwrap();

        let discovered = discover_skills(root);
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
        match err {
            ToolError::InvalidArguments(msg) => assert!(msg.contains("Unknown skill")),
            other => panic!("unexpected error: {:?}", other),
        }
    }
}
