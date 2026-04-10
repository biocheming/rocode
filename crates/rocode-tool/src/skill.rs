use async_trait::async_trait;
use rocode_config::ConfigStore;
use rocode_skill::{SkillAuthority, SkillDefinition, SkillError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

use crate::{PermissionRequest, Tool, ToolContext, ToolError, ToolResult};

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

fn map_skill_error(err: SkillError) -> ToolError {
    match err {
        SkillError::UnknownSkill {
            requested,
            available,
        } => ToolError::InvalidArguments(format!(
            "Unknown skill: {}. Available skills: {}",
            requested, available
        )),
    }
}

fn authority_for(base: &Path, config_store: Option<Arc<ConfigStore>>) -> SkillAuthority {
    SkillAuthority::new(base.to_path_buf(), config_store)
}

fn sample_skill_files(skill: &SkillDefinition, limit: usize) -> Vec<PathBuf> {
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
        "skill"
    }

    fn description(&self) -> &str {
        "Load and execute a skill (predefined expertise module). Skills provide specialized knowledge for specific tasks."
    }

    fn parameters(&self) -> serde_json::Value {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let config_store = ConfigStore::from_project_dir(&base).ok().map(Arc::new);
        let authority = authority_for(&base, config_store);
        let skill_names: Vec<String> = authority
            .list_skills()
            .into_iter()
            .map(|skill| skill.name)
            .collect();

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

        let authority = authority_for(Path::new(&ctx.directory), ctx.config_store.clone());
        let skills = authority.discover_skills();
        let skill = skills
            .iter()
            .find(|s| s.name == input.skill_name)
            .cloned()
            .ok_or_else(|| {
                map_skill_error(SkillError::UnknownSkill {
                    requested: input.skill_name.clone(),
                    available: skills
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                })
            })?;

        ctx.ask_permission(
            PermissionRequest::new("skill")
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

        let sampled_files = sample_skill_files(&skill, 10);
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
            "display.summary".to_string(),
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
