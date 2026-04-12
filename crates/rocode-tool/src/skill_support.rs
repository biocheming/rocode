use rocode_config::ConfigStore;
use rocode_skill::{
    infer_toolsets_from_tools, LoadedSkill, LoadedSkillFile, SkillAuthority, SkillError,
    SkillFilter, SkillGovernanceAuthority, SkillMetaView,
};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::{Metadata, ToolContext, ToolError};

#[derive(Debug, Clone, Default)]
pub(crate) struct LoadedSkillsPromptContext {
    pub prompt_context: String,
    pub loaded_skills: Vec<SkillMetaView>,
}

impl LoadedSkillsPromptContext {
    pub fn loaded_skill_names(&self) -> Vec<String> {
        self.loaded_skills
            .iter()
            .map(|skill| skill.name.clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.prompt_context.trim().is_empty()
    }
}

pub(crate) fn map_skill_error(err: SkillError) -> ToolError {
    match err {
        SkillError::UnknownSkill {
            requested,
            available,
        } => ToolError::InvalidArguments(format!(
            "Unknown skill: {}. Available skills: {}",
            requested, available
        )),
        SkillError::InvalidSkillFilePath { skill, file_path } => ToolError::InvalidArguments(
            format!("Invalid skill file path for {}: {}", skill, file_path),
        ),
        SkillError::SkillFileNotFound { skill, file_path } => ToolError::InvalidArguments(format!(
            "Skill file not found for {}: {}",
            skill, file_path
        )),
        SkillError::InvalidWriteTarget { path } => ToolError::InvalidArguments(format!(
            "Skill writes are limited to workspace .rocode/skills: {}",
            path.display()
        )),
        SkillError::SkillNotWritable { name, path } => ToolError::InvalidArguments(format!(
            "Skill {} is outside the workspace sandbox and cannot be modified here: {}",
            name,
            path.display()
        )),
        SkillError::InvalidSkillName { name } => {
            ToolError::InvalidArguments(format!("Invalid skill name: {}", name))
        }
        SkillError::InvalidSkillDescription { name } => {
            ToolError::InvalidArguments(format!("Invalid skill description for {}", name))
        }
        SkillError::InvalidSkillContent { message } => ToolError::InvalidArguments(message),
        SkillError::InvalidSkillCategory { category } => {
            ToolError::InvalidArguments(format!("Invalid skill category path: {}", category))
        }
        SkillError::InvalidSkillFrontmatter { message } => {
            ToolError::InvalidArguments(format!("Invalid skill frontmatter: {}", message))
        }
        SkillError::SkillAlreadyExists { name } => {
            ToolError::InvalidArguments(format!("Skill already exists: {}", name))
        }
        SkillError::GuardBlocked { report } => ToolError::InvalidArguments(format!(
            "Skill guard blocked {}: {}",
            report.skill_name,
            summarize_guard_report(&report)
        )),
        SkillError::SkillWriteSizeExceeded { path, size, limit } => {
            ToolError::InvalidArguments(format!(
                "Skill write exceeds size limit for {}: {} bytes > {} bytes",
                path, size, limit
            ))
        }
        SkillError::ArtifactFetchTimeout {
            locator,
            timeout_ms,
        } => ToolError::Timeout(format!(
            "Artifact fetch timed out for {} after {}ms",
            locator, timeout_ms
        )),
        SkillError::ArtifactDownloadSizeExceeded {
            locator,
            size,
            limit,
        } => ToolError::InvalidArguments(format!(
            "Artifact download size limit exceeded for {}: {} bytes > {} bytes",
            locator, size, limit
        )),
        SkillError::ArtifactExtractSizeExceeded { path, size, limit } => {
            ToolError::InvalidArguments(format!(
                "Artifact extract size limit exceeded for {}: {} bytes > {} bytes",
                path.display(),
                size,
                limit
            ))
        }
        SkillError::ArtifactChecksumMismatch { expected, actual } => {
            ToolError::InvalidArguments(format!(
                "Artifact checksum mismatch: expected sha256:{}, got sha256:{}",
                expected, actual
            ))
        }
        SkillError::ArtifactLayoutMismatch { path, message } => {
            ToolError::InvalidArguments(format!(
                "Artifact layout mismatch at {}: {}",
                path.display(),
                message
            ))
        }
        SkillError::ReadFailed { path, message } => {
            ToolError::ExecutionError(format!("Failed to read {}: {}", path.display(), message))
        }
        SkillError::WriteFailed { path, message } => {
            ToolError::ExecutionError(format!("Failed to write {}: {}", path.display(), message))
        }
    }
}

fn summarize_guard_report(report: &rocode_types::SkillGuardReport) -> String {
    report
        .violations
        .iter()
        .map(|violation| violation.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn authority_for(base: &Path, config_store: Option<Arc<ConfigStore>>) -> SkillAuthority {
    SkillAuthority::new(base.to_path_buf(), config_store)
}

pub(crate) fn governance_authority_for(
    base: &Path,
    config_store: Option<Arc<ConfigStore>>,
) -> SkillGovernanceAuthority {
    SkillGovernanceAuthority::new(base.to_path_buf(), config_store)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedSkillFilter {
    pub available_tools: Option<HashSet<String>>,
    pub available_toolsets: Option<HashSet<String>>,
    pub current_stage: Option<String>,
    pub category: Option<String>,
}

impl ResolvedSkillFilter {
    pub(crate) fn as_filter(&self) -> SkillFilter<'_> {
        SkillFilter {
            available_tools: self.available_tools.as_ref(),
            available_toolsets: self.available_toolsets.as_ref(),
            current_stage: self.current_stage.as_deref(),
            category: self.category.as_deref(),
        }
    }
}

pub(crate) async fn resolve_skill_filter(
    ctx: &ToolContext,
    category: Option<&str>,
) -> ResolvedSkillFilter {
    let available_tools = if let Some(tools) = metadata_string_set(&ctx.extra, "available_tool_ids")
    {
        Some(tools)
    } else if let Some(registry) = ctx.registry.as_ref() {
        Some(
            registry
                .list_ids()
                .await
                .into_iter()
                .map(|tool| tool.to_ascii_lowercase())
                .collect::<HashSet<_>>(),
        )
    } else {
        None
    };

    let available_toolsets = metadata_string_set(&ctx.extra, "available_toolsets").or_else(|| {
        available_tools
            .as_ref()
            .map(|tools| infer_toolsets_from_tools(tools.iter().map(String::as_str)))
    });

    ResolvedSkillFilter {
        available_tools,
        available_toolsets,
        current_stage: ctx
            .extra
            .get("scheduler_stage")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        category: category
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }
}

pub(crate) fn load_skills_prompt_context(
    base_dir: &Path,
    config_store: Option<Arc<ConfigStore>>,
    requested_skills: Option<&[String]>,
) -> Result<LoadedSkillsPromptContext, ToolError> {
    let Some(requested_skills) = requested_skills else {
        return Ok(LoadedSkillsPromptContext::default());
    };

    let authority = authority_for(base_dir, config_store);
    let requested_skills = normalize_requested_skill_names(requested_skills);
    if requested_skills.is_empty() {
        return Ok(LoadedSkillsPromptContext::default());
    }

    let mut rendered = Vec::new();
    let mut loaded_skills = Vec::new();
    for requested_skill in requested_skills {
        let skill = authority
            .load_skill(&requested_skill, None)
            .map_err(map_skill_error)?;
        let (output, _) = format_loaded_skill_output(&skill, base_dir, None, None);
        rendered.push(output);
        loaded_skills.push(SkillMetaView::from(&skill.meta));
    }

    Ok(LoadedSkillsPromptContext {
        prompt_context: format!(
            "<loaded_skills>\n{}\n</loaded_skills>",
            rendered.join("\n\n")
        ),
        loaded_skills,
    })
}

pub(crate) fn format_loaded_skill_output(
    skill: &LoadedSkill,
    base_dir: &Path,
    arguments: Option<&serde_json::Value>,
    prompt: Option<&str>,
) -> (String, Metadata) {
    let mut output = format!("<skill_content name=\"{}\">\n\n", skill.meta.name);
    output.push_str(&format!("# Skill: {}\n\n", skill.meta.name));
    output.push_str(&skill.content);
    output.push_str("\n\n");
    output.push_str(&format!(
        "Base directory for this skill: {}\n",
        skill.meta.location.parent().unwrap_or(base_dir).display()
    ));
    output.push_str(
        "Relative paths in this skill (e.g., scripts/, references/) are relative to this base directory.\n",
    );
    if !skill.meta.supporting_files.is_empty() {
        output.push_str("Supporting files available via skill_view(name, file_path):\n\n");
        output.push_str("<skill_files>\n");
        for file in &skill.meta.supporting_files {
            output.push_str(&format!("<file>{}</file>\n", file.relative_path));
        }
        output.push_str("</skill_files>\n");
    }

    if let Some(args) = arguments {
        output.push_str(&format!(
            "\n**Arguments:**\n```json\n{}\n```\n",
            serde_json::to_string_pretty(args).unwrap_or_default()
        ));
    }

    if let Some(prompt) = prompt.filter(|value| !value.trim().is_empty()) {
        output.push_str(&format!("\n**Additional Instructions:**\n{}\n", prompt));
    }

    output.push_str("\n</skill_content>");

    let mut metadata = Metadata::new();
    metadata.insert("name".to_string(), serde_json::json!(&skill.meta.name));
    metadata.insert(
        "dir".to_string(),
        serde_json::json!(skill
            .meta
            .location
            .parent()
            .unwrap_or(base_dir)
            .to_string_lossy()
            .to_string()),
    );
    metadata.insert(
        "location".to_string(),
        serde_json::json!(skill.meta.location.to_string_lossy().to_string()),
    );
    metadata.insert(
        "description".to_string(),
        serde_json::json!(&skill.meta.description),
    );
    metadata.insert(
        "supporting_files".to_string(),
        serde_json::json!(skill
            .meta
            .supporting_files
            .iter()
            .map(|file| file.relative_path.clone())
            .collect::<Vec<_>>()),
    );
    metadata.insert(
        "display.summary".to_string(),
        serde_json::json!(skill.meta.description.clone()),
    );

    (output, metadata)
}

pub(crate) fn format_loaded_skill_file_output(file: &LoadedSkillFile) -> (String, Metadata) {
    let output = format!(
        "<skill_file skill=\"{}\" path=\"{}\">\n\n{}\n\n</skill_file>",
        file.skill_name, file.file_path, file.content
    );

    let mut metadata = Metadata::new();
    metadata.insert("name".to_string(), serde_json::json!(&file.skill_name));
    metadata.insert("file_path".to_string(), serde_json::json!(&file.file_path));
    metadata.insert(
        "location".to_string(),
        serde_json::json!(file.location.to_string_lossy().to_string()),
    );
    metadata.insert(
        "display.summary".to_string(),
        serde_json::json!(format!("{} :: {}", file.skill_name, file.file_path)),
    );

    (output, metadata)
}

pub(crate) fn format_skill_list_output(skills: &[SkillMetaView]) -> String {
    if skills.is_empty() {
        return "<available_skills />".to_string();
    }

    let mut output = String::from("<available_skills>\n");
    for skill in skills {
        match skill.category.as_deref() {
            Some(category) if !category.is_empty() => {
                output.push_str(&format!(
                    "- [{}] {}: {}\n",
                    category, skill.name, skill.description
                ));
            }
            _ => {
                output.push_str(&format!("- {}: {}\n", skill.name, skill.description));
            }
        }
    }
    output.push_str("</available_skills>");
    output
}

fn normalize_requested_skill_names(raw_names: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for raw_name in raw_names {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if normalized
            .iter()
            .any(|seen: &String| seen.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn metadata_string_set(metadata: &Metadata, key: &str) -> Option<HashSet<String>> {
    let values = metadata.get(key)?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(|value| value.as_str())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect(),
    )
}
