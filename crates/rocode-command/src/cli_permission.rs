//! CLI interactive permission approval UI.
//!
//! Displays permission requests from tool execution and lets the user
//! choose `Allow`, `Allow Always`, or `Deny` via the interactive selector.
//!
//! "Allow Always" remembers the permission type + pattern for the remainder
//! of the session so subsequent identical requests are auto-approved.

use crate::cli_select::{interactive_select, SelectOption, SelectResult};
use crate::cli_spinner::SpinnerGuard;
use crate::cli_style::CliStyle;
use rocode_permission::PermissionKind;
pub use rocode_permission::PermissionMemory;
use serde::Deserialize;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

/// The three possible user decisions for a permission request.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PermissionMetadata {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    command: Option<String>,
    #[serde(
        default,
        alias = "filePath",
        alias = "file_path",
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    filepath: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    diff: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    query: Option<String>,
}

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        None => None,
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    })
}

impl PermissionMetadata {
    fn from_map(metadata: &std::collections::HashMap<String, serde_json::Value>) -> Self {
        serde_json::to_value(metadata)
            .ok()
            .and_then(|value| serde_json::from_value::<Self>(value).ok())
            .unwrap_or_default()
    }
}

/// Format a permission request into a human-readable summary block for the terminal.
fn format_permission_summary(
    permission: &PermissionKind,
    patterns: &[String],
    metadata: &std::collections::HashMap<String, serde_json::Value>,
    style: &CliStyle,
) -> String {
    let mut lines = Vec::new();
    let metadata = PermissionMetadata::from_map(metadata);

    // Permission type icon + label (canonical model mapping)
    let icon = permission.icon();
    let label = permission.label();

    lines.push(format!(
        "  {} {} {}",
        icon,
        style.bold(label.as_ref()),
        style.dim(&format!("({})", permission.as_str()))
    ));

    // Show patterns (file paths, commands, etc.)
    if !patterns.is_empty() {
        for pattern in patterns {
            lines.push(format!("    {} {}", style.dim("→"), pattern));
        }
    }

    // Show relevant metadata
    if let Some(command) = metadata.command.as_deref() {
        let display = if command.len() > 120 {
            format!("{}…", &command[..117])
        } else {
            command.to_string()
        };
        lines.push(format!("    {} {}", style.dim("$"), display));
    }

    if let Some(filepath) = metadata.filepath.as_deref() {
        if patterns.is_empty() || !patterns.iter().any(|p| p == filepath) {
            lines.push(format!("    {} {}", style.dim("file:"), filepath));
        }
    }

    if let Some(diff) = metadata.diff.as_deref() {
        // Show first few lines of the diff
        let diff_lines: Vec<&str> = diff.lines().take(8).collect();
        if !diff_lines.is_empty() {
            lines.push(format!("    {}", style.dim("diff:")));
            for dline in &diff_lines {
                let colored = if dline.starts_with('+') {
                    style.bold_green(dline)
                } else if dline.starts_with('-') {
                    style.bold_red(dline)
                } else {
                    style.dim(dline)
                };
                lines.push(format!("    {}", colored));
            }
            let total_diff_lines = diff.lines().count();
            if total_diff_lines > 8 {
                lines.push(format!(
                    "    {}",
                    style.dim(&format!("... ({} more lines)", total_diff_lines - 8))
                ));
            }
        }
    }

    if let Some(query) = metadata.query.as_deref() {
        lines.push(format!("    {} {}", style.dim("query:"), query));
    }

    lines.join("\n")
}

/// Present a permission approval prompt to the user.
///
/// Returns the user's decision: Allow, Allow Always, or Deny.
pub fn prompt_permission(
    permission: &PermissionKind,
    patterns: &[String],
    metadata: &std::collections::HashMap<String, serde_json::Value>,
    style: &CliStyle,
) -> io::Result<PermissionDecision> {
    let summary = format_permission_summary(permission, patterns, metadata, style);

    // Print the summary block to stderr
    let mut stderr = io::stderr();
    write!(stderr, "\n{}\n", summary)?;
    stderr.flush()?;

    let options = vec![
        SelectOption {
            label: "Allow".to_string(),
            description: Some("Allow this action once".to_string()),
        },
        SelectOption {
            label: "Allow Always".to_string(),
            description: Some("Allow this type for the rest of the session".to_string()),
        },
        SelectOption {
            label: "Deny".to_string(),
            description: Some("Block this action".to_string()),
        },
    ];

    let result = interactive_select("Permission required", None, &options, style)?;

    match result {
        SelectResult::Selected(choices) => {
            let choice = choices.first().map(|s| s.as_str()).unwrap_or("Deny");
            match choice {
                "Allow" => Ok(PermissionDecision::Allow),
                "Allow Always" => Ok(PermissionDecision::AllowAlways),
                _ => Ok(PermissionDecision::Deny),
            }
        }
        SelectResult::Other(_) => Ok(PermissionDecision::Deny),
        SelectResult::Cancelled => Ok(PermissionDecision::Deny),
    }
}

/// Build a CLI permission callback that can be passed to `AgentExecutor::with_ask_permission()`.
///
/// Returns a closure that:
/// - Checks the session-scoped `PermissionMemory` for prior "Allow Always" grants
/// - If not already granted, pauses the spinner, prompts the user interactively, then resumes
/// - Records "Allow Always" decisions in memory for future auto-approval
pub fn build_cli_permission_callback(
    spinner_guard: Arc<std::sync::Mutex<SpinnerGuard>>,
) -> impl Fn(
    rocode_tool::PermissionRequest,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), rocode_tool::ToolError>> + Send>,
> + Send
       + Sync
       + 'static {
    let memory = Arc::new(Mutex::new(PermissionMemory::new()));

    move |request: rocode_tool::PermissionRequest| {
        let memory = memory.clone();
        let spinner_guard = spinner_guard.clone();
        Box::pin(async move {
            // Check if already granted
            {
                let mem = memory.lock().await;
                if mem.is_request_granted(&request) {
                    return Ok(());
                }
            }

            // Check if the request itself declares always-allow patterns
            // (e.g. grep with `always_allow()` — these are auto-approved)
            if !request.always.is_empty() {
                // The tool itself says this should always be allowed
                let mut mem = memory.lock().await;
                mem.grant_request(&request);
                return Ok(());
            }

            // Pause spinner so it doesn't trample the permission prompt
            let guard = spinner_guard
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| SpinnerGuard::noop());
            guard.pause();

            // Prompt user on a blocking task (crossterm raw mode needs real terminal)
            let permission = request.permission.clone();
            let patterns = request.patterns.clone();
            let metadata = request.metadata.clone();

            let decision = tokio::task::spawn_blocking(move || {
                let style = CliStyle::detect();
                prompt_permission(&permission, &patterns, &metadata, &style)
            })
            .await
            .map_err(|e| {
                guard.resume();
                rocode_tool::ToolError::ExecutionError(format!("Permission prompt failed: {}", e))
            })?
            .map_err(|e| {
                guard.resume();
                rocode_tool::ToolError::ExecutionError(format!("Permission prompt IO error: {}", e))
            })?;

            guard.resume();

            match decision {
                PermissionDecision::Allow => Ok(()),
                PermissionDecision::AllowAlways => {
                    let mut mem = memory.lock().await;
                    mem.grant_request(&request);
                    Ok(())
                }
                PermissionDecision::Deny => Err(rocode_tool::ToolError::PermissionDenied(format!(
                    "User denied permission: {} [{}]",
                    request.permission,
                    request.patterns.join(", ")
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocode_core::contracts::tools::BuiltinToolName;

    #[test]
    fn permission_memory_grant_and_check() {
        let mut mem = PermissionMemory::new();

        assert!(!mem.is_granted(PermissionKind::from_tool_name("bash"), &["ls".to_string()]));

        mem.grant_always(PermissionKind::from_tool_name("bash"), &["ls".to_string()]);
        assert!(mem.is_granted(PermissionKind::from_tool_name("bash"), &["ls".to_string()]));
        assert!(!mem.is_granted(
            PermissionKind::from_tool_name("bash"),
            &["rm -rf /".to_string()]
        ));
    }

    #[test]
    fn permission_memory_wildcard_grant() {
        let mut mem = PermissionMemory::new();

        mem.grant_always(PermissionKind::from_tool_name("edit"), &[]);
        assert!(mem.is_granted(
            PermissionKind::from_tool_name("write"),
            &["any-file.rs".to_string()]
        ));
        assert!(mem.is_granted(
            PermissionKind::from_tool_name("edit"),
            &["another.rs".to_string()]
        ));
    }

    #[test]
    fn permission_memory_multiple_patterns() {
        let mut mem = PermissionMemory::new();

        let patterns = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        mem.grant_always(PermissionKind::from_tool_name("edit"), &patterns);

        assert!(mem.is_granted(
            PermissionKind::from_tool_name("edit"),
            &["src/a.rs".to_string()]
        ));
        assert!(mem.is_granted(
            PermissionKind::from_tool_name("edit"),
            &["src/b.rs".to_string()]
        ));
        assert!(mem.is_granted(PermissionKind::from_tool_name("edit"), &patterns));
        assert!(!mem.is_granted(
            PermissionKind::from_tool_name("edit"),
            &["src/c.rs".to_string()]
        ));
    }

    #[test]
    fn permission_memory_empty_patterns_not_granted_without_wildcard() {
        let mem = PermissionMemory::new();
        assert!(!mem.is_granted(PermissionKind::from_tool_name("bash"), &[]));
    }

    #[test]
    fn format_summary_bash_command() {
        let style = CliStyle::plain();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("command".to_string(), serde_json::json!("cargo test --all"));

        let summary = format_permission_summary(
            &PermissionKind::from(BuiltinToolName::Bash),
            &["cargo test --all".to_string()],
            &metadata,
            &style,
        );

        assert!(summary.contains("Run shell command"));
        assert!(summary.contains("cargo test --all"));
    }

    #[test]
    fn format_summary_edit_with_diff() {
        let style = CliStyle::plain();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "diff".to_string(),
            serde_json::json!("-old line\n+new line"),
        );
        metadata.insert("filepath".to_string(), serde_json::json!("src/main.rs"));

        let summary = format_permission_summary(
            &PermissionKind::from_tool_name("edit"),
            &["src/main.rs".to_string()],
            &metadata,
            &style,
        );

        assert!(summary.contains("Edit file"));
        assert!(summary.contains("-old line"));
        assert!(summary.contains("+new line"));
    }
}
