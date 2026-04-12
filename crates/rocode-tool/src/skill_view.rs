use async_trait::async_trait;
use serde::Deserialize;
use std::path::Path;

use crate::skill_support::{
    authority_for, format_loaded_skill_file_output, format_loaded_skill_output,
    load_skill_with_runtime_materialization, map_skill_error, resolve_skill_filter,
    resolve_skill_with_runtime_materialization,
};
use crate::{PermissionRequest, Tool, ToolContext, ToolError, ToolResult};

pub struct SkillViewTool;

#[derive(Debug, Deserialize)]
struct SkillViewInput {
    name: String,
    #[serde(default)]
    file_path: Option<String>,
}

#[async_trait]
impl Tool for SkillViewTool {
    fn id(&self) -> &str {
        "skill_view"
    }

    fn description(&self) -> &str {
        "Load a specific skill's full SKILL.md content or one supporting file. Use skills_categories, then skills_list, to choose the correct skill."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Exact skill name. Use skills_categories and skills_list first to inspect names, descriptions, and categories."
                },
                "file_path": {
                    "type": "string",
                    "description": "Optional supporting file path relative to the skill root, e.g. references/api.md"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: SkillViewInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        let authority = authority_for(Path::new(&ctx.directory), ctx.config_store.clone());
        let resolved_filter = resolve_skill_filter(&ctx, None).await;
        let filter = resolved_filter.as_filter();

        if let Some(file_path) = input.file_path.as_deref() {
            let meta = resolve_skill_with_runtime_materialization(
                &authority,
                Path::new(&ctx.directory),
                ctx.config_store.clone(),
                &input.name,
                Some(&filter),
                Some(&ctx.extra),
            )?;
            ctx.ask_permission(
                PermissionRequest::new("skill")
                    .with_pattern(&meta.name)
                    .with_always(&meta.name),
            )
            .await?;
            let loaded = authority
                .load_skill_file(&meta.name, file_path)
                .map_err(map_skill_error)?;
            let (output, metadata) = format_loaded_skill_file_output(&loaded);
            return Ok(ToolResult {
                title: format!(
                    "Loaded skill file: {} :: {}",
                    loaded.skill_name, loaded.file_path
                ),
                output,
                metadata,
                truncated: false,
            });
        }

        ctx.ask_permission(
            PermissionRequest::new("skill")
                .with_pattern(&input.name)
                .with_always(&input.name),
        )
        .await?;

        let loaded = load_skill_with_runtime_materialization(
            &authority,
            Path::new(&ctx.directory),
            ctx.config_store.clone(),
            &input.name,
            Some(&filter),
            Some(&ctx.extra),
        )?;
        let detail = authority
            .load_skill_detail_for_meta(&loaded.meta)
            .map_err(map_skill_error)?;
        let (output, metadata) = format_loaded_skill_output(
            &loaded,
            Some(&detail),
            Path::new(&ctx.directory),
            None,
            None,
        );
        Ok(ToolResult {
            title: format!("Loaded skill: {}", loaded.meta.name),
            output,
            metadata,
            truncated: false,
        })
    }
}
