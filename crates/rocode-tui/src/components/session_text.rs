use std::collections::HashMap;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

use super::markdown::MarkdownRenderer;
use crate::{context::Message, theme::Theme};

pub const ASSISTANT_MARKER: &str = "▶ ";

pub struct MessageTextRender {
    pub lines: Vec<Line<'static>>,
    pub allow_semantic_highlighting: bool,
    pub source_len: usize,
}

pub fn render_message_text_part(
    message: &Message,
    text: &str,
    theme: &Theme,
    marker_color: Color,
) -> MessageTextRender {
    let metadata = message.metadata.as_ref();

    if let Some(stage) = scheduler_stage(metadata) {
        if stage == "route" {
            if let Some(lines) = render_route_stage_part(text, metadata, theme, marker_color) {
                return MessageTextRender {
                    lines,
                    allow_semantic_highlighting: false,
                    source_len: text.len(),
                };
            }
        }

        let lines = render_scheduler_stage_part(text, stage, metadata, theme, marker_color);
        return MessageTextRender {
            lines,
            allow_semantic_highlighting: false,
            source_len: text.len(),
        };
    }

    MessageTextRender {
        lines: render_text_part(text, theme, marker_color),
        allow_semantic_highlighting: true,
        source_len: text.len(),
    }
}

pub fn render_text_part(text: &str, theme: &Theme, marker_color: Color) -> Vec<Line<'static>> {
    let cleaned = strip_think_tags(text);
    let renderer = MarkdownRenderer::new(theme.clone());
    apply_assistant_marker(renderer.to_lines(&cleaned), marker_color)
}

pub struct ReasoningRender {
    pub lines: Vec<Line<'static>>,
    pub collapsible: bool,
}

pub fn render_reasoning_part(
    text: &str,
    theme: &Theme,
    collapsed: bool,
    preview_lines: usize,
) -> ReasoningRender {
    let cleaned = strip_think_tags(&text.replace("[REDACTED]", ""))
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return ReasoningRender {
            lines: Vec::new(),
            collapsible: false,
        };
    }

    let mut lines = Vec::new();
    let renderer = MarkdownRenderer::new(theme.clone()).with_concealed(true);
    let content_lines = renderer.to_lines(&cleaned);
    let total_content_lines = content_lines.len();
    let collapsible = total_content_lines > preview_lines;

    let header_style = Style::default().fg(theme.info).add_modifier(Modifier::BOLD);

    if collapsible && collapsed {
        lines.push(Line::from(Span::styled(
            format!("▶ Thinking ({} lines)", total_content_lines),
            header_style,
        )));
        return ReasoningRender { lines, collapsible };
    }

    lines.push(Line::from(Span::styled("▼ Thinking", header_style)));

    let visible_count = if collapsible && collapsed {
        preview_lines
    } else {
        total_content_lines
    };

    for line in content_lines.into_iter().take(visible_count) {
        let mut spans = vec![Span::styled("  ", Style::default().fg(theme.text_muted))];
        spans.extend(
            line.spans
                .into_iter()
                .map(|span| Span::styled(span.content, span.style.fg(theme.text_muted))),
        );
        lines.push(Line::from(spans));
    }

    if collapsible {
        lines.push(Line::from(Span::styled(
            "  [click to collapse]",
            Style::default().fg(theme.text_muted),
        )));
    }

    ReasoningRender { lines, collapsible }
}

// ---------------------------------------------------------------------------
// Stage card header (shared by all non-route scheduler stages)
// ---------------------------------------------------------------------------

struct StageDecoration {
    icon: &'static str,
    label: &'static str,
    color_fn: fn(&Theme) -> Color,
}

fn stage_decoration(stage: &str) -> StageDecoration {
    match stage {
        "route" => StageDecoration {
            icon: "◈",
            label: "Route",
            color_fn: |t| t.info,
        },
        "interview" => StageDecoration {
            icon: "❓",
            label: "Interview",
            color_fn: |t| t.warning,
        },
        "plan" => StageDecoration {
            icon: "📋",
            label: "Plan",
            color_fn: |t| t.info,
        },
        "delegation" => StageDecoration {
            icon: "📤",
            label: "Delegation",
            color_fn: |t| t.secondary,
        },
        "review" => StageDecoration {
            icon: "🔍",
            label: "Review",
            color_fn: |t| t.warning,
        },
        "execution-orchestration" => StageDecoration {
            icon: "⚡",
            label: "Execution",
            color_fn: |t| t.primary,
        },
        "synthesis" => StageDecoration {
            icon: "✦",
            label: "Synthesis",
            color_fn: |t| t.success,
        },
        "handoff" => StageDecoration {
            icon: "📎",
            label: "Handoff",
            color_fn: |t| t.secondary,
        },
        _ => StageDecoration {
            icon: "◈",
            label: "Stage",
            color_fn: |t| t.primary,
        },
    }
}

fn render_stage_header(
    profile: &str,
    stage: &str,
    stage_index: Option<u64>,
    stage_total: Option<u64>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let decoration = stage_decoration(stage);
    let accent = (decoration.color_fn)(theme);

    let mut title_spans = vec![
        Span::styled(format!("{} ", decoration.icon), Style::default().fg(accent)),
        Span::styled(
            format!("{} · {}", prettify_token(profile), decoration.label),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
    ];

    if let (Some(index), Some(total)) = (stage_index, stage_total) {
        title_spans.push(Span::styled(
            format!("  ({}/{})", index, total),
            Style::default().fg(theme.text_muted),
        ));
    }

    let separator = "─".repeat(40);

    vec![
        Line::from(title_spans),
        Line::from(Span::styled(
            separator,
            Style::default().fg(theme.border_subtle),
        )),
    ]
}

fn render_scheduler_stage_part(
    text: &str,
    stage: &str,
    metadata: Option<&HashMap<String, Value>>,
    theme: &Theme,
    marker_color: Color,
) -> Vec<Line<'static>> {
    let (_title, body) = split_stage_heading(text);
    let profile = metadata
        .and_then(|m| m.get("scheduler_profile"))
        .and_then(Value::as_str)
        .unwrap_or("scheduler");
    let stage_index = metadata
        .and_then(|m| m.get("scheduler_stage_index"))
        .and_then(Value::as_u64);
    let stage_total = metadata
        .and_then(|m| m.get("scheduler_stage_total"))
        .and_then(Value::as_u64);

    let mut lines = render_stage_header(profile, stage, stage_index, stage_total, theme);

    let cleaned = strip_think_tags(body);
    let renderer = MarkdownRenderer::new(theme.clone());
    let body_lines = renderer.to_lines(&cleaned);
    lines.extend(body_lines);

    apply_assistant_marker(lines, marker_color)
}

// ---------------------------------------------------------------------------
// Route stage — structured JSON rendering
// ---------------------------------------------------------------------------

fn render_route_stage_part(
    text: &str,
    metadata: Option<&HashMap<String, Value>>,
    theme: &Theme,
    marker_color: Color,
) -> Option<Vec<Line<'static>>> {
    let (_title, body) = split_stage_heading(text);
    let decision = parse_route_decision_value(body.trim())?;

    let profile = metadata
        .and_then(|m| m.get("scheduler_profile"))
        .and_then(Value::as_str)
        .unwrap_or("scheduler");
    let stage_index = metadata
        .and_then(|m| m.get("scheduler_stage_index"))
        .and_then(Value::as_u64);
    let stage_total = metadata
        .and_then(|m| m.get("scheduler_stage_total"))
        .and_then(Value::as_u64);

    let mut lines = render_stage_header(profile, "route", stage_index, stage_total, theme);

    lines.push(Line::from(vec![
        Span::styled("◈ ", Style::default().fg(theme.primary)),
        Span::styled(
            "Routing Decision",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let mode = route_string_field(&decision, "mode")
        .map(prettify_token)
        .unwrap_or_else(|| "Unknown".to_string());
    let mode_label = route_string_field(&decision, "direct_kind")
        .map(prettify_token)
        .map(|kind| format!("{mode} {kind}"))
        .unwrap_or(mode);
    let mode_style = route_mode_style(&decision, theme);
    lines.push(route_field_line("Mode", &mode_label, theme, mode_style));

    if let Some(preset) = route_string_field(&decision, "preset") {
        lines.push(route_field_line(
            "Preset",
            &prettify_token(preset),
            theme,
            Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(review_mode) = route_string_field(&decision, "review_mode") {
        let review_style = match review_mode {
            "skip" => Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
            _ => Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        };
        lines.push(route_field_line(
            "Review",
            &prettify_token(review_mode),
            theme,
            review_style,
        ));
    }
    if let Some(insert_plan_stage) = decision.get("insert_plan_stage").and_then(Value::as_bool) {
        let value = if insert_plan_stage { "Yes" } else { "No" };
        let style = if insert_plan_stage {
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_muted)
        };
        lines.push(route_field_line("Insert Plan Stage", value, theme, style));
    }
    if let Some(reason) = route_string_field(&decision, "rationale_summary")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(route_field_line(
            "Reason",
            reason,
            theme,
            Style::default().fg(theme.text),
        ));
    }
    if let Some(context) = route_string_field(&decision, "context_append")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("✦ ", Style::default().fg(theme.secondary)),
            Span::styled(
                "Appended Context",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let renderer = MarkdownRenderer::new(theme.clone());
        for line in renderer.to_lines(context) {
            let mut spans = vec![Span::styled("  ", Style::default().fg(theme.text_muted))];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }
    if let Some(response) = route_string_field(&decision, "direct_response")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("✦ ", Style::default().fg(theme.secondary)),
            Span::styled(
                "Direct Response",
                Style::default()
                    .fg(theme.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let renderer = MarkdownRenderer::new(theme.clone());
        for line in renderer.to_lines(response) {
            let mut spans = vec![Span::styled("  ", Style::default().fg(theme.text_muted))];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
    }

    Some(apply_assistant_marker(lines, marker_color))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn route_field_line(label: &str, value: &str, theme: &Theme, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled("• ", Style::default().fg(theme.border_active)),
        Span::styled(
            format!("{label}: "),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), value_style),
    ])
}

fn route_mode_style(decision: &Value, theme: &Theme) -> Style {
    match route_string_field(decision, "mode") {
        Some("direct") => Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
        Some("orchestrate") => Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(theme.text),
    }
}

fn apply_assistant_marker(lines: Vec<Line<'static>>, marker_color: Color) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .enumerate()
        .map(|(idx, line)| {
            let marker = if idx == 0 { ASSISTANT_MARKER } else { "  " };
            let mut spans = vec![Span::styled(marker, Style::default().fg(marker_color))];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

fn scheduler_stage(metadata: Option<&HashMap<String, Value>>) -> Option<&str> {
    metadata
        .and_then(|m| m.get("scheduler_stage"))
        .and_then(Value::as_str)
}

fn split_stage_heading(text: &str) -> (Option<&str>, &str) {
    if let Some(rest) = text.strip_prefix("## ") {
        if let Some((title, body)) = rest.split_once("\n\n") {
            return (Some(title.trim()), body);
        }
        if let Some((title, body)) = rest.split_once('\n') {
            return (Some(title.trim()), body);
        }
    }

    (None, text)
}

fn parse_route_decision_value(text: &str) -> Option<Value> {
    let candidate = extract_json_candidate(text)?;
    serde_json::from_str(candidate).ok()
}

fn extract_json_candidate(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(json_block) = extract_fenced_json_block(trimmed) {
        return Some(json_block);
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (start < end).then_some(&trimmed[start..=end])
}

fn extract_fenced_json_block(text: &str) -> Option<&str> {
    for fence in ["```json", "```JSON", "```"] {
        if let Some(start) = text.find(fence) {
            let rest = &text[start + fence.len()..];
            if let Some(end) = rest.find("```") {
                return Some(rest[..end].trim());
            }
        }
    }
    None
}

fn route_string_field<'a>(decision: &'a Value, key: &str) -> Option<&'a str> {
    decision.get(key).and_then(Value::as_str)
}

fn prettify_token(raw: &str) -> String {
    raw.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_think_tags(text: &str) -> String {
    text.replace("<think>", "")
        .replace("</think>", "")
        .replace("<think/>", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{MessageRole, TokenUsage};
    use chrono::Utc;
    use serde_json::json;

    fn message_with_stage(stage: &str) -> Message {
        message_with_stage_meta(stage, None, None, None)
    }

    fn message_with_stage_meta(
        stage: &str,
        profile: Option<&str>,
        index: Option<u64>,
        total: Option<u64>,
    ) -> Message {
        let mut metadata = HashMap::new();
        metadata.insert("scheduler_stage".to_string(), json!(stage));
        if let Some(p) = profile {
            metadata.insert("scheduler_profile".to_string(), json!(p));
        }
        if let Some(i) = index {
            metadata.insert("scheduler_stage_index".to_string(), json!(i));
        }
        if let Some(t) = total {
            metadata.insert("scheduler_stage_total".to_string(), json!(t));
        }
        Message {
            id: "m1".to_string(),
            role: MessageRole::Assistant,
            content: String::new(),
            created_at: Utc::now(),
            agent: None,
            model: None,
            mode: None,
            finish: None,
            error: None,
            completed_at: None,
            cost: 0.0,
            tokens: TokenUsage::default(),
            metadata: Some(metadata),
            parts: Vec::new(),
        }
    }

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn first_assistant_line_uses_larger_marker() {
        let lines = render_text_part("hello", &Theme::default(), Color::Blue);
        assert!(line_text(&lines[0]).starts_with(ASSISTANT_MARKER));
    }

    #[test]
    fn render_message_text_part_styles_route_orchestration_decision() {
        let theme = Theme::default();
        let message = message_with_stage_meta("route", Some("prometheus"), Some(1), Some(4));
        let rendered = render_message_text_part(
            &message,
            "## prometheus · Route\n\n```json\n{\n  \"mode\": \"orchestrate\",\n  \"preset\": \"prometheus\",\n  \"review_mode\": \"normal\",\n  \"insert_plan_stage\": true,\n  \"rationale_summary\": \"Needs upfront planning.\"\n}\n```",
            &theme,
            Color::Blue,
        );

        assert!(!rendered.allow_semantic_highlighting);
        // Stage header should be present
        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Prometheus · Route"));
        assert!(all_text.contains("(1/4)"));

        let mode_line = rendered
            .lines
            .iter()
            .find(|line| line_text(line).contains("Mode: Orchestrate"))
            .expect("mode line should exist");
        assert_eq!(mode_line.spans[2].style.fg, Some(theme.primary));
        assert!(mode_line.spans[2]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert_eq!(mode_line.spans[3].style.fg, Some(theme.success));
        assert!(mode_line.spans[3]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
        assert!(rendered
            .lines
            .iter()
            .all(|line| !line_text(line).contains("\"mode\"")));
    }

    #[test]
    fn render_message_text_part_styles_route_direct_response_section() {
        let theme = Theme::default();
        let message = message_with_stage("route");
        let rendered = render_message_text_part(
            &message,
            "## router · Route\n\n{\"mode\":\"direct\",\"direct_kind\":\"reply\",\"direct_response\":\"Hi there!\",\"rationale_summary\":\"Greeting\"}",
            &theme,
            Color::Blue,
        );

        let heading = rendered
            .lines
            .iter()
            .find(|line| line_text(line).contains("Direct Response"))
            .expect("direct response heading should exist");
        assert_eq!(heading.spans[2].style.fg, Some(theme.secondary));
        assert!(heading.spans[2].style.add_modifier.contains(Modifier::BOLD));
        assert!(rendered
            .lines
            .iter()
            .any(|line| line_text(line).contains("Hi there!")));
    }

    #[test]
    fn render_message_text_part_styles_route_context_as_markdown() {
        let theme = Theme::default();
        let message = message_with_stage("route");
        let rendered = render_message_text_part(
            &message,
            "## router · Route\n\n{\"mode\":\"orchestrate\",\"preset\":\"sisyphus\",\"context_append\":\"**important** context\",\"rationale_summary\":\"test\"}",
            &theme,
            Color::Blue,
        );

        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Appended Context"));
        // The markdown should be rendered, not shown as raw **bold**
        assert!(all_text.contains("important"));
        assert!(!all_text.contains("**important**"));
    }

    #[test]
    fn scheduler_stage_interview_gets_stage_header() {
        let theme = Theme::default();
        let message = message_with_stage_meta("interview", Some("prometheus"), Some(1), Some(4));
        let rendered = render_message_text_part(
            &message,
            "## prometheus · Interview\n\nWhat scope do you want?",
            &theme,
            Color::Blue,
        );

        assert!(!rendered.allow_semantic_highlighting);
        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Prometheus · Interview"));
        assert!(all_text.contains("(1/4)"));
        assert!(all_text.contains("What scope do you want?"));
    }

    #[test]
    fn scheduler_stage_plan_gets_stage_header() {
        let theme = Theme::default();
        let message = message_with_stage_meta("plan", Some("prometheus"), Some(2), Some(4));
        let rendered = render_message_text_part(
            &message,
            "## prometheus · Plan\n\n### Step 1\nMigrate schema",
            &theme,
            Color::Blue,
        );

        assert!(!rendered.allow_semantic_highlighting);
        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Prometheus · Plan"));
        assert!(all_text.contains("(2/4)"));
        assert!(all_text.contains("Migrate schema"));
    }

    #[test]
    fn scheduler_stage_synthesis_gets_success_accent() {
        let theme = Theme::default();
        let message = message_with_stage_meta("synthesis", Some("sisyphus"), Some(2), Some(2));
        let rendered = render_message_text_part(
            &message,
            "## sisyphus · Synthesis\n\nAll tasks completed.",
            &theme,
            Color::Blue,
        );

        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Sisyphus · Synthesis"));
        assert!(all_text.contains("(2/2)"));
        // Synthesis header should use success color
        let header_line = &rendered.lines[0];
        let accent_span = &header_line.spans[2]; // icon span is [1], title span is [2]
        assert_eq!(accent_span.style.fg, Some(theme.success));
    }

    #[test]
    fn scheduler_stage_execution_orchestration_gets_header() {
        let theme = Theme::default();
        let message = message_with_stage_meta(
            "execution-orchestration",
            Some("hephaestus"),
            Some(1),
            Some(1),
        );
        let rendered = render_message_text_part(
            &message,
            "## hephaestus · Execution Orchestration\n\nFixed the bug.",
            &theme,
            Color::Blue,
        );

        let all_text: String = rendered.lines.iter().map(|l| line_text(l)).collect();
        assert!(all_text.contains("Hephaestus · Execution"));
        assert!(all_text.contains("(1/1)"));
    }

    #[test]
    fn non_scheduler_message_renders_plain_markdown() {
        let message = Message {
            id: "m1".to_string(),
            role: MessageRole::Assistant,
            content: String::new(),
            created_at: Utc::now(),
            agent: None,
            model: None,
            mode: None,
            finish: None,
            error: None,
            completed_at: None,
            cost: 0.0,
            tokens: TokenUsage::default(),
            metadata: None,
            parts: Vec::new(),
        };
        let rendered = render_message_text_part(&message, "hello", &Theme::default(), Color::Blue);
        assert!(rendered.allow_semantic_highlighting);
        assert!(line_text(&rendered.lines[0]).contains("hello"));
    }
}
