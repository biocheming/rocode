use std::collections::HashMap;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

use rocode_core::contracts::output_blocks::keys as output_keys;
use rocode_core::contracts::output_blocks::DisplayModeWire;
use rocode_core::contracts::patch::{keys as patch_keys, FileChangeType};
use rocode_core::contracts::task::{
    metadata_keys as task_metadata_keys, TaskResultEnvelope, TASK_STATUS_COMPLETED,
};
use rocode_core::contracts::todo::keys as todo_keys;
use rocode_core::contracts::tools::{arg_keys as tool_arg_keys, BuiltinToolName};
use rocode_core::contracts::wire::fields as wire_fields;

use super::markdown::MarkdownRenderer;
use crate::theme::Theme;

/// Rich tool result info that carries title and metadata through the rendering pipeline.
#[derive(Clone, Debug, Default)]
pub struct ToolResultInfo {
    pub output: String,
    pub is_error: bool,
    pub title: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Clone, Copy)]
pub enum ToolState {
    Pending,
    Running,
    Completed,
    Failed,
}

/// Threshold: tool results longer than this are "block" tools with expandable output
const BLOCK_RESULT_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Default)]
struct ReadSummary {
    size_bytes: Option<usize>,
    total_lines: Option<usize>,
}

#[derive(Debug, Clone, Default)]
struct WriteSummary {
    size_bytes: Option<usize>,
    total_lines: Option<usize>,
    path: Option<String>,
    verb: Option<&'static str>,
}

/// Map tool name to a semantic glyph
pub fn tool_glyph(name: &str) -> &'static str {
    let normalized = normalize_tool_name(name);
    if normalized == "subagent" {
        return "#";
    }

    match BuiltinToolName::parse(normalized.as_str()) {
        Some(BuiltinToolName::Bash) => "$",
        Some(BuiltinToolName::Read | BuiltinToolName::Ls) => "→",
        Some(BuiltinToolName::Write | BuiltinToolName::Edit | BuiltinToolName::MultiEdit) => "←",
        Some(BuiltinToolName::Glob | BuiltinToolName::Grep) => "✱",
        Some(BuiltinToolName::WebFetch) => "%",
        Some(BuiltinToolName::CodeSearch) => "◇",
        Some(BuiltinToolName::WebSearch) => "◈",
        Some(BuiltinToolName::Task | BuiltinToolName::TaskFlow) => "#",
        Some(BuiltinToolName::ApplyPatch) => "%",
        Some(BuiltinToolName::Skill) => "⚙",
        Some(BuiltinToolName::Batch) => "⫘",
        Some(BuiltinToolName::Question) => "?",
        Some(BuiltinToolName::TodoWrite | BuiltinToolName::TodoRead) => "☐",
        _ => "⚙",
    }
}

/// Returns true if this tool typically produces block-level output
fn is_block_tool(name: &str, result: Option<&ToolResultInfo>) -> bool {
    // Check display.mode override from metadata
    if let Some(info) = result {
        if let Some(mode) = info
            .metadata
            .as_ref()
            .and_then(|m| m.get(output_keys::DISPLAY_MODE))
            .and_then(|v| v.as_str())
        {
            if DisplayModeWire::parse(mode) == Some(DisplayModeWire::Block) {
                return true;
            }
        }
    }

    let normalized = normalize_tool_name(name);
    // Tools that always produce block output
    match BuiltinToolName::parse(normalized.as_str()) {
        Some(
            BuiltinToolName::Bash
            | BuiltinToolName::ApplyPatch
            | BuiltinToolName::Batch
            | BuiltinToolName::Question
            | BuiltinToolName::Task
            | BuiltinToolName::TaskFlow
            | BuiltinToolName::TodoWrite,
        ) => return true,
        Some(BuiltinToolName::Skill) => return false,
        _ => {}
    }
    // edit/write tools with diff metadata are block-level
    if is_write_tool(&normalized) || is_edit_tool(&normalized) {
        if let Some(info) = result {
            if info
                .metadata
                .as_ref()
                .and_then(|m| m.get(patch_keys::DIFF))
                .and_then(|v| v.as_str())
                .is_some_and(|d| !d.is_empty())
            {
                return true;
            }
        }
    }
    // Otherwise, check result length
    if let Some(info) = result {
        info.output.lines().count() > BLOCK_RESULT_THRESHOLD
    } else {
        false
    }
}

fn is_read_tool(normalized_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(normalized_name),
        Some(BuiltinToolName::Read)
    )
}

fn is_list_tool(normalized_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(normalized_name),
        Some(BuiltinToolName::Ls)
    )
}

fn is_write_tool(normalized_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(normalized_name),
        Some(BuiltinToolName::Write)
    )
}

fn is_edit_tool(normalized_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(normalized_name),
        Some(BuiltinToolName::Edit | BuiltinToolName::MultiEdit)
    )
}

fn is_patch_tool(normalized_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(normalized_name),
        Some(BuiltinToolName::ApplyPatch)
    )
}

fn split_list_output<'a>(lines: &'a [&'a str]) -> (Option<&'a str>, Vec<&'a str>) {
    if lines.is_empty() {
        return (None, Vec::new());
    }
    let first = lines[0].trim();
    if first.starts_with('/') && first.ends_with('/') {
        (Some(first), lines[1..].to_vec())
    } else {
        (None, lines.to_vec())
    }
}

/// Render a single tool call as lines (inline or block style)
pub fn render_tool_call(
    id: &str,
    name: &str,
    arguments: &str,
    state: ToolState,
    tool_results: &HashMap<String, ToolResultInfo>,
    show_tool_details: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let normalized = normalize_tool_name(name);
    let tool_kind = BuiltinToolName::parse(normalized.as_str());
    if matches!(state, ToolState::Completed)
        && !show_tool_details
        && !matches!(
            tool_kind,
            Some(BuiltinToolName::Task | BuiltinToolName::TodoWrite)
        )
    {
        return Vec::new();
    }

    let result = tool_results.get(id);
    let block_mode = is_block_tool(name, result);
    let read_summary = if is_read_tool(&normalized) {
        result.and_then(|info| {
            if info.is_error {
                None
            } else {
                Some(parse_read_summary(&info.output))
            }
        })
    } else {
        None
    };

    let glyph = tool_glyph(name);
    let is_denied = result.is_some_and(|info| info.is_error && is_denied_result(&info.output));

    let (state_icon, icon_style, name_style) = styles_for_state(state, is_denied, theme);

    let mut lines = Vec::new();

    if block_mode {
        let bg = theme.background_panel;
        let mut main_spans = vec![
            block_prefix(theme, bg),
            Span::styled(format!("{} ", state_icon), icon_style.bg(bg)),
            Span::styled(format!("{} ", glyph), icon_style.bg(bg)),
            Span::styled(name.to_string(), name_style.bg(bg)),
        ];

        let argument_preview = tool_argument_preview(&normalized, arguments);
        if let Some(ref preview) = argument_preview {
            main_spans.push(Span::styled(
                format!("  {}", preview),
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        } else if let Some(title) = result
            .and_then(|info| info.title.as_deref())
            .filter(|t| !t.is_empty())
        {
            main_spans.push(Span::styled(
                format!("  {}", format_preview_line(title, 60)),
                Style::default().fg(theme.text_muted).bg(bg),
            ));
        }
        if let Some(summary) = read_summary.as_ref() {
            if let Some(compact) = format_read_summary(summary) {
                main_spans.push(Span::styled(
                    format!("  [{}]", compact),
                    Style::default().fg(theme.text_muted).bg(bg),
                ));
            }
        }

        if is_denied {
            main_spans.push(Span::styled(
                "  denied",
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD)
                    .bg(bg),
            ));
        }

        lines.push(Line::from(main_spans));

        if let Some(info) = result {
            let result_text = &info.output;
            let is_error = info.is_error;

            if is_error {
                let mut iter = result_text.lines().filter(|line| !line.trim().is_empty());
                if let Some(first_line) = iter.next() {
                    lines.push(block_content_line(
                        format!("Error: {}", format_preview_line(first_line, 96)),
                        Style::default().fg(theme.error),
                        theme,
                        bg,
                    ));
                }

                let extra_error_lines = if show_tool_details { 4 } else { 2 };
                for line in iter.take(extra_error_lines) {
                    lines.push(block_content_line(
                        format_preview_line(line, 96),
                        Style::default().fg(theme.error),
                        theme,
                        bg,
                    ));
                }
            } else if render_display_hints(info, theme, bg, &mut lines) {
                // Display hints handled the rendering
            } else if matches!(tool_kind, Some(BuiltinToolName::Task)) {
                render_task_result_block(
                    result_text,
                    arguments,
                    info.metadata.as_ref(),
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if matches!(tool_kind, Some(BuiltinToolName::TodoWrite)) {
                render_todowrite_result_block(
                    result_text,
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if is_write_tool(&normalized) {
                render_write_result_block(
                    result_text,
                    arguments,
                    info.metadata.as_ref(),
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if is_edit_tool(&normalized) {
                render_edit_result_block(
                    result_text,
                    arguments,
                    info.metadata.as_ref(),
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if is_patch_tool(&normalized) {
                render_patch_result_block(
                    result_text,
                    arguments,
                    info.metadata.as_ref(),
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if is_read_tool(&normalized) {
                // Read output is very large and noisy; keep it summarized in the header only.
            } else if matches!(tool_kind, Some(BuiltinToolName::Batch)) {
                render_batch_result_block(
                    result_text,
                    arguments,
                    show_tool_details,
                    theme,
                    bg,
                    &mut lines,
                );
            } else if matches!(tool_kind, Some(BuiltinToolName::Question)) {
                render_question_result_block(result_text, arguments, theme, bg, &mut lines);
            } else if show_tool_details {
                let output_lines = result_text.lines().collect::<Vec<_>>();
                let (list_root, list_entries) = if is_list_tool(&normalized) {
                    split_list_output(&output_lines)
                } else {
                    (None, output_lines.clone())
                };
                let line_count = list_entries.len();
                let mut preview_limit = if matches!(tool_kind, Some(BuiltinToolName::Bash)) {
                    10usize
                } else if is_list_tool(&normalized) {
                    40usize
                } else {
                    6usize
                };
                if line_count.saturating_sub(preview_limit) <= 2 {
                    preview_limit = line_count;
                }

                if let Some(root) = list_root {
                    lines.push(block_content_line(
                        format!("[Directory]: {}", root),
                        Style::default()
                            .fg(theme.info)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                        theme,
                        bg,
                    ));
                }

                lines.push(block_content_line(
                    if is_list_tool(&normalized) {
                        format!("({} files)", line_count)
                    } else {
                        format!("({} lines of output)", line_count)
                    },
                    Style::default().fg(theme.text_muted),
                    theme,
                    bg,
                ));

                for line in list_entries.iter().take(preview_limit) {
                    lines.push(block_content_line(
                        format_preview_line(line, 96),
                        Style::default().fg(theme.text),
                        theme,
                        bg,
                    ));
                }

                if line_count > preview_limit {
                    lines.push(block_content_line(
                        format!("… ({} more lines)", line_count - preview_limit),
                        Style::default().fg(theme.text_muted),
                        theme,
                        bg,
                    ));
                }
            }
        } else if matches!(tool_kind, Some(BuiltinToolName::Task))
            && matches!(state, ToolState::Pending | ToolState::Running)
        {
            render_task_running_block(arguments, theme, bg, &mut lines);
        }

        return lines;
    }

    // Inline mode
    let mut main_spans = vec![
        Span::styled(format!("{} ", state_icon), icon_style),
        Span::styled(format!("{} ", glyph), Style::default().fg(theme.tool_icon)),
        Span::styled(name.to_string(), name_style),
    ];

    // Argument preview on the same line as tool name (e.g. "◯ → ls → .")
    if let Some(argument_preview) = tool_argument_preview(&normalized, arguments) {
        main_spans.push(Span::styled(
            format!("  {}", argument_preview),
            Style::default().fg(theme.text_muted),
        ));
    }

    // Inline result summary for completed non-block tools
    if let Some(info) = result {
        if info.is_error {
            let first_line = info.output.lines().next().unwrap_or(&info.output).trim();
            main_spans.push(Span::styled(
                format!(" — {}", format_preview_line(first_line, 96)),
                Style::default().fg(theme.error),
            ));
            if is_denied {
                main_spans.push(Span::styled(
                    " (denied)",
                    Style::default()
                        .fg(theme.error)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            // Check for display.summary override
            let display_summary = info
                .metadata
                .as_ref()
                .and_then(|m| m.get(output_keys::DISPLAY_SUMMARY))
                .and_then(|v| v.as_str());

            if let Some(summary) = display_summary {
                main_spans.push(Span::styled(
                    format!(" — {}", format_preview_line(summary, 80)),
                    Style::default().fg(theme.text_muted),
                ));
            } else {
                let result_text = &info.output;
                if is_write_tool(&normalized) {
                    if let Some(write_summary) = parse_write_summary(result_text) {
                        let mut summary_parts = Vec::new();
                        if let Some(size_bytes) = write_summary.size_bytes {
                            summary_parts.push(format_bytes(size_bytes));
                        }
                        if let Some(total_lines) = write_summary.total_lines {
                            summary_parts.push(format!("{} lines", total_lines));
                        }
                        let verb = write_summary.verb.unwrap_or("updated");
                        let summary_text = if summary_parts.is_empty() {
                            if let Some(path) = write_summary.path.as_deref() {
                                format!("{} {}", verb, path)
                            } else {
                                verb.to_string()
                            }
                        } else {
                            format!("{} · {}", verb, summary_parts.join(" · "))
                        };
                        main_spans.push(Span::styled(
                            format!(" — {}", summary_text),
                            Style::default().fg(theme.success),
                        ));
                    }
                } else {
                    let line_count = result_text.lines().count();
                    if line_count <= 1 {
                        let summary = result_text.trim();
                        if !summary.is_empty() && summary.len() <= 80 {
                            main_spans.push(Span::styled(
                                format!(" — {}", summary),
                                Style::default().fg(theme.text_muted),
                            ));
                        }
                    } else if let Some(first_line) =
                        result_text.lines().find(|line| !line.trim().is_empty())
                    {
                        main_spans.push(Span::styled(
                            format!(
                                " — {} (+{} lines)",
                                format_preview_line(first_line, 72),
                                line_count.saturating_sub(1)
                            ),
                            Style::default().fg(theme.text_muted),
                        ));
                    }
                }
            }
        }
    }

    lines.push(Line::from(main_spans));

    lines
}

/// Render structured display hints from tool metadata.
/// Returns true if any display hints were rendered, false to fall through to default rendering.
fn render_display_hints(
    info: &ToolResultInfo,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) -> bool {
    let metadata = match info.metadata.as_ref() {
        Some(m) => m,
        None => return false,
    };

    let has_fields = metadata.contains_key(output_keys::DISPLAY_FIELDS);
    let has_summary = metadata.contains_key(output_keys::DISPLAY_SUMMARY);

    if !has_fields && !has_summary {
        return false;
    }

    // Render display.summary as the summary line
    if let Some(summary) = metadata
        .get(output_keys::DISPLAY_SUMMARY)
        .and_then(|v| v.as_str())
    {
        lines.push(block_content_line(
            format_preview_line(summary, 96),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    // Render display.fields as key-value pairs
    if let Some(fields) = metadata
        .get(output_keys::DISPLAY_FIELDS)
        .and_then(|v| v.as_array())
    {
        for field in fields {
            let key = field
                .get(output_keys::DISPLAY_FIELD_KEY)
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let value = field
                .get(output_keys::DISPLAY_FIELD_VALUE)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            lines.push(block_content_line(
                format!("{}: {}", key, format_preview_line(value, 88 - key.len())),
                Style::default().fg(theme.text),
                theme,
                bg,
            ));
        }
    }

    true
}

/// Render batch tool results as a list of sub-tool entries instead of raw JSON.
fn render_batch_result_block(
    result_text: &str,
    arguments: &str,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    // Parse sub-tool names from arguments for labeling
    let arg_parsed = serde_json::from_str::<Value>(arguments).ok();
    let calls = arg_parsed
        .as_ref()
        .and_then(|v| {
            v.get(tool_arg_keys::TOOL_CALLS_CAMEL)
                .or_else(|| v.get(tool_arg_keys::TOOL_CALLS))
        })
        .and_then(|v| v.as_array());

    // Try to parse the result as JSON array.
    // The batch tool output is: "All N tools...\n\nResults:\n[{...}]"
    // so we need to extract the JSON after "Results:\n".
    let json_text = result_text
        .find("Results:\n")
        .map(|pos| &result_text[pos + "Results:\n".len()..])
        .unwrap_or(result_text);
    let result_parsed = serde_json::from_str::<Value>(json_text).ok();
    let result_array = result_parsed.as_ref().and_then(|v| {
        v.as_array()
            .or_else(|| v.get(tool_arg_keys::RESULTS).and_then(|r| r.as_array()))
    });

    if let Some(results) = result_array {
        let total = results.len();
        let ok_count = results
            .iter()
            .filter(|r| {
                r.get(tool_arg_keys::SUCCESS)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true)
            })
            .count();
        let fail_count = total - ok_count;

        // Summary line: "5 tools: 5 ok" or "5 tools: 3 ok, 2 failed"
        let summary = if fail_count == 0 {
            format!("{} tools: all ok", total)
        } else {
            format!("{} tools: {} ok, {} failed", total, ok_count, fail_count)
        };
        let summary_color = if fail_count > 0 {
            theme.warning
        } else {
            theme.text_muted
        };
        lines.push(block_content_line(
            summary,
            Style::default().fg(summary_color),
            theme,
            bg,
        ));

        if !show_tool_details {
            return;
        }

        // Render each sub-tool as a mini entry
        for (i, result_entry) in results.iter().enumerate() {
            let sub_name = calls
                .and_then(|c| c.get(i))
                .and_then(|c| {
                    c.get(tool_arg_keys::TOOL)
                        .or_else(|| c.get(tool_arg_keys::NAME))
                        .or_else(|| c.get(tool_arg_keys::TOOL_NAME))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("?");
            let sub_glyph = tool_glyph(sub_name);

            let is_ok = result_entry
                .get(tool_arg_keys::SUCCESS)
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let (icon, icon_color) = if is_ok {
                ("●", theme.success)
            } else {
                ("✗", theme.error)
            };

            // Extract a short preview of the sub-tool result
            let sub_result = result_entry
                .get(tool_arg_keys::OUTPUT)
                .or_else(|| result_entry.get(tool_arg_keys::RESULT))
                .or_else(|| result_entry.get(tool_arg_keys::ERROR))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let first_line = sub_result
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            let line_count = sub_result.lines().count();

            let mut spans = vec![
                block_prefix(theme, bg),
                Span::styled("  ", Style::default().bg(bg)),
                Span::styled(format!("{} ", icon), Style::default().fg(icon_color).bg(bg)),
                Span::styled(
                    format!("{} ", sub_glyph),
                    Style::default().fg(theme.tool_icon).bg(bg),
                ),
                Span::styled(
                    sub_name.to_string(),
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD)
                        .bg(bg),
                ),
            ];

            // Add sub-tool argument preview if available
            if let Some(call_args) = calls.and_then(|c| c.get(i)) {
                let sub_args_str = call_args
                    .get(tool_arg_keys::PARAMETERS)
                    .map(|v| v.to_string());
                if let Some(ref args_json) = sub_args_str {
                    let sub_normalized = normalize_tool_name(sub_name);
                    if let Some(preview) = tool_argument_preview(&sub_normalized, args_json) {
                        spans.push(Span::styled(
                            format!("  {}", format_preview_line(&preview, 40)),
                            Style::default().fg(theme.text_muted).bg(bg),
                        ));
                    }
                }
            }

            // Add result summary
            if !is_ok {
                let err_preview = format_preview_line(first_line, 48);
                if !err_preview.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", err_preview),
                        Style::default().fg(theme.error).bg(bg),
                    ));
                }
            } else if line_count > 1 {
                spans.push(Span::styled(
                    format!("  (+{} lines)", line_count),
                    Style::default().fg(theme.text_muted).bg(bg),
                ));
            }

            lines.push(Line::from(spans));
        }
    } else if show_tool_details {
        // Fallback: couldn't parse as structured batch result, show raw lines
        let output_lines: Vec<&str> = result_text.lines().collect();
        let line_count = output_lines.len();
        lines.push(block_content_line(
            format!("({} lines of output)", line_count),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
        for line in output_lines.iter().take(8) {
            lines.push(block_content_line(
                format_preview_line(line, 96),
                Style::default().fg(theme.text),
                theme,
                bg,
            ));
        }
        if line_count > 8 {
            lines.push(block_content_line(
                format!("… ({} more lines)", line_count - 8),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }
}

/// Render question tool results: show each Q&A pair instead of raw JSON.
fn render_question_result_block(
    result_text: &str,
    arguments: &str,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    // Parse questions from arguments
    let arg_parsed = serde_json::from_str::<Value>(arguments).ok();
    let questions = arg_parsed
        .as_ref()
        .and_then(|v| v.get(wire_fields::QUESTIONS))
        .and_then(|v| v.as_array());

    // Parse answers from result
    let result_parsed = serde_json::from_str::<Value>(result_text).ok();
    let answers = result_parsed
        .as_ref()
        .and_then(|v| v.get(tool_arg_keys::ANSWERS))
        .and_then(|v| v.as_array());

    if let Some(qs) = questions {
        for (i, q) in qs.iter().enumerate() {
            let q_text = q
                .get(wire_fields::QUESTION)
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            // Show question
            lines.push(block_content_line(
                format!("Q: {}", format_preview_line(q_text, 88)),
                Style::default().fg(theme.info),
                theme,
                bg,
            ));
            // Show options if any
            if let Some(opts) = q.get(wire_fields::OPTIONS).and_then(|v| v.as_array()) {
                for opt in opts.iter() {
                    let label = opt
                        .get(wire_fields::LABEL)
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let desc = opt.get(tool_arg_keys::DESCRIPTION).and_then(|v| v.as_str());
                    let opt_text = match desc {
                        Some(d) => format!("  · {} — {}", label, format_preview_line(d, 64)),
                        None => format!("  · {}", label),
                    };
                    lines.push(block_content_line(
                        opt_text,
                        Style::default().fg(theme.text_muted),
                        theme,
                        bg,
                    ));
                }
            }
            // Show answer
            let answer = answers
                .and_then(|a| a.get(i))
                .and_then(|v| v.as_str())
                .unwrap_or("(no answer)");
            lines.push(block_content_line(
                format!("A: {}", format_preview_line(answer, 88)),
                Style::default().fg(theme.success),
                theme,
                bg,
            ));
        }
    } else {
        // Fallback: just show the raw result compactly
        let first_line = result_text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(result_text);
        lines.push(block_content_line(
            format_preview_line(first_line, 88),
            Style::default().fg(theme.text),
            theme,
            bg,
        ));
    }
}

fn render_write_result_block(
    result_text: &str,
    arguments: &str,
    metadata: Option<&HashMap<String, serde_json::Value>>,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    let args_parsed = serde_json::from_str::<Value>(arguments).ok();
    let write_summary = parse_write_summary(result_text);
    let write_path = args_parsed
        .as_ref()
        .and_then(extract_path)
        .or_else(|| extract_jsonish_path_from_raw(arguments))
        .or_else(|| {
            write_summary
                .as_ref()
                .and_then(|summary| summary.path.clone())
        });

    lines.push(block_content_line(
        "✦ Write Complete",
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
        theme,
        bg,
    ));

    if let Some(path) = write_path {
        lines.push(block_content_line(
            format!("File: {}", path),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
            theme,
            bg,
        ));
    }

    if let Some(summary) = write_summary.as_ref() {
        let mut stats = Vec::new();
        if let Some(size_bytes) = summary.size_bytes {
            stats.push(format!("Size {}", format_bytes(size_bytes)));
        }
        if let Some(total_lines) = summary.total_lines {
            stats.push(format!("Lines {}", total_lines));
        }
        if !stats.is_empty() {
            lines.push(block_content_line(
                stats.join("  ·  "),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }

    if show_tool_details {
        // Render inline diff from metadata if available
        if let Some(diff_str) = metadata
            .and_then(|m| m.get(patch_keys::DIFF))
            .and_then(|v| v.as_str())
            .filter(|d| !d.is_empty())
        {
            render_inline_diff(diff_str, theme, bg, lines);
        } else if let Some(first_line) = result_text.lines().find(|line| !line.trim().is_empty()) {
            lines.push(block_content_line(
                format_preview_line(first_line, 96),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }
}

/// Maximum diff lines shown inline before truncation.
const INLINE_DIFF_MAX_LINES: usize = 12;

/// Render diff content inline in the message flow, with truncation.
fn render_inline_diff(
    diff_str: &str,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    use super::diff::DiffView;

    let diff_view = DiffView::new().with_content(diff_str);
    let diff_lines = diff_view.to_lines(theme);
    let total = diff_lines.len();

    for diff_line in diff_lines.into_iter().take(INLINE_DIFF_MAX_LINES) {
        // Wrap each diff line with the block prefix for consistent indentation
        let mut spans = vec![block_prefix(theme, bg)];
        spans.extend(
            diff_line
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content, s.style.bg(bg))),
        );
        lines.push(Line::from(spans));
    }

    if total > INLINE_DIFF_MAX_LINES {
        lines.push(block_content_line(
            format!("… (+{} more diff lines)", total - INLINE_DIFF_MAX_LINES),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }
}

/// Render edit tool result block with inline diff.
fn render_edit_result_block(
    result_text: &str,
    arguments: &str,
    metadata: Option<&HashMap<String, serde_json::Value>>,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    let args_parsed = serde_json::from_str::<Value>(arguments).ok();
    let edit_path = args_parsed
        .as_ref()
        .and_then(extract_path)
        .or_else(|| extract_jsonish_path_from_raw(arguments));

    lines.push(block_content_line(
        "✦ Edit Complete",
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
        theme,
        bg,
    ));

    if let Some(path) = edit_path {
        lines.push(block_content_line(
            format!("File: {}", path),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
            theme,
            bg,
        ));
    }

    // Show replacement count from metadata if available
    if let Some(replacements) = metadata
        .and_then(|m| m.get(patch_keys::REPLACEMENTS))
        .and_then(|v| v.as_u64())
    {
        lines.push(block_content_line(
            format!("{} replacement(s)", replacements),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    // Show diagnostics warning if present
    if let Some(diags) = metadata
        .and_then(|m| m.get(patch_keys::DIAGNOSTICS))
        .and_then(|v| v.as_array())
    {
        if !diags.is_empty() {
            lines.push(block_content_line(
                format!("⚠ {} diagnostic(s)", diags.len()),
                Style::default().fg(theme.warning),
                theme,
                bg,
            ));
        }
    }

    if show_tool_details {
        if let Some(diff_str) = metadata
            .and_then(|m| m.get(patch_keys::DIFF))
            .and_then(|v| v.as_str())
            .filter(|d| !d.is_empty())
        {
            render_inline_diff(diff_str, theme, bg, lines);
        } else if let Some(first_line) = result_text.lines().find(|line| !line.trim().is_empty()) {
            lines.push(block_content_line(
                format_preview_line(first_line, 96),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }
}

/// Render apply_patch tool result block with inline diff.
fn render_patch_result_block(
    result_text: &str,
    _arguments: &str,
    metadata: Option<&HashMap<String, serde_json::Value>>,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    // Extract file list from metadata
    let files: Vec<String> = metadata
        .and_then(|m| m.get(patch_keys::FILES))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    // files metadata is array of objects with "path" key, or strings
                    f.as_str().map(String::from).or_else(|| {
                        f.get(patch_keys::LEGACY_PATH)
                            .and_then(|p| p.as_str())
                            .map(String::from)
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    lines.push(block_content_line(
        format!("✦ Patch Applied — {} file(s)", files.len().max(1)),
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
        theme,
        bg,
    ));

    // List affected files
    for file in files.iter().take(8) {
        lines.push(block_content_line(
            format!("  {}", file),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
    }
    if files.len() > 8 {
        lines.push(block_content_line(
            format!("  … (+{} more files)", files.len() - 8),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    // Show diagnostics warning if present
    if let Some(diags) = metadata
        .and_then(|m| m.get(patch_keys::DIAGNOSTICS))
        .and_then(|v| v.as_array())
    {
        if !diags.is_empty() {
            lines.push(block_content_line(
                format!("⚠ {} diagnostic(s)", diags.len()),
                Style::default().fg(theme.warning),
                theme,
                bg,
            ));
        }
    }

    if show_tool_details {
        // Try per-file diffs first (richer display with headers)
        let per_file_diffs: Vec<(String, String, String)> = metadata
            .and_then(|m| m.get(patch_keys::FILES))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        let path = f
                            .get(patch_keys::RELATIVE_PATH)
                            .or_else(|| f.get(patch_keys::LEGACY_PATH))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string();
                        let change_type = f
                            .get(patch_keys::CHANGE_TYPE)
                            .and_then(|v| v.as_str())
                            .unwrap_or(FileChangeType::Update.as_str())
                            .to_string();
                        let diff = f
                            .get(patch_keys::FILE_DIFF)
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if diff.is_empty() {
                            None
                        } else {
                            Some((path, change_type, diff.to_string()))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !per_file_diffs.is_empty() {
            for (path, change_type, diff_str) in &per_file_diffs {
                let label = match FileChangeType::parse(change_type) {
                    Some(FileChangeType::Add) => format!("# Created {}", path),
                    Some(FileChangeType::Delete) => format!("# Deleted {}", path),
                    Some(FileChangeType::Move) => format!("# Moved {}", path),
                    Some(FileChangeType::Update) | None => format!("← Patched {}", path),
                };
                lines.push(block_content_line(
                    label,
                    Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
                    theme,
                    bg,
                ));
                render_inline_diff(diff_str, theme, bg, lines);
            }
        } else if let Some(diff_str) = metadata
            .and_then(|m| m.get(patch_keys::DIFF))
            .and_then(|v| v.as_str())
            .filter(|d| !d.is_empty())
        {
            render_inline_diff(diff_str, theme, bg, lines);
        } else if let Some(first_line) = result_text.lines().find(|line| !line.trim().is_empty()) {
            lines.push(block_content_line(
                format_preview_line(first_line, 96),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }
}

#[derive(Debug, Clone, Default)]
struct TaskResultSummary {
    task_id: Option<String>,
    task_status: Option<String>,
    body: String,
}

fn parse_task_result_summary(result_text: &str) -> TaskResultSummary {
    let envelope = TaskResultEnvelope::parse(result_text);
    TaskResultSummary {
        task_id: envelope.task_id,
        task_status: envelope.task_status,
        body: envelope.body,
    }
}

#[derive(Debug, Clone, Default)]
struct TaskArgumentSummary {
    category: Option<String>,
    subagent_type: Option<String>,
    description: Option<String>,
    prompt_preview: Option<String>,
    skill_count: Option<usize>,
    checklist: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChecklistItem {
    checked: bool,
    text: String,
}

fn parse_markdown_checklist(text: &str) -> Vec<ChecklistItem> {
    fn parse_line(line: &str) -> Option<ChecklistItem> {
        let trimmed = line.trim();
        let (checked, rest) = if let Some(rest) = trimmed.strip_prefix("- [x]") {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [X]") {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("- [ ]") {
            (false, rest)
        } else {
            return None;
        };
        let text = rest.trim();
        if text.is_empty() {
            return None;
        }
        Some(ChecklistItem {
            checked,
            text: text.to_string(),
        })
    }

    text.lines().filter_map(parse_line).collect()
}

fn parse_task_argument_summary(arguments: &str) -> TaskArgumentSummary {
    let mut summary = TaskArgumentSummary::default();
    let raw = arguments.trim();
    if raw.is_empty() {
        return summary;
    }

    let parsed = serde_json::from_str::<Value>(raw)
        .ok()
        .or_else(|| rocode_util::json::try_parse_json_object_robust(raw));
    let Some(value) = parsed.as_ref() else {
        return summary;
    };

    summary.category = extract_string_key(value, &[tool_arg_keys::CATEGORY]);
    summary.subagent_type = extract_string_key(
        value,
        &[
            tool_arg_keys::SUBAGENT_TYPE,
            tool_arg_keys::SUBAGENT_TYPE_CAMEL,
        ],
    );
    summary.description = extract_string_key(value, &[tool_arg_keys::DESCRIPTION]);
    let prompt = extract_string_key(value, &[tool_arg_keys::PROMPT]);
    if let Some(prompt_text) = prompt.as_deref() {
        summary.checklist = parse_markdown_checklist(prompt_text)
            .into_iter()
            .map(|item| item.text)
            .collect();
    }
    summary.prompt_preview = prompt.and_then(|prompt| {
        prompt
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("- ["))
            .or_else(|| prompt.lines().map(str::trim).find(|line| !line.is_empty()))
            .map(|line| format_preview_line(line, 88))
    });
    summary.skill_count = value
        .get(tool_arg_keys::LOAD_SKILLS)
        .or_else(|| value.get(tool_arg_keys::LOADED_SKILLS))
        .and_then(|v| v.as_array())
        .map(Vec::len);

    summary
}

fn render_task_running_block(
    arguments: &str,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    lines.push(block_content_line(
        "Delegating task to subagent…",
        Style::default().fg(theme.warning),
        theme,
        bg,
    ));

    let summary = parse_task_argument_summary(arguments);
    let subagent = summary
        .category
        .as_deref()
        .or(summary.subagent_type.as_deref());
    if let Some(name) = subagent {
        lines.push(block_content_line(
            format!("Subagent: {}", name),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
    }

    if let Some(prompt) = summary.prompt_preview.as_deref() {
        lines.push(block_content_line(
            format!("Task: {}", prompt),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    } else if let Some(description) = summary.description.as_deref() {
        lines.push(block_content_line(
            format!("Task: {}", format_preview_line(description, 88)),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    if let Some(skill_count) = summary.skill_count {
        lines.push(block_content_line(
            format!(
                "Skills: {}",
                if skill_count == 0 {
                    "none".to_string()
                } else {
                    skill_count.to_string()
                }
            ),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    if !summary.checklist.is_empty() {
        let total = summary.checklist.len();
        let preview_limit = total.min(4);
        lines.push(block_content_line(
            format!("Checklist ({} items):", total),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
        for item in summary.checklist.iter().take(preview_limit) {
            lines.push(block_content_line(
                format!("[ ] {}", format_preview_line(item, 88)),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
        if total > preview_limit {
            lines.push(block_content_line(
                format!("… ({} more items)", total - preview_limit),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }
}

fn render_task_result_block(
    result_text: &str,
    arguments: &str,
    metadata: Option<&HashMap<String, serde_json::Value>>,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    let arg_summary = parse_task_argument_summary(arguments);
    let subagent = arg_summary
        .category
        .as_deref()
        .or(arg_summary.subagent_type.as_deref());
    if let Some(name) = subagent {
        lines.push(block_content_line(
            format!("Subagent: {}", name),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
    }
    if let Some(prompt) = arg_summary.prompt_preview.as_deref() {
        lines.push(block_content_line(
            format!("Task: {}", prompt),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }

    let summary = parse_task_result_summary(result_text);
    if let Some(task_id) = summary.task_id.as_deref() {
        lines.push(block_content_line(
            format!("Task ID: {}", task_id),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
    }
    if let Some(task_status) = summary.task_status.as_deref() {
        let status_color = if task_status.eq_ignore_ascii_case(TASK_STATUS_COMPLETED) {
            theme.success
        } else {
            theme.info
        };
        lines.push(block_content_line(
            format!("Status: {}", task_status),
            Style::default().fg(status_color),
            theme,
            bg,
        ));
    }
    if let Some(meta) = metadata {
        if let Some(has_text_output) = meta
            .get(task_metadata_keys::HAS_TEXT_OUTPUT)
            .and_then(|v| v.as_bool())
        {
            lines.push(block_content_line(
                format!(
                    "Text Output: {}",
                    if has_text_output { "yes" } else { "no" }
                ),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
        if let Some(model) = meta
            .get(task_metadata_keys::MODEL)
            .and_then(|v| v.as_object())
        {
            let provider = model
                .get(task_metadata_keys::MODEL_PROVIDER_ID_CAMEL)
                .or_else(|| model.get(task_metadata_keys::MODEL_PROVIDER_ID_SNAKE))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let model_id = model
                .get(task_metadata_keys::MODEL_ID_CAMEL)
                .or_else(|| model.get(task_metadata_keys::MODEL_ID_SNAKE))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !provider.is_empty() || !model_id.is_empty() {
                let rendered = if !provider.is_empty() && !model_id.is_empty() {
                    format!("{provider}:{model_id}")
                } else {
                    format!("{provider}{model_id}")
                };
                lines.push(block_content_line(
                    format!("Model: {}", rendered),
                    Style::default().fg(theme.text_muted),
                    theme,
                    bg,
                ));
            }
        }
    }

    if summary.body.trim().is_empty() {
        lines.push(block_content_line(
            "Subagent finished with no textual output",
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
        if !arg_summary.checklist.is_empty() {
            let completed = summary
                .task_status
                .as_deref()
                .is_some_and(|status| status.eq_ignore_ascii_case("completed"));
            lines.push(block_content_line(
                format!("Checklist ({} items):", arg_summary.checklist.len()),
                Style::default().fg(theme.info),
                theme,
                bg,
            ));
            let preview_limit = if show_tool_details {
                arg_summary.checklist.len()
            } else {
                arg_summary.checklist.len().min(5)
            };
            for item in arg_summary.checklist.iter().take(preview_limit) {
                let checked = completed;
                let marker = if checked { "[x]" } else { "[ ]" };
                let style = if checked {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.text_muted)
                };
                lines.push(block_content_line(
                    format!("{} {}", marker, format_preview_line(item, 88)),
                    style,
                    theme,
                    bg,
                ));
            }
            if arg_summary.checklist.len() > preview_limit {
                lines.push(block_content_line(
                    format!(
                        "… ({} more items, toggle Tool Details to expand)",
                        arg_summary.checklist.len() - preview_limit
                    ),
                    Style::default().fg(theme.text_muted),
                    theme,
                    bg,
                ));
            }
        }
        return;
    }

    let mut checklist = parse_markdown_checklist(&summary.body);
    if checklist.is_empty() && !arg_summary.checklist.is_empty() {
        let completed = summary
            .task_status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case("completed"));
        checklist = arg_summary
            .checklist
            .iter()
            .map(|item| ChecklistItem {
                checked: completed,
                text: item.clone(),
            })
            .collect();
    }
    if !checklist.is_empty() {
        let total = checklist.len();
        let preview_limit = if show_tool_details {
            total
        } else {
            total.min(5)
        };
        lines.push(block_content_line(
            format!("Checklist ({} items):", total),
            Style::default().fg(theme.info),
            theme,
            bg,
        ));
        for item in checklist.iter().take(preview_limit) {
            let marker = if item.checked { "[x]" } else { "[ ]" };
            let style = if item.checked {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.text_muted)
            };
            lines.push(block_content_line(
                format!("{} {}", marker, format_preview_line(&item.text, 88)),
                style,
                theme,
                bg,
            ));
        }
        if total > preview_limit {
            lines.push(block_content_line(
                format!(
                    "… ({} more items, toggle Tool Details to expand)",
                    total - preview_limit
                ),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
    }

    let markdown_lines = MarkdownRenderer::new(theme.clone()).to_lines(&summary.body);
    let total = markdown_lines.len();
    let preview_limit = if show_tool_details {
        total
    } else {
        total.min(5)
    };

    if total > 1 {
        lines.push(block_content_line(
            format!("({} lines)", total),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }
    for markdown_line in markdown_lines.into_iter().take(preview_limit) {
        lines.push(block_markdown_line(markdown_line, theme, bg));
    }
    if total > preview_limit {
        lines.push(block_content_line(
            format!(
                "… ({} more lines, toggle Tool Details to expand)",
                total - preview_limit
            ),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }
}

#[derive(Debug, Clone)]
struct TodoPreviewEntry {
    status: String,
    text: String,
    priority: Option<String>,
}

fn is_ascii_todo_marker_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- [ ]")
        || trimmed.starts_with("- [x]")
        || trimmed.starts_with("- [X]")
        || trimmed.starts_with("- [~]")
    {
        return true;
    }

    let mut parts = trimmed.split_whitespace();
    let first = parts.next().unwrap_or_default();
    let second = parts.next().unwrap_or_default();
    first.starts_with("todo_")
        || second.starts_with("todo_")
        || first.starts_with("TODO_")
        || second.starts_with("TODO_")
}

fn is_legacy_todo_marker_line(line: &str) -> bool {
    line.starts_with("✅")
        || line.starts_with("🔄")
        || line.starts_with("⏳")
        || line.starts_with("☐")
        || line.starts_with("☑")
        || line.starts_with("❌")
}

fn is_todo_marker_line(line: &str) -> bool {
    is_ascii_todo_marker_line(line) || is_legacy_todo_marker_line(line)
}

fn detect_todo_status(line: &str) -> String {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();

    // Prefer ASCII markdown checkbox markers for consistency.
    if lower.starts_with("- [x]") {
        return "done".to_string();
    }
    if lower.starts_with("- [~]") {
        return "in progress".to_string();
    }
    if lower.starts_with("- [ ]") {
        return "pending".to_string();
    }
    if lower.contains("[done]") {
        return "done".to_string();
    }
    if lower.contains("[in_progress]") || lower.contains("[in-progress]") {
        return "in progress".to_string();
    }
    if lower.contains("[cancelled]") || lower.contains("[canceled]") {
        return "cancelled".to_string();
    }
    if lower.contains("[pending]") {
        return "pending".to_string();
    }

    // Backward-compatible fallback for legacy emoji output.
    if trimmed.starts_with("✅") || trimmed.starts_with("☑") {
        return "done".to_string();
    }
    if trimmed.starts_with("🔄") {
        return "in progress".to_string();
    }
    if trimmed.starts_with("❌") {
        return "cancelled".to_string();
    }
    if trimmed.starts_with("⏳") || trimmed.starts_with("☐") {
        return "pending".to_string();
    }

    "todo".to_string()
}

fn parse_todowrite_entries(result_text: &str) -> Vec<TodoPreviewEntry> {
    let raw_lines: Vec<&str> = result_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut entries = Vec::new();
    let mut idx = 0usize;

    while idx < raw_lines.len() {
        let line = raw_lines[idx];
        if line.starts_with('#') {
            idx += 1;
            continue;
        }

        if !is_todo_marker_line(line) {
            idx += 1;
            continue;
        }

        let status = detect_todo_status(line);

        let priority = ["high", "medium", "low"].iter().find_map(|p| {
            let tag = format!("[{}]", p);
            line.contains(&tag).then(|| p.to_string())
        });

        let mut text = line.to_string();
        if idx + 1 < raw_lines.len() {
            let next = raw_lines[idx + 1];
            let next_is_marker = next.starts_with('#') || is_todo_marker_line(next);
            if !next_is_marker {
                text = next.to_string();
                idx += 1;
            }
        }

        entries.push(TodoPreviewEntry {
            status,
            text,
            priority,
        });
        idx += 1;
    }

    entries
}

fn render_todowrite_result_block(
    result_text: &str,
    show_tool_details: bool,
    theme: &Theme,
    bg: ratatui::style::Color,
    lines: &mut Vec<Line<'static>>,
) {
    let entries = parse_todowrite_entries(result_text);
    if entries.is_empty() {
        let output_lines: Vec<&str> = result_text.lines().collect();
        let line_count = output_lines.len();
        lines.push(block_content_line(
            format!("({} lines of output)", line_count),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
        for line in output_lines.iter().take(8) {
            lines.push(block_content_line(
                format_preview_line(line, 96),
                Style::default().fg(theme.text),
                theme,
                bg,
            ));
        }
        if line_count > 8 {
            lines.push(block_content_line(
                format!("… ({} more lines)", line_count - 8),
                Style::default().fg(theme.text_muted),
                theme,
                bg,
            ));
        }
        return;
    }

    let total = entries.len();
    let preview_limit = if show_tool_details {
        total
    } else {
        total.min(5)
    };
    lines.push(block_content_line(
        format!("Todo List ({} items)", total),
        Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
        theme,
        bg,
    ));

    for entry in entries.iter().take(preview_limit) {
        let status_style = match entry.status.as_str() {
            "done" => Style::default().fg(theme.success),
            "in progress" => Style::default().fg(theme.warning),
            "cancelled" => Style::default().fg(theme.error),
            _ => Style::default().fg(theme.text_muted),
        };
        let mut row = format!(
            "[{}] {}",
            entry.status,
            format_preview_line(&entry.text, 72)
        );
        if let Some(priority) = entry.priority.as_deref() {
            row.push_str(&format!("  [{}]", priority));
        }
        lines.push(block_content_line(row, status_style, theme, bg));
    }

    if total > preview_limit {
        lines.push(block_content_line(
            format!(
                "… ({} more todos, toggle Tool Details to expand)",
                total - preview_limit
            ),
            Style::default().fg(theme.text_muted),
            theme,
            bg,
        ));
    }
}

fn block_prefix(theme: &Theme, background: ratatui::style::Color) -> Span<'static> {
    Span::styled(
        "│ ",
        Style::default().fg(theme.border_subtle).bg(background),
    )
}

fn block_content_line(
    content: impl Into<String>,
    style: Style,
    theme: &Theme,
    background: ratatui::style::Color,
) -> Line<'static> {
    Line::from(vec![
        block_prefix(theme, background),
        Span::styled(format!("  {}", content.into()), style.bg(background)),
    ])
}

fn block_markdown_line(
    content: Line<'static>,
    theme: &Theme,
    background: ratatui::style::Color,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(content.spans.len() + 2);
    spans.push(block_prefix(theme, background));
    spans.push(Span::styled("  ", Style::default().bg(background)));
    for span in content.spans {
        spans.push(Span::styled(span.content, span.style.bg(background)));
    }
    Line::from(spans)
}

fn styles_for_state(
    state: ToolState,
    is_denied: bool,
    theme: &Theme,
) -> (&'static str, Style, Style) {
    match state {
        ToolState::Pending => (
            "◯",
            Style::default().fg(theme.warning),
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        ),
        ToolState::Running => (
            super::spinner::progress_circle_icon(),
            Style::default().fg(theme.warning),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        ToolState::Completed => (
            "●",
            Style::default().fg(theme.success),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        ToolState::Failed => {
            let mut name_style = Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD);
            if is_denied {
                name_style = name_style.add_modifier(Modifier::CROSSED_OUT);
            }
            ("✗", Style::default().fg(theme.error), name_style)
        }
    }
}

fn normalize_tool_name(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

fn tool_argument_preview(normalized_name: &str, arguments: &str) -> Option<String> {
    let raw = arguments.trim();
    let parsed = serde_json::from_str::<Value>(raw)
        .ok()
        .or_else(|| rocode_util::json::try_parse_json_object_robust(raw));
    let object = parsed.as_ref().and_then(|v| v.as_object());
    let tool_kind = BuiltinToolName::parse(normalized_name);

    if matches!(tool_kind, Some(BuiltinToolName::Bash)) {
        let command = parsed
            .as_ref()
            .and_then(extract_shell_command)
            .or_else(|| (!raw.is_empty()).then_some(raw.to_string()))?;
        return Some(format!("$ {}", command.trim()));
    }

    if matches!(tool_kind, Some(BuiltinToolName::Read)) {
        if let Some(path) = parsed.as_ref().and_then(extract_path) {
            return Some(format!("→ {}", path));
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::Ls)) {
        if let Some(path) = parsed.as_ref().and_then(extract_path) {
            return Some(format!("→ {}", path));
        }
        return Some("→ .".to_string());
    }

    if matches!(
        tool_kind,
        Some(BuiltinToolName::Write | BuiltinToolName::Edit | BuiltinToolName::MultiEdit)
    ) {
        if let Some(path) = parsed.as_ref().and_then(extract_path) {
            return Some(format!("← {}", path));
        }
        if let Some(path) = extract_jsonish_path_from_raw(raw) {
            return Some(format!("← {}", path));
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::Glob)) {
        if let Some(pattern) = parsed
            .as_ref()
            .and_then(|value| extract_string_key(value, &[tool_arg_keys::PATTERN]))
        {
            let target = parsed.as_ref().and_then(extract_path);
            return Some(match target {
                Some(path) => format!("\"{}\" in {}", pattern, path),
                None => format!("\"{}\"", pattern),
            });
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::Grep)) {
        if let Some(pattern) = parsed.as_ref().and_then(|value| {
            extract_string_key(value, &[tool_arg_keys::PATTERN, tool_arg_keys::QUERY])
        }) {
            let target = parsed.as_ref().and_then(extract_path);
            return Some(match target {
                Some(path) => format!("\"{}\" in {}", pattern, path),
                None => format!("\"{}\"", pattern),
            });
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::WebFetch)) {
        if let Some(url) = parsed
            .as_ref()
            .and_then(|value| extract_string_key(value, &[tool_arg_keys::URL]))
        {
            return Some(url);
        }
    }

    if matches!(
        tool_kind,
        Some(BuiltinToolName::WebSearch | BuiltinToolName::CodeSearch)
    ) {
        if let Some(query) = parsed
            .as_ref()
            .and_then(|value| extract_string_key(value, &[tool_arg_keys::QUERY]))
        {
            return Some(format!("\"{}\"", query));
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::Task)) {
        let summary = parse_task_argument_summary(arguments);
        let kind = summary
            .category
            .as_deref()
            .or(summary.subagent_type.as_deref());
        let description = summary.description.as_deref();
        let prompt = summary.prompt_preview.as_deref();

        return match (kind, description, prompt) {
            (Some(kind), Some(description), _) => Some(format!(
                "{kind} task {}",
                format_preview_line(description, 56)
            )),
            (Some(kind), None, Some(prompt)) => Some(format!("{kind} task {}", prompt)),
            (Some(kind), None, None) => Some(format!("{kind} task")),
            (None, Some(description), _) => Some(format_preview_line(description, 72)),
            (None, None, Some(prompt)) => Some(prompt.to_string()),
            (None, None, None) => None,
        };
    }

    if matches!(tool_kind, Some(BuiltinToolName::Batch)) {
        if let Some(calls) = parsed
            .as_ref()
            .and_then(|v| {
                v.get(tool_arg_keys::TOOL_CALLS_CAMEL)
                    .or_else(|| v.get(tool_arg_keys::TOOL_CALLS))
            })
            .and_then(|v| v.as_array())
        {
            let count = calls.len();
            let names: Vec<String> = calls
                .iter()
                .filter_map(|call| {
                    call.get(tool_arg_keys::TOOL)
                        .or_else(|| call.get(tool_arg_keys::NAME))
                        .or_else(|| call.get(tool_arg_keys::TOOL_NAME))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            // Deduplicate while preserving order
            let mut seen = std::collections::HashSet::new();
            let unique: Vec<&str> = names
                .iter()
                .filter(|n| seen.insert(n.as_str()))
                .map(|n| n.as_str())
                .collect();
            return if unique.is_empty() {
                Some(format!("{} tools", count))
            } else {
                Some(format!("{} tools ({})", count, unique.join(", ")))
            };
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::Question)) {
        if let Some(questions) = object
            .and_then(|value| value.get(wire_fields::QUESTIONS))
            .and_then(|value| value.as_array())
        {
            let count = questions.len();
            // Show the first question text as preview
            let first_q = questions
                .first()
                .and_then(|q| q.get(wire_fields::QUESTION).and_then(|v| v.as_str()));
            return match first_q {
                Some(text) if count == 1 => Some(format_preview_line(text, 72)),
                Some(text) => Some(format!(
                    "{} (+{} more)",
                    format_preview_line(text, 52),
                    count - 1
                )),
                None => Some(format!(
                    "{} question{}",
                    count,
                    if count == 1 { "" } else { "s" }
                )),
            };
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::TodoWrite)) {
        if let Some(count) = object
            .and_then(|value| value.get(todo_keys::TODOS))
            .and_then(|value| value.as_array())
            .map(Vec::len)
        {
            return Some(format!(
                "Update {} todo{}",
                count,
                if count == 1 { "" } else { "s" }
            ));
        }
        return Some("Update todos".to_string());
    }

    if matches!(tool_kind, Some(BuiltinToolName::Skill)) {
        if let Some(name) = parsed
            .as_ref()
            .and_then(|value| extract_string_key(value, &[tool_arg_keys::NAME]))
        {
            return Some(format!("\"{}\"", name));
        }
    }

    if matches!(tool_kind, Some(BuiltinToolName::ApplyPatch)) {
        return Some("Patch".to_string());
    }

    if matches!(tool_kind, Some(BuiltinToolName::Lsp)) {
        if let Some(operation) = parsed
            .as_ref()
            .and_then(|value| extract_string_key(value, &[tool_arg_keys::OPERATION]))
        {
            let target = parsed.as_ref().and_then(|value| {
                extract_string_key(
                    value,
                    &[
                        patch_keys::FILE_PATH,
                        patch_keys::FILE_PATH_SNAKE,
                        patch_keys::LEGACY_PATH,
                    ],
                )
            });
            return Some(match target {
                Some(path) => format!("{} {}", operation, path),
                None => operation,
            });
        }
    }

    if raw.is_empty() {
        return None;
    }

    if let Some(preview) = object.and_then(|value| {
        format_primitive_arguments(
            value,
            &[
                patch_keys::CONTENT,
                patch_keys::NEW_STRING,
                patch_keys::OLD_STRING,
                "patch",
                tool_arg_keys::PROMPT,
                wire_fields::QUESTIONS,
                todo_keys::TODOS,
            ],
        )
    }) {
        return Some(preview);
    }

    let first = raw.lines().next().unwrap_or(raw).trim();
    if first.is_empty() {
        None
    } else {
        Some(format_preview_line(first, 84))
    }
}

fn extract_shell_command(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    for key in [
        tool_arg_keys::COMMAND,
        tool_arg_keys::CMD,
        tool_arg_keys::SCRIPT,
        tool_arg_keys::INPUT,
        output_keys::TEXT,
    ] {
        if let Some(command) = object.get(key).and_then(|v| v.as_str()) {
            let trimmed = command.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_path(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    for key in [
        patch_keys::LEGACY_PATH,
        patch_keys::FILE_PATH_SNAKE,
        patch_keys::FILE_PATH,
        "file",
        "filename",
        patch_keys::FILEPATH,
        "absolute_path",
        "absolutePath",
        "target",
        "destination",
        "to",
        "from",
    ] {
        if let Some(path) = object.get(key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_string_key(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(content) = object.get(*key).and_then(|value| value.as_str()) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn format_primitive_arguments(
    object: &serde_json::Map<String, Value>,
    omit: &[&str],
) -> Option<String> {
    let mut parts = Vec::new();

    for (key, value) in object {
        if omit.contains(&key.as_str()) {
            continue;
        }

        let rendered = match value {
            Value::String(content) => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    continue;
                }
                format_preview_line(trimmed, 28)
            }
            Value::Number(number) => number.to_string(),
            Value::Bool(flag) => flag.to_string(),
            _ => continue,
        };

        parts.push(format!("{key}={rendered}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("[{}]", parts.join(", ")))
    }
}

fn extract_jsonish_string_field(input: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let field_idx = input.find(&needle)?;
    let after_field = &input[field_idx + needle.len()..];
    let colon_idx = after_field.find(':')?;
    let mut chars = after_field[colon_idx + 1..].chars().peekable();

    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
    if !matches!(chars.next(), Some('"')) {
        return None;
    }

    let mut value = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            match ch {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                other => value.push(other),
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => return Some(value),
            other => value.push(other),
        }
    }

    Some(value)
}

fn extract_jsonish_path_from_raw(raw: &str) -> Option<String> {
    let direct = extract_jsonish_string_field(raw, patch_keys::FILE_PATH_SNAKE)
        .or_else(|| extract_jsonish_string_field(raw, patch_keys::FILE_PATH));
    if direct.is_some() {
        return direct;
    }

    if raw.contains("\\\"") {
        let de_escaped = raw.replace("\\\"", "\"");
        return extract_jsonish_string_field(&de_escaped, patch_keys::FILE_PATH_SNAKE)
            .or_else(|| extract_jsonish_string_field(&de_escaped, patch_keys::FILE_PATH));
    }

    None
}

fn parse_write_summary(result_text: &str) -> Option<WriteSummary> {
    let first_line = result_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if first_line.is_empty() {
        return None;
    }

    let lower = first_line.to_ascii_lowercase();
    let verb = if lower.contains("wrote") || lower.contains("written") {
        Some("wrote")
    } else if lower.contains("updated") {
        Some("updated")
    } else if lower.contains("created") {
        Some("created")
    } else if lower.contains("saved") {
        Some("saved")
    } else {
        None
    };

    let mut summary = WriteSummary {
        verb,
        ..WriteSummary::default()
    };

    let tokens: Vec<&str> = first_line.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        if token.starts_with("bytes") && index > 0 {
            summary.size_bytes = parse_numeric_token(tokens[index - 1]);
        }
        if token.starts_with("lines") && index > 0 {
            summary.total_lines = parse_numeric_token(tokens[index - 1]);
        }
    }

    summary.path = first_line
        .rsplit_once(" to ")
        .map(|(_, path)| sanitize_path_token(path))
        .or_else(|| {
            first_line
                .rsplit_once(" into ")
                .map(|(_, path)| sanitize_path_token(path))
        });

    if summary.verb.is_none()
        && summary.size_bytes.is_none()
        && summary.total_lines.is_none()
        && summary.path.is_none()
    {
        None
    } else {
        Some(summary)
    }
}

fn parse_numeric_token(token: &str) -> Option<usize> {
    let digits: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<usize>().ok()
    }
}

fn sanitize_path_token(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim_end_matches('.')
        .trim_end_matches(',')
        .to_string()
}

fn parse_read_summary(result_text: &str) -> ReadSummary {
    let mut summary = ReadSummary::default();
    for line in result_text.lines() {
        if summary.size_bytes.is_none() {
            summary.size_bytes = extract_tag_value(line, "size").and_then(|v| v.parse().ok());
        }
        if summary.total_lines.is_none() {
            summary.total_lines =
                extract_tag_value(line, "total-lines").and_then(|v| v.parse().ok());
        }
        if summary.size_bytes.is_some() && summary.total_lines.is_some() {
            break;
        }
    }
    summary
}

fn format_read_summary(summary: &ReadSummary) -> Option<String> {
    match (summary.size_bytes, summary.total_lines) {
        (Some(size), Some(lines)) => Some(format!("{}, {} lines", format_bytes(size), lines)),
        (Some(size), None) => Some(format_bytes(size)),
        (None, Some(lines)) => Some(format!("{} lines", lines)),
        (None, None) => None,
    }
}

fn extract_tag_value<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    let content = line.strip_prefix(start_tag.as_str())?;
    content.strip_suffix(end_tag.as_str())
}

fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.1} KB", bytes as f64 / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn is_denied_result(result_text: &str) -> bool {
    let lower = result_text.to_ascii_lowercase();
    lower.contains("permission denied")
        || lower.contains("denied")
        || lower.contains("not permitted")
        || lower.contains("forbidden")
}

fn format_preview_line(line: &str, max_chars: usize) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

#[cfg(test)]
mod tests {
    use super::{
        format_read_summary, parse_markdown_checklist, parse_read_summary,
        parse_task_argument_summary, parse_write_summary, tool_argument_preview,
    };
    use rocode_core::contracts::patch::{keys as patch_keys, FileChangeType};
    use rocode_core::contracts::tools::BuiltinToolName;

    #[test]
    fn list_tool_preview_shows_path() {
        let preview = tool_argument_preview(BuiltinToolName::Ls.as_str(), r#"{"path":"."}"#);
        assert_eq!(preview.as_deref(), Some("→ ."));
    }

    #[test]
    fn read_tool_preview_supports_file_path_keys() {
        let preview = tool_argument_preview(
            BuiltinToolName::Read.as_str(),
            r#"{"file_path":"/tmp/a.txt"}"#,
        );
        assert_eq!(preview.as_deref(), Some("→ /tmp/a.txt"));
    }

    #[test]
    fn generic_preview_compacts_json_to_key_values() {
        let preview = tool_argument_preview("unknown", r#"{"path":".","recursive":true}"#);
        assert_eq!(preview.as_deref(), Some("[path=., recursive=true]"));
    }

    #[test]
    fn apply_patch_preview_hides_patch_body() {
        let preview =
            tool_argument_preview(BuiltinToolName::ApplyPatch.as_str(), "*** Begin Patch\n...");
        assert_eq!(preview.as_deref(), Some("Patch"));
    }

    #[test]
    fn parse_read_summary_from_tool_output_tags() {
        let output = "<path>/tmp/a.txt</path>\n<size>4096</size>\n<total-lines>256</total-lines>\n<content>...</content>";
        let summary = parse_read_summary(output);
        assert_eq!(
            format_read_summary(&summary).as_deref(),
            Some("4.0 KB, 256 lines")
        );
    }

    #[test]
    fn batch_preview_shows_tool_count_and_names() {
        let args = r#"{"toolCalls":[{"tool":"read","parameters":{"file_path":"/tmp/a.txt"}},{"tool":"edit","parameters":{"file_path":"/tmp/b.txt"}},{"tool":"read","parameters":{"file_path":"/tmp/c.txt"}}]}"#;
        let preview = tool_argument_preview(BuiltinToolName::Batch.as_str(), args);
        assert_eq!(preview.as_deref(), Some("3 tools (read, edit)"));
    }

    #[test]
    fn batch_preview_with_no_names_shows_count_only() {
        let args = r#"{"toolCalls":[{},{}]}"#;
        let preview = tool_argument_preview(BuiltinToolName::Batch.as_str(), args);
        assert_eq!(preview.as_deref(), Some("2 tools"));
    }

    #[test]
    fn write_preview_recovers_path_from_jsonish_arguments() {
        let args = "{\"file_path\":\"t2.html\",\"content\":\"<!DOCTYPE html>\n<html";
        let preview = tool_argument_preview(BuiltinToolName::Write.as_str(), args);
        assert_eq!(preview.as_deref(), Some("← t2.html"));
    }

    #[test]
    fn task_preview_uses_prompt_when_description_missing() {
        let args = r###"{"category":"quick","prompt":"## 1. TASK\nRedesign t2.html with stronger visual impact."}"###;
        let preview = tool_argument_preview(BuiltinToolName::Task.as_str(), args);
        assert_eq!(
            preview.as_deref(),
            Some("quick task Redesign t2.html with stronger visual impact.")
        );
    }

    #[test]
    fn task_argument_summary_extracts_category_prompt_and_skill_count() {
        let args = r###"{"category":"visual-engineering","load_skills":["frontend-ui-ux","theme-factory"],"prompt":"## 1. TASK\nRedesign page"}"###;
        let summary = parse_task_argument_summary(args);
        assert_eq!(summary.category.as_deref(), Some("visual-engineering"));
        assert_eq!(summary.prompt_preview.as_deref(), Some("Redesign page"));
        assert_eq!(summary.skill_count, Some(2));
    }

    #[test]
    fn task_argument_summary_extracts_prompt_checklist() {
        let args = r###"{"category":"visual-engineering","prompt":"## 2. EXPECTED OUTCOME\n- [ ] 修改 t2.html\n- [ ] 增强视觉冲击力"}"###;
        let summary = parse_task_argument_summary(args);
        assert_eq!(summary.checklist.len(), 2);
        assert_eq!(summary.checklist[0], "修改 t2.html");
        assert_eq!(summary.checklist[1], "增强视觉冲击力");
    }

    #[test]
    fn parse_markdown_checklist_supports_ascii_markers() {
        let text = "- [ ] todo a\n- [x] todo b\n- [X] todo c";
        let checklist = parse_markdown_checklist(text);
        assert_eq!(checklist.len(), 3);
        assert!(!checklist[0].checked);
        assert!(checklist[1].checked);
        assert!(checklist[2].checked);
    }

    #[test]
    fn parse_write_summary_from_success_message() {
        let output = "Successfully wrote 30199 bytes (725 lines) to ./t2.html";
        let summary = parse_write_summary(output).expect("write summary should parse");
        assert_eq!(summary.size_bytes, Some(30199));
        assert_eq!(summary.total_lines, Some(725));
        assert_eq!(summary.path.as_deref(), Some("./t2.html"));
        assert_eq!(summary.verb, Some("wrote"));
    }

    #[test]
    fn render_edit_result_block_shows_diff_when_metadata_has_diff() {
        use super::{render_tool_call, ToolResultInfo, ToolState};
        use std::collections::HashMap;

        let theme = crate::theme::Theme::dark();
        let diff_content = "--- a/test.rs\n+++ b/test.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"old\");\n+    println!(\"new\");\n }";
        let mut metadata = HashMap::new();
        metadata.insert(
            patch_keys::DIFF.to_string(),
            serde_json::json!(diff_content),
        );
        metadata.insert(
            patch_keys::FILEPATH.to_string(),
            serde_json::json!("test.rs"),
        );
        let mut tool_results = HashMap::new();
        tool_results.insert(
            "tc1".to_string(),
            ToolResultInfo {
                output: "Edit completed".to_string(),
                is_error: false,
                title: None,
                metadata: Some(metadata),
            },
        );

        let lines = render_tool_call(
            "tc1",
            BuiltinToolName::Edit.as_str(),
            r#"{"file_path":"test.rs","old_string":"old","new_string":"new"}"#,
            ToolState::Completed,
            &tool_results,
            true, // show_tool_details = true to trigger diff rendering
            &theme,
        );

        // Should have more than just the header — diff lines should be present
        assert!(
            lines.len() > 3,
            "Expected diff lines, got {} lines",
            lines.len()
        );

        // Flatten all spans to text and check for diff markers
        let full_text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(
            full_text.contains("Edit Complete"),
            "Should show edit complete header"
        );
        assert!(
            full_text.contains("new") || full_text.contains("+"),
            "Should contain diff addition marker or content"
        );
    }

    #[test]
    fn render_patch_result_block_shows_per_file_diffs() {
        use super::{render_tool_call, ToolResultInfo, ToolState};
        use std::collections::HashMap;

        let theme = crate::theme::Theme::dark();
        let file1_diff = "--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new";
        let file2_diff = "--- a/bar.rs\n+++ b/bar.rs\n@@ -1 +1 @@\n-x\n+y";
        let mut metadata = HashMap::new();
        metadata.insert(
            patch_keys::DIFF.to_string(),
            serde_json::json!(format!("{}\n{}", file1_diff, file2_diff)),
        );
        metadata.insert(
            patch_keys::FILES.to_string(),
            serde_json::Value::Array(vec![
                serde_json::Value::Object(serde_json::Map::from_iter([
                    (
                        patch_keys::RELATIVE_PATH.to_string(),
                        serde_json::json!("foo.rs"),
                    ),
                    (
                        patch_keys::CHANGE_TYPE.to_string(),
                        serde_json::json!(FileChangeType::Update.as_str()),
                    ),
                    (
                        patch_keys::FILE_DIFF.to_string(),
                        serde_json::json!(file1_diff),
                    ),
                ])),
                serde_json::Value::Object(serde_json::Map::from_iter([
                    (
                        patch_keys::RELATIVE_PATH.to_string(),
                        serde_json::json!("bar.rs"),
                    ),
                    (
                        patch_keys::CHANGE_TYPE.to_string(),
                        serde_json::json!(FileChangeType::Add.as_str()),
                    ),
                    (
                        patch_keys::FILE_DIFF.to_string(),
                        serde_json::json!(file2_diff),
                    ),
                ])),
            ]),
        );
        let mut tool_results = HashMap::new();
        tool_results.insert(
            "tc1".to_string(),
            ToolResultInfo {
                output: "Patch applied".to_string(),
                is_error: false,
                title: None,
                metadata: Some(metadata),
            },
        );

        let lines = render_tool_call(
            "tc1",
            BuiltinToolName::ApplyPatch.as_str(),
            "",
            ToolState::Completed,
            &tool_results,
            true,
            &theme,
        );

        let full_text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();

        // Should show per-file headers
        assert!(
            full_text.contains("Patched foo.rs"),
            "Should show per-file header for foo.rs, got: {}",
            full_text
        );
        assert!(
            full_text.contains("Created bar.rs"),
            "Should show 'Created' header for added file bar.rs, got: {}",
            full_text
        );
    }

    #[test]
    fn render_write_result_block_shows_diff_from_metadata() {
        use super::{render_tool_call, ToolResultInfo, ToolState};
        use std::collections::HashMap;

        let theme = crate::theme::Theme::dark();
        let diff_content = "--- /dev/null\n+++ b/new_file.txt\n@@ -0,0 +1,2 @@\n+line1\n+line2";
        let mut metadata = HashMap::new();
        metadata.insert(
            patch_keys::DIFF.to_string(),
            serde_json::json!(diff_content),
        );
        let mut tool_results = HashMap::new();
        tool_results.insert(
            "tc1".to_string(),
            ToolResultInfo {
                output: "Successfully wrote 10 bytes (2 lines) to ./new_file.txt".to_string(),
                is_error: false,
                title: None,
                metadata: Some(metadata),
            },
        );

        let lines = render_tool_call(
            "tc1",
            BuiltinToolName::Write.as_str(),
            r#"{"file_path":"./new_file.txt","content":"line1\nline2"}"#,
            ToolState::Completed,
            &tool_results,
            true,
            &theme,
        );

        let full_text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(
            full_text.contains("Write Complete"),
            "Should show write header"
        );
        assert!(
            full_text.contains("line1") || full_text.contains("+"),
            "Should render diff content"
        );
    }
}
