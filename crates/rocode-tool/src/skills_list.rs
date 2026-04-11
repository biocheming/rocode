use async_trait::async_trait;
use rocode_config::ConfigStore;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

use crate::skill_support::{
    authority_for, format_skill_list_output, map_skill_error, resolve_skill_filter,
};
use crate::{Tool, ToolContext, ToolError, ToolResult};

pub struct SkillsListTool;

#[derive(Debug, Deserialize)]
struct SkillsListInput {
    #[serde(default)]
    category: Option<String>,
}

#[async_trait]
impl Tool for SkillsListTool {
    fn id(&self) -> &str {
        "skills_list"
    }

    fn description(&self) -> &str {
        "List available skills with name and description. Use skill_view(name) to load full content."
    }

    fn parameters(&self) -> serde_json::Value {
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let config_store = ConfigStore::from_project_dir(&base).ok().map(Arc::new);
        let authority = authority_for(&base, config_store);
        let categories = authority
            .list_skill_meta(None)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|skill| skill.category)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        serde_json::json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Optional category filter",
                    "enum": categories
                }
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: SkillsListInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        let authority = authority_for(
            std::path::Path::new(&ctx.directory),
            ctx.config_store.clone(),
        );
        let resolved_filter = resolve_skill_filter(&ctx, input.category.as_deref()).await;
        let filter = resolved_filter.as_filter();
        let skills = authority
            .list_skill_meta(Some(&filter))
            .map_err(map_skill_error)?;
        let output = format_skill_list_output(&skills);

        Ok(ToolResult::simple("Available skills", output)
            .with_metadata("count", serde_json::json!(skills.len()))
            .with_metadata(
                "skills",
                serde_json::json!(skills
                    .iter()
                    .map(|skill| serde_json::json!({
                        "name": skill.name,
                        "description": skill.description,
                        "category": skill.category,
                    }))
                    .collect::<Vec<_>>()),
            ))
    }
}
