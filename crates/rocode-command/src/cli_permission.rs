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
use rocode_core::contracts::patch::keys as patch_keys;
use rocode_core::contracts::permission::keys as permission_keys;
use rocode_core::contracts::permission::PermissionTypeWire;
use rocode_core::contracts::tools::BuiltinToolName;
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Stores permission grants that were approved with "Allow Always".
///
/// Key format: `"{permission}:{pattern}"` (e.g. `"bash:ls"`, `"edit:src/main.rs"`).
/// A wildcard key `"{permission}:*"` means the entire permission type was blanket-approved.
#[derive(Debug, Clone, Default)]
pub struct PermissionMemory {
    granted: HashSet<String>,
}

impl PermissionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a specific permission + patterns combination was always-approved.
    pub fn grant_always(&mut self, permission: &str, patterns: &[String]) {
        if patterns.is_empty() {
            // No patterns → blanket grant for the permission type
            self.granted.insert(format!("{}:*", permission));
        } else {
            for pattern in patterns {
                self.granted.insert(format!("{}:{}", permission, pattern));
            }
        }
    }

    /// Check whether the permission request is already auto-approved.
    pub fn is_granted(&self, permission: &str, patterns: &[String]) -> bool {
        // Blanket wildcard grant
        if self.granted.contains(&format!("{}:*", permission)) {
            return true;
        }
        // Check each pattern
        if patterns.is_empty() {
            return false;
        }
        patterns
            .iter()
            .all(|p| self.granted.contains(&format!("{}:{}", permission, p)))
    }
}

/// The three possible user decisions for a permission request.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow,
    AllowAlways,
    Deny,
}

/// Format a permission request into a human-readable summary block for the terminal.
fn format_permission_summary(
    permission: &str,
    patterns: &[String],
    metadata: &std::collections::HashMap<String, serde_json::Value>,
    style: &CliStyle,
) -> String {
    let mut lines = Vec::new();

    // Permission type icon + label
    let (icon, label) = match (
        BuiltinToolName::parse(permission),
        PermissionTypeWire::parse(permission),
    ) {
        (Some(BuiltinToolName::Bash), _) => ("⚡", "Execute Command"),
        (Some(BuiltinToolName::Edit | BuiltinToolName::MultiEdit | BuiltinToolName::ApplyPatch), _) => {
            ("✏️ ", "Edit File")
        }
        (Some(BuiltinToolName::Write), _) => ("📝", "Write File"),
        (Some(BuiltinToolName::Read), _) => ("📖", "Read File"),
        (Some(BuiltinToolName::Grep), _) => ("🔍", "Search Files"),
        (Some(BuiltinToolName::Glob), _) => ("📂", "Find Files"),
        (Some(BuiltinToolName::Ls), _) | (_, Some(PermissionTypeWire::List)) => {
            ("📂", "List Directory")
        }
        (_, Some(PermissionTypeWire::ExternalDirectory)) => ("⚠️ ", "Access External Directory"),
        (Some(BuiltinToolName::WebSearch), _) => ("🌐", "Web Search"),
        (Some(BuiltinToolName::WebFetch), _) => ("🌐", "Network Request"),
        (Some(BuiltinToolName::BrowserSession), _) => ("🌐", "Browser Session"),
        (Some(BuiltinToolName::ContextDocs), _) => ("📚", "Context Docs"),
        (Some(BuiltinToolName::MediaInspect), _) => ("🖼️ ", "Media Inspect"),
        (Some(BuiltinToolName::Task | BuiltinToolName::TaskFlow), _) => {
            ("📋", "Task Management")
        }
        (Some(BuiltinToolName::CodeSearch | BuiltinToolName::GitHubResearch | BuiltinToolName::RepoHistory), _) => {
            ("🔎", "Code Research")
        }
        _ => ("🔧", permission),
    };

    lines.push(format!(
        "  {} {} {}",
        icon,
        style.bold(label),
        style.dim(&format!("({})", permission))
    ));

    // Show patterns (file paths, commands, etc.)
    if !patterns.is_empty() {
        for pattern in patterns {
            lines.push(format!("    {} {}", style.dim("→"), pattern));
        }
    }

    // Show relevant metadata
    if let Some(command) = metadata
        .get(permission_keys::COMMAND)
        .and_then(|v| v.as_str())
    {
        let display = if command.len() > 120 {
            format!("{}…", &command[..117])
        } else {
            command.to_string()
        };
        lines.push(format!("    {} {}", style.dim("$"), display));
    }

    if let Some(filepath) = metadata
        .get(patch_keys::FILEPATH)
        .and_then(|v| v.as_str())
    {
        if patterns.is_empty() || !patterns.iter().any(|p| p == filepath) {
            lines.push(format!("    {} {}", style.dim("file:"), filepath));
        }
    }

    if let Some(diff) = metadata.get(patch_keys::DIFF).and_then(|v| v.as_str()) {
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

    if let Some(query) = metadata.get("query").and_then(|v| v.as_str()) {
        lines.push(format!("    {} {}", style.dim("query:"), query));
    }

    lines.join("\n")
}

/// Present a permission approval prompt to the user.
///
/// Returns the user's decision: Allow, Allow Always, or Deny.
pub fn prompt_permission(
    permission: &str,
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
                if mem.is_granted(&request.permission, &request.patterns) {
                    return Ok(());
                }
            }

            // Check if the request itself declares always-allow patterns
            // (e.g. grep with `always_allow()` — these are auto-approved)
            if !request.always.is_empty() {
                // The tool itself says this should always be allowed
                let mut mem = memory.lock().await;
                mem.grant_always(&request.permission, &request.patterns);
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
                    mem.grant_always(&request.permission, &request.patterns);
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

    #[test]
    fn permission_memory_grant_and_check() {
        let mut mem = PermissionMemory::new();

        assert!(!mem.is_granted(BuiltinToolName::Bash.as_str(), &["ls".to_string()]));

        mem.grant_always(BuiltinToolName::Bash.as_str(), &["ls".to_string()]);
        assert!(mem.is_granted(BuiltinToolName::Bash.as_str(), &["ls".to_string()]));
        assert!(!mem.is_granted(BuiltinToolName::Bash.as_str(), &["rm -rf /".to_string()]));
    }

    #[test]
    fn permission_memory_wildcard_grant() {
        let mut mem = PermissionMemory::new();

        mem.grant_always(BuiltinToolName::Edit.as_str(), &[]);
        assert!(mem.is_granted(
            BuiltinToolName::Edit.as_str(),
            &["any-file.rs".to_string()]
        ));
        assert!(mem.is_granted(
            BuiltinToolName::Edit.as_str(),
            &["another.rs".to_string()]
        ));
    }

    #[test]
    fn permission_memory_multiple_patterns() {
        let mut mem = PermissionMemory::new();

        let patterns = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        mem.grant_always(BuiltinToolName::Edit.as_str(), &patterns);

        assert!(mem.is_granted(
            BuiltinToolName::Edit.as_str(),
            &["src/a.rs".to_string()]
        ));
        assert!(mem.is_granted(
            BuiltinToolName::Edit.as_str(),
            &["src/b.rs".to_string()]
        ));
        assert!(mem.is_granted(BuiltinToolName::Edit.as_str(), &patterns));
        assert!(!mem.is_granted(
            BuiltinToolName::Edit.as_str(),
            &["src/c.rs".to_string()]
        ));
    }

    #[test]
    fn permission_memory_empty_patterns_not_granted_without_wildcard() {
        let mem = PermissionMemory::new();
        assert!(!mem.is_granted(BuiltinToolName::Bash.as_str(), &[]));
    }

    #[test]
    fn format_summary_bash_command() {
        let style = CliStyle::plain();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            permission_keys::COMMAND.to_string(),
            serde_json::json!("cargo test --all"),
        );

        let summary = format_permission_summary(
            BuiltinToolName::Bash.as_str(),
            &["cargo test --all".to_string()],
            &metadata,
            &style,
        );

        assert!(summary.contains("Execute Command"));
        assert!(summary.contains("cargo test --all"));
    }

    #[test]
    fn format_summary_edit_with_diff() {
        let style = CliStyle::plain();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            patch_keys::DIFF.to_string(),
            serde_json::json!("-old line\n+new line"),
        );
        metadata.insert(patch_keys::FILEPATH.to_string(), serde_json::json!("src/main.rs"));

        let summary = format_permission_summary(
            BuiltinToolName::Edit.as_str(),
            &["src/main.rs".to_string()],
            &metadata,
            &style,
        );

        assert!(summary.contains("Edit File"));
        assert!(summary.contains("-old line"));
        assert!(summary.contains("+new line"));
    }
}
