use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::skill_support::{
    authority_for, format_loaded_skill_output, load_skill_with_runtime_materialization,
    resolve_skill_filter,
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
        "Deprecated compatibility wrapper around skill_view. Load a specific skill only after discovering the correct name via skills_categories and skills_list."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "Exact skill name to load. First call skills_categories and skills_list to inspect names and descriptions; do not guess from memory."
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
        let skill = load_skill_with_runtime_materialization(
            &authority,
            Path::new(&ctx.directory),
            ctx.config_store.clone(),
            &input.skill_name,
            Some(&filter),
            Some(&ctx.extra),
        )?;

        ctx.ask_permission(
            PermissionRequest::new("skill")
                .with_pattern(&skill.meta.name)
                .with_always(&skill.meta.name)
                .with_metadata("description", serde_json::json!(&skill.meta.description)),
        )
        .await?;
        let detail = authority
            .load_skill_detail_for_meta(&skill.meta)
            .map_err(crate::skill_support::map_skill_error)?;

        let (output, metadata) = format_loaded_skill_output(
            &skill,
            Some(&detail),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_parameters_do_not_inline_skill_catalog_enum() {
        let schema = SkillTool.parameters();
        let skill_name = &schema["properties"]["skill_name"];
        assert!(skill_name.get("enum").is_none());
        assert!(skill_name["description"]
            .as_str()
            .unwrap_or_default()
            .contains("skills_categories"));
    }
}
