use async_trait::async_trait;
use rocode_core::contracts::events::BusEventName;
use rocode_core::contracts::fs::{keys as fs_keys, FileWatcherEventKind};
use rocode_core::contracts::patch::keys as patch_keys;
use rocode_core::contracts::permission::PermissionTypeWire;
use rocode_core::contracts::tools::{arg_keys as tool_arg_keys, BuiltinToolName};
use std::path::{Path, PathBuf};
use tokio::fs;

use super::replacers::CompositeReplacer;
use crate::path_guard::{resolve_user_path, RootPathFallbackPolicy};
use crate::{with_file_lock, Metadata, Tool, ToolContext, ToolError, ToolResult};

#[cfg(feature = "lsp")]
const MAX_DIAGNOSTICS_PER_FILE: usize = 20;

pub struct EditTool {
    directory: PathBuf,
}

impl EditTool {
    pub fn new() -> Self {
        Self {
            directory: std::env::current_dir().unwrap_or_default(),
        }
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EditTool {
    fn id(&self) -> &str {
        BuiltinToolName::Edit.as_str()
    }

    fn description(&self) -> &str {
        "Performs string replacements in a file with multiple matching strategies. Use this to make precise edits."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                (patch_keys::FILE_PATH_SNAKE): {
                    "type": "string",
                    "description": "Absolute path or project-relative path to the file to edit"
                },
                (patch_keys::OLD_STRING): {
                    "type": "string",
                    "description": "The text to replace"
                },
                (patch_keys::NEW_STRING): {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_string (default false)"
                }
            },
            "required": [patch_keys::FILE_PATH_SNAKE, patch_keys::OLD_STRING, patch_keys::NEW_STRING]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let file_path: String = args
            .get(patch_keys::FILE_PATH_SNAKE)
            .or_else(|| args.get(patch_keys::FILE_PATH))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArguments("file_path (or filePath) is required".into())
            })?
            .trim()
            .to_string();

        let old_string: String = args
            .get(patch_keys::OLD_STRING)
            .or_else(|| args.get("oldString"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArguments("old_string (or oldString) is required".into())
            })?
            .to_string();

        let new_string: String = args
            .get(patch_keys::NEW_STRING)
            .or_else(|| args.get("newString"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArguments("new_string (or newString) is required".into())
            })?
            .to_string();

        let replace_all = args
            .get("replace_all")
            .or_else(|| args.get("replaceAll"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let base_dir = if ctx.directory.is_empty() {
            &self.directory
        } else {
            Path::new(&ctx.directory)
        };

        let resolved = resolve_user_path(
            &file_path,
            base_dir,
            RootPathFallbackPolicy::ExistingFallbackOnly,
        );
        let path = resolved.resolved;
        if let Some(original) = resolved.corrected_from {
            tracing::warn!(
                from = %original.display(),
                to = %path.display(),
                session_dir = %base_dir.display(),
                "corrected suspicious root-level edit path into session directory"
            );
        }

        let path_str = path.to_string_lossy().to_string();

        if ctx.is_external_path(&path_str) {
            let parent = path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| path_str.clone());

            ctx.ask_permission(
                crate::PermissionRequest::new(PermissionTypeWire::ExternalDirectory.as_str())
                    .with_pattern(format!("{}/*", parent))
                    .with_metadata(patch_keys::FILEPATH, serde_json::json!(&path_str))
                    .with_metadata(tool_arg_keys::PARENT_DIR, serde_json::json!(parent)),
            )
            .await?;
        }

        let title = path
            .strip_prefix(&ctx.worktree)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        let ctx_clone = ctx.clone();
        let path_clone = path.clone();
        let path_str_clone = path_str.clone();
        let title_clone = title.clone();
        let old_string_clone = old_string.clone();
        let new_string_clone = new_string.clone();

        with_file_lock(&path_str, || async {
            let content = fs::read_to_string(&path_clone)
                .await
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read file: {}", e)))?;

            let content = normalize_line_endings(&content);

            let existed = !content.is_empty();
            if existed {
                ctx_clone
                    .do_file_time_assert(path_str_clone.clone())
                    .await?;
            }

            if old_string_clone.is_empty() {
                let new_string_normalized = normalize_line_endings(&new_string_clone);
                let diff = create_diff(&path_str_clone, "", &new_string_normalized);
                ctx_clone
                    .ask_permission(
                        crate::PermissionRequest::new(BuiltinToolName::Edit.as_str())
                            .with_pattern(&path_str_clone)
                            .with_metadata(patch_keys::DIFF, serde_json::json!(diff))
                            .always_allow(),
                    )
                    .await?;

                fs::write(&path_clone, &new_string_normalized)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionError(format!("Failed to write file: {}", e))
                    })?;

                let file_watcher_event = if existed {
                    FileWatcherEventKind::Change
                } else {
                    FileWatcherEventKind::Add
                };

                ctx_clone
                    .do_publish_bus(
                        BusEventName::FileEdited.as_str(),
                        serde_json::json!({
                            (fs_keys::FILE): path_str_clone.clone()
                        }),
                    )
                    .await;

                ctx_clone
                    .do_publish_bus(
                        BusEventName::FileWatcherUpdated.as_str(),
                        serde_json::json!({
                            (fs_keys::FILE): path_str_clone.clone(),
                            (fs_keys::EVENT): file_watcher_event.as_str()
                        }),
                    )
                    .await;

                ctx_clone
                    .do_lsp_touch_file(path_str_clone.clone(), true)
                    .await?;
                ctx_clone.do_file_time_read(path_str_clone.clone()).await?;

                let output = format!("Created new file content at {}", path_clone.display());
                let (lsp_output, lsp_diagnostics) =
                    get_lsp_diagnostics_with_meta(&path_clone, &ctx_clone).await;
                let final_output = if !lsp_output.is_empty() {
                    format!("{}\n\n{}", output, lsp_output)
                } else {
                    output
                };

                let path_for_metadata = path_str_clone.clone();

                return Ok(ToolResult {
                    title: title_clone,
                    output: final_output,
                    metadata: {
                        let mut m = Metadata::new();
                        m.insert(
                            patch_keys::FILEPATH.into(),
                            serde_json::json!(path_for_metadata),
                        );
                        if !lsp_diagnostics.is_empty() {
                            m.insert(
                                patch_keys::DIAGNOSTICS.into(),
                                serde_json::json!(lsp_diagnostics),
                            );
                        }
                        m
                    },
                    truncated: false,
                });
            }

            let replacer = CompositeReplacer::new();
            let old_string_normalized = normalize_line_endings(&old_string_clone);
            let new_string_normalized = normalize_line_endings(&new_string_clone);
            let new_content = replacer
                .replace(
                    &content,
                    &old_string_normalized,
                    &new_string_normalized,
                    replace_all,
                )
                .map_err(ToolError::ExecutionError)?;

            let diff = create_diff(&path_str_clone, &content, &new_content);
            ctx_clone
                .ask_permission(
                    crate::PermissionRequest::new(BuiltinToolName::Edit.as_str())
                        .with_pattern(&path_str_clone)
                        .with_metadata(patch_keys::DIFF, serde_json::json!(diff))
                        .always_allow(),
                )
                .await?;

            let replacements = if replace_all {
                content.matches(&old_string_normalized).count()
            } else {
                1
            };

            fs::write(&path_clone, &new_content)
                .await
                .map_err(|e| ToolError::ExecutionError(format!("Failed to write file: {}", e)))?;

            let file_watcher_event = if existed {
                FileWatcherEventKind::Change
            } else {
                FileWatcherEventKind::Add
            };

            ctx_clone
                .do_publish_bus(
                    BusEventName::FileEdited.as_str(),
                    serde_json::json!({
                        (fs_keys::FILE): path_str_clone.clone()
                    }),
                )
                .await;

            ctx_clone
                .do_publish_bus(
                    BusEventName::FileWatcherUpdated.as_str(),
                    serde_json::json!({
                        (fs_keys::FILE): path_str_clone.clone(),
                        (fs_keys::EVENT): file_watcher_event.as_str()
                    }),
                )
                .await;

            ctx_clone
                .do_lsp_touch_file(path_str_clone.clone(), true)
                .await?;
            ctx_clone.do_file_time_read(path_str_clone.clone()).await?;

            let base_output = format!(
                "Successfully edited {} ({} replacement{})",
                path_clone.display(),
                replacements,
                if replacements != 1 { "s" } else { "" }
            );

            let (lsp_output, lsp_diagnostics) =
                get_lsp_diagnostics_with_meta(&path_clone, &ctx_clone).await;
            let final_output = if lsp_output.is_empty() {
                base_output
            } else {
                format!("{}\n\n{}", base_output, lsp_output)
            };

            let diff_for_metadata = diff.clone();
            let path_for_metadata = path_str_clone.clone();

            Ok(ToolResult {
                title: title_clone,
                output: final_output,
                metadata: {
                    let mut m = Metadata::new();
                    m.insert(
                        patch_keys::REPLACEMENTS.into(),
                        serde_json::json!(replacements),
                    );
                    m.insert(
                        patch_keys::FILEPATH.into(),
                        serde_json::json!(path_for_metadata),
                    );
                    m.insert(
                        patch_keys::DIFF.into(),
                        serde_json::json!(diff_for_metadata),
                    );
                    if !lsp_diagnostics.is_empty() {
                        m.insert(
                            patch_keys::DIAGNOSTICS.into(),
                            serde_json::json!(lsp_diagnostics),
                        );
                    }
                    m
                },
                truncated: false,
            })
        })
        .await
    }
}

async fn get_lsp_diagnostics_with_meta(
    path: &Path,
    ctx: &ToolContext,
) -> (String, Vec<serde_json::Value>) {
    #[cfg(feature = "lsp")]
    {
        use rocode_lsp::detect_language;

        if let Some(lsp_registry) = &ctx.lsp_registry {
            return get_lsp_diagnostics_impl_with_meta(path, lsp_registry.clone()).await;
        }
    }

    #[cfg(not(feature = "lsp"))]
    {
        let _ = (path, ctx);
    }

    (String::new(), Vec::new())
}

#[cfg(feature = "lsp")]
async fn get_lsp_diagnostics_impl_with_meta(
    path: &Path,
    lsp_registry: std::sync::Arc<rocode_lsp::LspClientRegistry>,
) -> (String, Vec<serde_json::Value>) {
    use rocode_lsp::detect_language;

    let language = detect_language(path);
    let clients = lsp_registry.list().await;

    let client = clients
        .iter()
        .find(|(id, _)| id.contains(language))
        .map(|(_, c)| c.clone());

    match client {
        Some(client) => {
            if let Ok(content) = tokio::fs::read_to_string(path).await {
                let _ = client.open_document(path, &content, language).await;

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                let diagnostics = client.get_diagnostics(path).await;
                let errors: Vec<_> = diagnostics
                    .iter()
                    .filter(|d| d.severity == Some(lsp_types::DiagnosticSeverity::ERROR))
                    .collect();

                if errors.is_empty() {
                    return (String::new(), Vec::new());
                }

                let total_errors = errors.len();
                let limited: Vec<_> = errors.into_iter().take(MAX_DIAGNOSTICS_PER_FILE).collect();
                let suffix = if limited.len() < total_errors {
                    format!("\n... and {} more", total_errors - limited.len())
                } else {
                    String::new()
                };

                let error_lines: Vec<String> = limited
                    .iter()
                    .map(|d| {
                        let line = d.range.start.line + 1;
                        let msg = &d.message;
                        format!("  Line {}: {}", line, msg)
                    })
                    .collect();

                let diagnostics_meta: Vec<serde_json::Value> = limited.iter()
                    .map(|d| {
                        serde_json::json!({
                            "line": d.range.start.line + 1,
                            "message": d.message,
                            "severity": d.severity.as_ref().map(|s| format!("{:?}", s)).unwrap_or_else(|| "Unknown".to_string())
                        })
                    })
                    .collect();

                let output = format!(
                    "LSP errors detected in this file, please fix:\n<diagnostics file=\"{}\">\n{}{}\n</diagnostics>",
                    path.display(),
                    error_lines.join("\n"),
                    suffix
                );

                (output, diagnostics_meta)
            } else {
                (String::new(), Vec::new())
            }
        }
        None => (String::new(), Vec::new()),
    }
}

fn create_diff(filepath: &str, old_content: &str, new_content: &str) -> String {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let mut diff = format!("--- {}\n+++ {}\n", filepath, filepath);

    let mut old_idx = 0;
    let mut new_idx = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if old_idx >= old_lines.len() {
            diff.push_str(&format!("+{}\n", new_lines[new_idx]));
            new_idx += 1;
        } else if new_idx >= new_lines.len() {
            diff.push_str(&format!("-{}\n", old_lines[old_idx]));
            old_idx += 1;
        } else if old_lines[old_idx] == new_lines[new_idx] {
            old_idx += 1;
            new_idx += 1;
        } else {
            diff.push_str(&format!("-{}\n", old_lines[old_idx]));
            diff.push_str(&format!("+{}\n", new_lines[new_idx]));
            old_idx += 1;
            new_idx += 1;
        }
    }

    diff
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}
