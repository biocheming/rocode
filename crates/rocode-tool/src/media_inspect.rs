use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::attachment_metadata::collect_attachments_from_metadata;
use crate::path_guard::{resolve_user_path, RootPathFallbackPolicy};
use crate::{Metadata, PermissionRequest, Tool, ToolContext, ToolError, ToolRegistry, ToolResult};
use rocode_core::contracts::tools::BuiltinToolName;

const DEFAULT_QUESTION: &str =
    "Describe the relevant contents of this media file and answer the user's need concisely.";
const DESCRIPTION: &str = r#"Inspect a local media file by delegating to the `media-reader` agent.

Phase 2 scope:
- accepts a local file path
- preflights the target through the authoritative `read` tool when registry access is available
- preserves discovered attachment payloads on the `media_inspect` tool result
- creates a `media-reader` subsession with explicit preflight media context
- still relies on the existing attachment-aware `read` tool for image/PDF payload delivery inside the delegated session

This tool does not perform OCR or binary parsing itself."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaInspectInput {
    #[serde(alias = "file_path")]
    file_path: String,
    #[serde(default)]
    question: Option<String>,
}

#[derive(Debug, Clone)]
struct MediaPreflight {
    output: String,
    metadata: Metadata,
    attachments: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ReadInvokeArgs {
    file_path: String,
}

#[derive(Debug, Default, Deserialize)]
struct MediaPreflightMetadataWire {
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    size: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct AttachmentSummaryWire {
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

fn preflight_metadata_wire(metadata: &Metadata) -> MediaPreflightMetadataWire {
    serde_json::from_value::<MediaPreflightMetadataWire>(serde_json::Value::Object(
        metadata.clone().into_iter().collect(),
    ))
    .unwrap_or_default()
}

pub struct MediaInspectTool;

impl MediaInspectTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(
        &self,
        input: &MediaInspectInput,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let resolved_path = resolve_media_path(input, ctx)?;
        let preflight = run_media_preflight(input, &resolved_path, ctx).await?;
        let agent = ctx.do_get_agent_info("media-reader").await.ok_or_else(|| {
            ToolError::ExecutionError("media-reader agent is not configured".to_string())
        })?;
        let preferred_model = if let Some(model) = agent
            .model
            .as_ref()
            .map(|m| format!("{}:{}", m.provider_id, m.model_id))
        {
            Some(model)
        } else {
            ctx.do_get_last_model().await
        };

        let session_id = ctx
            .do_create_subsession(
                "media-reader".to_string(),
                Some(format_media_title(&resolved_path)),
                preferred_model.clone(),
                Vec::new(),
            )
            .await?;
        let prompt = build_media_prompt(
            &resolved_path,
            input.question.as_deref(),
            preflight.as_ref(),
        );
        let result_text = ctx.do_prompt_subsession(session_id.clone(), prompt).await?;

        let mut metadata = Metadata::new();
        metadata.insert("agent".to_string(), serde_json::json!("media-reader"));
        metadata.insert("sessionId".to_string(), serde_json::json!(session_id));
        metadata.insert(
            "filePath".to_string(),
            serde_json::json!(resolved_path.to_string_lossy().to_string()),
        );
        if let Some(model) = preferred_model {
            metadata.insert("model".to_string(), serde_json::json!(model));
        }
        if let Some(question) = input
            .question
            .as_ref()
            .map(|q| q.trim())
            .filter(|q| !q.is_empty())
        {
            metadata.insert("question".to_string(), serde_json::json!(question));
        }
        if let Some(preflight) = preflight {
            metadata.insert(
                "preflight".to_string(),
                serde_json::json!({
                    "output": preflight.output,
                    "metadata": preflight.metadata,
                }),
            );
            if !preflight.attachments.is_empty() {
                metadata.insert(
                    "attachments".to_string(),
                    serde_json::Value::Array(preflight.attachments.clone()),
                );
                if let Some(first) = preflight.attachments.first() {
                    metadata.insert("attachment".to_string(), first.clone());
                }
            }
        }

        Ok(ToolResult {
            title: format_media_title(&resolved_path),
            output: result_text,
            metadata,
            truncated: false,
        })
    }
}

impl Default for MediaInspectTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MediaInspectTool {
    fn id(&self) -> &str {
        "media_inspect"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Absolute path or session-relative local media file path"
                },
                "question": {
                    "type": "string",
                    "description": "Optional question to answer about the media file"
                }
            },
            "required": ["file_path"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: MediaInspectInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        validate_input(&input)?;

        let mut permission = PermissionRequest::for_tool(BuiltinToolName::MediaInspect)
            .with_pattern(&input.file_path)
            .with_metadata("file_path", serde_json::json!(&input.file_path))
            .always_allow();
        if let Some(question) = input.question.as_ref() {
            permission = permission.with_metadata("question", serde_json::json!(question));
        }
        ctx.ask_permission(permission).await?;

        self.execute_impl(&input, &ctx).await
    }
}

fn validate_input(input: &MediaInspectInput) -> Result<(), ToolError> {
    if input.file_path.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "file_path cannot be empty".to_string(),
        ));
    }
    Ok(())
}

fn resolve_media_path(input: &MediaInspectInput, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    let base_dir = if ctx.directory.is_empty() {
        Path::new(".")
    } else {
        Path::new(&ctx.directory)
    };
    let resolved = resolve_user_path(
        input.file_path.trim(),
        base_dir,
        RootPathFallbackPolicy::ExistingFallbackOnly,
    );
    if !resolved.resolved.exists() {
        return Err(ToolError::FileNotFound(format!(
            "media file not found: {}",
            resolved.resolved.display()
        )));
    }
    Ok(resolved.resolved)
}

async fn run_media_preflight(
    input: &MediaInspectInput,
    resolved_path: &Path,
    ctx: &ToolContext,
) -> Result<Option<MediaPreflight>, ToolError> {
    let Some(registry) = ctx.registry.as_ref().map(Arc::clone) else {
        return Ok(None);
    };
    execute_preflight_read(&registry, resolved_path, input, ctx)
        .await
        .map(Some)
}

async fn execute_preflight_read(
    registry: &ToolRegistry,
    resolved_path: &Path,
    _input: &MediaInspectInput,
    ctx: &ToolContext,
) -> Result<MediaPreflight, ToolError> {
    let result = registry
        .execute(
            "read",
            serde_json::to_value(ReadInvokeArgs {
                file_path: resolved_path.to_string_lossy().to_string(),
            })
            .unwrap_or(serde_json::Value::Null),
            ctx.clone(),
        )
        .await?;
    let attachments = collect_attachments_from_metadata(&result.metadata);
    Ok(MediaPreflight {
        output: result.output,
        metadata: result.metadata,
        attachments,
    })
}

fn build_media_prompt(
    path: &Path,
    question: Option<&str>,
    preflight: Option<&MediaPreflight>,
) -> String {
    let question = question
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .unwrap_or(DEFAULT_QUESTION);

    let mut prompt = format!(
        "Inspect the local media file at `{}`. First call the `read` tool on this exact path. Then answer this question:\n\n{}",
        path.display(),
        question
    );

    if let Some(preflight) = preflight {
        let preflight_metadata = preflight_metadata_wire(&preflight.metadata);
        let attachment_summary = summarize_attachment_payloads(&preflight.attachments);
        prompt.push_str("\n\nPreflight media context from the authoritative `read` tool:\n");
        prompt.push_str(&format!(
            "- read output: {}\n",
            sanitize_prompt_line(&preflight.output)
        ));
        if let Some(mime) = preflight_metadata.mime.as_deref() {
            prompt.push_str(&format!("- mime: {}\n", mime));
        }
        if let Some(size) = preflight_metadata.size {
            prompt.push_str(&format!("- size: {}\n", size));
        }
        if let Some(summary) = attachment_summary {
            prompt.push_str(&format!("- attachment summary: {}\n", summary));
        }
        prompt.push_str(
            "Use this preflight only as guidance. You must still call `read` on the exact same file path so the session can obtain the attachment payload for interpretation.",
        );
    } else {
        prompt.push_str(
            " If the read result includes an image or PDF attachment payload, use that attachment to interpret the file.",
        );
    }

    prompt
}

fn summarize_attachment_payloads(attachments: &[serde_json::Value]) -> Option<String> {
    let first = attachments.first()?;
    let first_wire =
        serde_json::from_value::<AttachmentSummaryWire>(first.clone()).unwrap_or_default();
    let mime = first_wire.mime.as_deref().unwrap_or("unknown");
    let filename = first_wire.filename.as_deref().unwrap_or("unknown");
    let url_kind = first_wire
        .url
        .as_deref()
        .map(|url| {
            if url.starts_with("data:") {
                "data-url payload"
            } else {
                "remote/file url"
            }
        })
        .unwrap_or("unknown url kind");
    Some(format!(
        "{} attachment(s), first mime={}, filename={}, payload={}.",
        attachments.len(),
        mime,
        filename,
        url_kind
    ))
}

fn sanitize_prompt_line(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

fn format_media_title(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("media file");
    format!("Media Inspect: {}", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read::ReadTool, TaskAgentInfo, ToolContext, ToolRegistry};
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    #[test]
    fn schema_requires_file_path() {
        let schema = MediaInspectTool::new().parameters();
        assert_eq!(schema["required"][0], serde_json::json!("file_path"));
    }

    #[test]
    fn prompt_mentions_preflight_context_when_available() {
        let preflight = MediaPreflight {
            output: "PDF read successfully (12 bytes)".to_string(),
            metadata: {
                let mut metadata = Metadata::new();
                metadata.insert("mime".to_string(), serde_json::json!("application/pdf"));
                metadata.insert("size".to_string(), serde_json::json!(12));
                metadata
            },
            attachments: vec![serde_json::json!({
                "mime": "application/pdf",
                "filename": "sample.pdf",
                "url": "data:application/pdf;base64,AA=="
            })],
        };
        let prompt = build_media_prompt(
            Path::new("/tmp/sample.pdf"),
            Some("Summarize the first page"),
            Some(&preflight),
        );
        assert!(prompt.contains("Preflight media context"));
        assert!(prompt.contains("mime: application/pdf"));
        assert!(prompt.contains("attachment summary"));
        assert!(prompt.contains("Summarize the first page"));
    }

    #[tokio::test]
    async fn execute_delegates_to_media_reader_with_preflight_attachments() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("sample.pdf");
        fs::write(&file_path, b"%PDF-1.7\n").expect("fixture media should write");

        let registry = Arc::new(ToolRegistry::new());
        registry.register(ReadTool::new()).await;

        let create_calls = Arc::new(Mutex::new(Vec::<(
            String,
            Option<String>,
            Option<String>,
            Vec<String>,
        )>::new()));
        let prompt_calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

        let ctx = ToolContext::new(
            "session-1".into(),
            "message-1".into(),
            dir.path().to_string_lossy().to_string(),
        )
        .with_registry(registry)
        .with_get_agent_info(|name| async move {
            if name == "media-reader" {
                Ok(Some(TaskAgentInfo {
                    name: "media-reader".to_string(),
                    model: None,
                    can_use_task: false,
                    steps: Some(12),
                    execution: None,
                    max_tokens: None,
                    temperature: None,
                    top_p: None,
                    variant: None,
                }))
            } else {
                Ok(None)
            }
        })
        .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
        .with_create_subsession({
            let create_calls = create_calls.clone();
            move |agent, title, model, disabled_tools| {
                let create_calls = create_calls.clone();
                async move {
                    create_calls
                        .lock()
                        .await
                        .push((agent, title, model, disabled_tools));
                    Ok("media_reader_session".to_string())
                }
            }
        })
        .with_prompt_subsession({
            let prompt_calls = prompt_calls.clone();
            move |session_id, prompt| {
                let prompt_calls = prompt_calls.clone();
                async move {
                    prompt_calls.lock().await.push((session_id, prompt));
                    Ok("media findings".to_string())
                }
            }
        });

        let result = MediaInspectTool::new()
            .execute(
                serde_json::json!({
                    "file_path": "sample.pdf",
                    "question": "What does this document contain?"
                }),
                ctx,
            )
            .await
            .expect("media inspect should succeed");

        assert_eq!(result.output, "media findings");
        assert_eq!(result.metadata["agent"], serde_json::json!("media-reader"));
        assert_eq!(
            result.metadata["sessionId"],
            serde_json::json!("media_reader_session")
        );
        assert!(result.metadata.contains_key("preflight"));
        let attachments = result
            .metadata
            .get("attachments")
            .and_then(|value| value.as_array())
            .expect("attachments should exist after preflight read");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0]["mime"], serde_json::json!("application/pdf"));

        let create_calls = create_calls.lock().await.clone();
        assert_eq!(create_calls.len(), 1);
        assert_eq!(create_calls[0].0, "media-reader");
        assert_eq!(create_calls[0].2, Some("provider-x:model-y".to_string()));

        let prompt_calls = prompt_calls.lock().await.clone();
        assert_eq!(prompt_calls.len(), 1);
        assert_eq!(prompt_calls[0].0, "media_reader_session");
        assert!(prompt_calls[0].1.contains("sample.pdf"));
        assert!(prompt_calls[0]
            .1
            .contains("What does this document contain?"));
        assert!(prompt_calls[0].1.contains("Preflight media context"));
        assert!(prompt_calls[0].1.contains("attachment summary"));
    }
}
