use crate::cli_markdown;
use crate::cli_style::CliStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTone {
    Title,
    Normal,
    Muted,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBlock {
    pub tone: BlockTone,
    pub text: String,
}

impl StatusBlock {
    pub fn title(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Title,
            text: text.into(),
        }
    }

    pub fn normal(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Normal,
            text: text.into(),
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Muted,
            text: text.into(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Success,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Error,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePhase {
    Start,
    Delta,
    End,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBlock {
    pub role: MessageRole,
    pub phase: MessagePhase,
    pub text: String,
}

impl MessageBlock {
    pub fn start(role: MessageRole) -> Self {
        Self {
            role,
            phase: MessagePhase::Start,
            text: String::new(),
        }
    }

    pub fn delta(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Delta,
            text: text.into(),
        }
    }

    pub fn end(role: MessageRole) -> Self {
        Self {
            role,
            phase: MessagePhase::End,
            text: String::new(),
        }
    }

    pub fn full(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Full,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPhase {
    Start,
    Running,
    Done,
    Error,
}

/// Structured detail extracted from tool result metadata for rich rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStructuredDetail {
    FileEdit {
        file_path: String,
        diff_preview: Option<String>,
    },
    FileWrite {
        file_path: String,
        bytes: Option<u64>,
        lines: Option<u64>,
        diff_preview: Option<String>,
    },
    FileRead {
        file_path: String,
        total_lines: Option<u64>,
        truncated: bool,
    },
    BashExec {
        command_preview: String,
        exit_code: Option<i64>,
        output_preview: Option<String>,
        truncated: bool,
    },
    Search {
        pattern: String,
        matches: Option<u64>,
        truncated: bool,
    },
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBlock {
    pub name: String,
    pub phase: ToolPhase,
    pub detail: Option<String>,
    /// Structured data for rich rendering (Phase 2).
    /// Populated from tool result metadata when available.
    pub structured: Option<ToolStructuredDetail>,
}

impl ToolBlock {
    pub fn start(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Start,
            detail: None,
            structured: None,
        }
    }

    pub fn running(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Running,
            detail: Some(detail.into()),
            structured: None,
        }
    }

    pub fn done(name: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Done,
            detail,
            structured: None,
        }
    }

    pub fn error(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Error,
            detail: Some(detail.into()),
            structured: None,
        }
    }

    /// Attach structured detail for rich rendering.
    pub fn with_structured(mut self, detail: ToolStructuredDetail) -> Self {
        self.structured = Some(detail);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputBlock {
    Status(StatusBlock),
    Message(MessageBlock),
    Tool(ToolBlock),
}

pub fn render_cli_block(block: &OutputBlock) -> String {
    match block {
        OutputBlock::Status(status) => render_status_block(status),
        OutputBlock::Message(message) => render_message_block(message),
        OutputBlock::Tool(tool) => render_tool_block(tool),
    }
}

fn render_status_block(status: &StatusBlock) -> String {
    let label = match status.tone {
        BlockTone::Title => "STATUS",
        BlockTone::Normal => "status",
        BlockTone::Muted => "status",
        BlockTone::Success => "status+",
        BlockTone::Warning => "status!",
        BlockTone::Error => "status-",
    };
    format!("[{label}] {}\n", status.text)
}

fn render_message_block(message: &MessageBlock) -> String {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    };
    match message.phase {
        MessagePhase::Start => format!("[message:{role}] "),
        MessagePhase::Delta => message.text.clone(),
        MessagePhase::End => "\n".to_string(),
        MessagePhase::Full => format!("[message:{role}] {}\n", message.text),
    }
}

fn render_tool_block(tool: &ToolBlock) -> String {
    let phase = match tool.phase {
        ToolPhase::Start => "start",
        ToolPhase::Running => "running",
        ToolPhase::Done => "done",
        ToolPhase::Error => "error",
    };
    match &tool.detail {
        Some(detail) if !detail.trim().is_empty() => {
            format!("[tool:{phase}] {} :: {}\n", tool.name, detail)
        }
        _ => format!("[tool:{phase}] {}\n", tool.name),
    }
}

// ── Rich rendering ──────────────────────────────────────────────────

/// Render an `OutputBlock` with ANSI colors, icons, and structure.
/// Falls back to plain text when `style.color` is false.
pub fn render_cli_block_rich(block: &OutputBlock, style: &CliStyle) -> String {
    if !style.color {
        return render_cli_block(block);
    }
    match block {
        OutputBlock::Status(status) => render_status_rich(status, style),
        OutputBlock::Message(message) => render_message_rich(message, style),
        OutputBlock::Tool(tool) => render_tool_rich(tool, style),
    }
}

fn render_status_rich(status: &StatusBlock, style: &CliStyle) -> String {
    match status.tone {
        BlockTone::Title => {
            format!(
                "{} {}\n",
                style.bold_cyan(style.bullet()),
                style.bold(&status.text)
            )
        }
        BlockTone::Normal => {
            format!(
                "{} {}\n",
                style.dim(style.bullet()),
                style.dim(&status.text)
            )
        }
        BlockTone::Muted => {
            format!("  {}\n", style.dim(&status.text))
        }
        BlockTone::Success => {
            format!(
                "{} {}\n",
                style.bold_green(style.check()),
                style.green(&status.text)
            )
        }
        BlockTone::Warning => {
            format!(
                "{} {}\n",
                style.bold_yellow(style.warning_icon()),
                style.yellow(&status.text)
            )
        }
        BlockTone::Error => {
            format!(
                "{} {}\n",
                style.bold_red(style.cross()),
                style.red(&status.text)
            )
        }
    }
}

fn render_message_rich(message: &MessageBlock, style: &CliStyle) -> String {
    match message.phase {
        MessagePhase::Start => {
            // Start a new assistant response with a bullet
            format!("\n{} ", style.bold_cyan(style.bullet()))
        }
        MessagePhase::Delta => {
            // Streaming text — output as-is (streaming markdown handled by MarkdownStreamer in the run loop)
            message.text.clone()
        }
        MessagePhase::End => "\n\n".to_string(),
        MessagePhase::Full => {
            // Full message — render with markdown
            let rendered = cli_markdown::render_markdown(&message.text, style);
            format!(
                "\n{} {}\n\n",
                style.bold_cyan(style.bullet()),
                rendered.trim_end()
            )
        }
    }
}

fn render_tool_rich(tool: &ToolBlock, style: &CliStyle) -> String {
    match tool.phase {
        ToolPhase::Start => {
            let label = format_tool_header(tool);
            format!(
                "\n{} {}\n",
                style.bold_cyan(style.bullet()),
                style.bold(&label)
            )
        }
        ToolPhase::Running => {
            let detail = tool.detail.as_deref().unwrap_or("");
            if detail.is_empty() {
                String::new()
            } else {
                let collapsed = style.collapse_with_width(detail, 5, 2, None);
                format!(
                    "  {} {}\n",
                    style.dim(style.tree_end()),
                    style.dim(&collapsed)
                )
            }
        }
        ToolPhase::Done => render_tool_done_rich(tool, style),
        ToolPhase::Error => {
            let detail = tool.detail.as_deref().unwrap_or("unknown error");
            let collapsed = style.collapse(detail, 5, 2);
            format!(
                "  {} {}\n",
                style.tree_end(),
                style.red(&format!("Error: {}", collapsed))
            )
        }
    }
}

/// Rich rendering of completed tool results.
fn render_tool_done_rich(tool: &ToolBlock, style: &CliStyle) -> String {
    if let Some(ref structured) = tool.structured {
        match structured {
            ToolStructuredDetail::FileEdit {
                file_path: _,
                diff_preview,
            } => {
                if let Some(diff) = diff_preview {
                    let rendered_diff = render_diff_preview(diff, style);
                    return format!("  {} {}\n", style.tree_end(), rendered_diff);
                }
            }
            ToolStructuredDetail::FileWrite {
                file_path: _,
                bytes,
                lines,
                diff_preview,
            } => {
                let mut summary_parts = Vec::new();
                if let Some(l) = lines {
                    summary_parts.push(format!("{} lines", l));
                }
                if let Some(b) = bytes {
                    summary_parts.push(format!("{} bytes", b));
                }
                let summary = if summary_parts.is_empty() {
                    "written".to_string()
                } else {
                    format!("wrote {}", summary_parts.join(", "))
                };
                if let Some(diff) = diff_preview {
                    let rendered_diff = render_diff_preview(diff, style);
                    return format!(
                        "  {} {}\n{}\n",
                        style.tree_end(),
                        style.dim(&summary),
                        rendered_diff
                    );
                }
                return format!("  {} {}\n", style.tree_end(), style.dim(&summary));
            }
            ToolStructuredDetail::FileRead {
                file_path: _,
                total_lines,
                truncated,
            } => {
                let mut parts = Vec::new();
                if let Some(n) = total_lines {
                    parts.push(format!("{} lines", n));
                }
                if *truncated {
                    parts.push("truncated".to_string());
                }
                let summary = if parts.is_empty() {
                    "read".to_string()
                } else {
                    parts.join(", ")
                };
                return format!("  {} {}\n", style.tree_end(), style.dim(&summary));
            }
            ToolStructuredDetail::BashExec {
                command_preview: _,
                exit_code,
                output_preview,
                truncated,
            } => {
                let mut out = String::new();
                if let Some(preview) = output_preview {
                    let collapsed = style.collapse_with_width(preview, 5, 2, None);
                    out.push_str(&format!(
                        "  {} {}\n",
                        style.tree_end(),
                        style.dim(&collapsed)
                    ));
                }
                let exit_str = match exit_code {
                    Some(0) | None => style.green("exit 0"),
                    Some(code) => style.red(&format!("exit {}", code)),
                };
                let mut suffix = exit_str;
                if *truncated {
                    suffix.push_str(&style.dim(" (truncated)"));
                }
                out.push_str(&format!("  {} {}\n", style.tree_end(), suffix));
                return out;
            }
            ToolStructuredDetail::Search {
                pattern: _,
                matches,
                truncated,
            } => {
                let mut parts = Vec::new();
                if let Some(n) = matches {
                    parts.push(format!("{} matches", n));
                }
                if *truncated {
                    parts.push("truncated".to_string());
                }
                let summary = if parts.is_empty() {
                    "searched".to_string()
                } else {
                    parts.join(", ")
                };
                return format!("  {} {}\n", style.tree_end(), style.dim(&summary));
            }
            ToolStructuredDetail::Generic => {}
        }
    }

    // Fallback: no structured data
    let detail = tool.detail.as_deref().unwrap_or("");
    if detail.is_empty() {
        format!("  {} {}\n", style.tree_end(), style.green("Done"))
    } else {
        let collapsed = style.collapse_with_width(detail, 5, 2, None);
        format!("  {} {}\n", style.tree_end(), collapsed)
    }
}

/// Render a unified diff preview with ± color.
fn render_diff_preview(diff: &str, style: &CliStyle) -> String {
    let lines: Vec<&str> = diff.lines().collect();
    let mut out = Vec::new();
    let total = lines.len();
    let max_lines = 12;

    let visible: Vec<&str> = if total > max_lines {
        let mut v: Vec<&str> = lines[..max_lines].to_vec();
        v.push(&""); // placeholder for summary
        v
    } else {
        lines.clone()
    };

    for (i, line) in visible.iter().enumerate() {
        if total > max_lines && i == max_lines {
            out.push(format!(
                "     {}",
                style.dim(&format!("… +{} lines", total - max_lines))
            ));
            break;
        }
        let rendered = if line.starts_with('+') && !line.starts_with("+++") {
            format!("     {}", style.green(line))
        } else if line.starts_with('-') && !line.starts_with("---") {
            format!("     {}", style.red(line))
        } else if line.starts_with("@@") {
            format!("     {}", style.cyan(line))
        } else {
            format!("     {}", style.dim(line))
        };
        out.push(rendered);
    }
    out.join("\n")
}

/// Format tool header with arguments, e.g. `Edit(src/main.rs)` or `Bash(ls -la)`.
fn format_tool_header(tool: &ToolBlock) -> String {
    let display = tool_display_name(&tool.name);

    // Try to extract a meaningful argument from the detail/structured
    let arg = if let Some(ref structured) = tool.structured {
        match structured {
            ToolStructuredDetail::FileEdit { file_path, .. }
            | ToolStructuredDetail::FileWrite { file_path, .. }
            | ToolStructuredDetail::FileRead { file_path, .. } => Some(file_path.clone()),
            ToolStructuredDetail::BashExec {
                command_preview, ..
            } => {
                let truncated: String = command_preview.chars().take(60).collect();
                if truncated.len() < command_preview.len() {
                    Some(format!("{}…", truncated))
                } else {
                    Some(truncated)
                }
            }
            ToolStructuredDetail::Search { pattern, .. } => Some(pattern.clone()),
            ToolStructuredDetail::Generic => None,
        }
    } else {
        None
    };

    match arg {
        Some(a) => format!("{}({})", display, a),
        None => display,
    }
}

/// Convert internal tool ID to a human-readable display name.
fn tool_display_name(tool_id: &str) -> String {
    match tool_id {
        "read" => "Read".to_string(),
        "write" => "Write".to_string(),
        "edit" => "Edit".to_string(),
        "multiedit" => "MultiEdit".to_string(),
        "bash" => "Bash".to_string(),
        "glob" => "Glob".to_string(),
        "grep" => "Grep".to_string(),
        "ls" => "Ls".to_string(),
        "websearch" => "WebSearch".to_string(),
        "webfetch" => "WebFetch".to_string(),
        "task" => "Task".to_string(),
        "task_flow" => "TaskFlow".to_string(),
        "question" => "Question".to_string(),
        "todo_read" => "TodoRead".to_string(),
        "todo_write" => "TodoWrite".to_string(),
        "apply_patch" => "ApplyPatch".to_string(),
        "skill" => "Skill".to_string(),
        "lsp" => "LSP".to_string(),
        "batch" => "Batch".to_string(),
        "codesearch" => "CodeSearch".to_string(),
        "context_docs" => "ContextDocs".to_string(),
        "github_research" => "GitHubResearch".to_string(),
        "repo_history" => "RepoHistory".to_string(),
        "media_inspect" => "MediaInspect".to_string(),
        "browser_session" => "BrowserSession".to_string(),
        "shell_session" => "ShellSession".to_string(),
        "ast_grep_search" => "AstGrepSearch".to_string(),
        "ast_grep_replace" => "AstGrepReplace".to_string(),
        "plan_enter" => "PlanEnter".to_string(),
        "plan_exit" => "PlanExit".to_string(),
        other => {
            // CamelCase conversion for unknown tools
            let mut result = String::new();
            for (i, ch) in other.chars().enumerate() {
                if ch == '_' || ch == '-' {
                    continue;
                }
                if i == 0
                    || other.as_bytes().get(i.wrapping_sub(1)) == Some(&b'_')
                    || other.as_bytes().get(i.wrapping_sub(1)) == Some(&b'-')
                {
                    result.push(ch.to_uppercase().next().unwrap_or(ch));
                } else {
                    result.push(ch);
                }
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_status_blocks() {
        let line = render_cli_block(&OutputBlock::Status(StatusBlock::success("ready")));
        assert_eq!(line, "[status+] ready\n");
    }

    #[test]
    fn renders_message_blocks() {
        let start = render_cli_block(&OutputBlock::Message(MessageBlock::start(
            MessageRole::Assistant,
        )));
        let delta = render_cli_block(&OutputBlock::Message(MessageBlock::delta(
            MessageRole::Assistant,
            "hello",
        )));
        let end = render_cli_block(&OutputBlock::Message(MessageBlock::end(
            MessageRole::Assistant,
        )));
        assert_eq!(start, "[message:assistant] ");
        assert_eq!(delta, "hello");
        assert_eq!(end, "\n");
    }

    #[test]
    fn renders_tool_blocks() {
        let line = render_cli_block(&OutputBlock::Tool(ToolBlock::error("bash", "exit=1")));
        assert_eq!(line, "[tool:error] bash :: exit=1\n");
    }

    // ── Rich rendering tests ────────────────────────────────────

    #[test]
    fn rich_status_title_has_bullet() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(&OutputBlock::Status(StatusBlock::title("Hello")), &style);
        assert!(out.contains("●"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn rich_status_success_has_check() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(&OutputBlock::Status(StatusBlock::success("Done")), &style);
        assert!(out.contains("✔"));
        assert!(out.contains("Done"));
    }

    #[test]
    fn rich_status_error_has_cross() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(&OutputBlock::Status(StatusBlock::error("fail")), &style);
        assert!(out.contains("✗"));
        assert!(out.contains("fail"));
    }

    #[test]
    fn rich_tool_start_capitalized() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(&OutputBlock::Tool(ToolBlock::start("edit")), &style);
        assert!(out.contains("Edit"));
        assert!(out.contains("●"));
    }

    #[test]
    fn rich_tool_error_red() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(
            &OutputBlock::Tool(ToolBlock::error("bash", "exit code 1")),
            &style,
        );
        assert!(out.contains("⎿"));
        assert!(out.contains("Error:"));
    }

    #[test]
    fn rich_message_start_has_bullet() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_cli_block_rich(
            &OutputBlock::Message(MessageBlock::start(MessageRole::Assistant)),
            &style,
        );
        assert!(out.contains("●"));
    }

    #[test]
    fn rich_fallback_to_plain_when_no_color() {
        let style = CliStyle::plain();
        let out = render_cli_block_rich(&OutputBlock::Status(StatusBlock::success("ok")), &style);
        assert_eq!(out, "[status+] ok\n");
    }

    #[test]
    fn tool_display_name_maps_known_tools() {
        assert_eq!(tool_display_name("bash"), "Bash");
        assert_eq!(tool_display_name("ast_grep_search"), "AstGrepSearch");
        assert_eq!(tool_display_name("websearch"), "WebSearch");
    }

    #[test]
    fn tool_display_name_converts_unknown() {
        assert_eq!(tool_display_name("my_custom_tool"), "MyCustomTool");
        assert_eq!(tool_display_name("something"), "Something");
    }
}
