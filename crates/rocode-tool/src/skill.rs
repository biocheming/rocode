use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::skill_support::{
    authority_for, format_loaded_skill_output, map_skill_error, resolve_skill_filter,
};
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

#[async_trait]
impl Tool for SkillTool {
    fn id(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Deprecated compatibility wrapper around skill_view. Load and execute a skill (predefined expertise module)."
    }

    fn parameters(&self) -> serde_json::Value {
        let base = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let authority = authority_for(
            &base,
            rocode_config::ConfigStore::from_project_dir(&base)
                .ok()
                .map(std::sync::Arc::new),
        );
        let skill_names: Vec<String> = authority
            .list_skill_meta(None)
            .unwrap_or_default()
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
        let resolved_filter = resolve_skill_filter(&ctx, None).await;
        let filter = resolved_filter.as_filter();
        let skill = authority
            .load_skill(&input.skill_name, Some(&filter))
            .map_err(map_skill_error)?;

        ctx.ask_permission(
            PermissionRequest::new("skill")
                .with_pattern(&skill.meta.name)
                .with_always(&skill.meta.name)
                .with_metadata("description", serde_json::json!(&skill.meta.description)),
        )
        .await?;

        let (output, metadata) = format_loaded_skill_output(
            &skill,
            Path::new(&ctx.directory),
            input.arguments.as_ref(),
            input.prompt.as_deref(),
        );

        Ok(ToolResult {
            title: format!("Loaded skill: {}", skill.meta.name),
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
