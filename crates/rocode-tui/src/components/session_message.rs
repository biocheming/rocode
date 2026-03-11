use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::{
    style::Color,
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::markdown::MarkdownRenderer;
use crate::context::{Message, MessagePart};
use crate::theme::Theme;

/// Render a user message with shared left gutter shape.
pub fn render_user_message(
    msg: &Message,
    theme: &Theme,
    show_timestamps: bool,
    agent: Option<&str>,
    show_system_prompt: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let border_char = "┃ ";
    let border_style = Style::default().fg(user_border_color_for_agent(agent, theme));
    if show_system_prompt {
        let profile_name = metadata_text(msg, "resolved_scheduler_profile");
        let resolved_agent = metadata_text(msg, "resolved_agent");
        let system_prompt = metadata_text(msg, "resolved_system_prompt_preview")
            .or_else(|| metadata_text(msg, "resolved_system_prompt"));
        if let Some(system_prompt) = system_prompt {
            let title = system_prompt_title(system_prompt, profile_name.or(resolved_agent));
            let subtitle = system_prompt_subtitle(profile_name, resolved_agent);
            let prompt_preview =
                compact_system_prompt_preview(system_prompt, 3, 84, title.as_deref());

            if let Some(title) = title {
                lines.push(Line::from(vec![
                    Span::styled(border_char, border_style),
                    Span::styled(
                        title,
                        Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            if let Some(subtitle) = subtitle {
                lines.push(Line::from(vec![
                    Span::styled(border_char, border_style),
                    Span::styled(subtitle, Style::default().fg(theme.text_muted)),
                ]));
            }
            if !prompt_preview.trim().is_empty() {
                for prompt_line in MarkdownRenderer::new(theme.clone()).to_lines(&prompt_preview) {
                    let mut spans = vec![Span::styled(border_char, border_style)];
                    spans.push(Span::styled("↳ ", Style::default().fg(theme.text_muted)));
                    spans.extend(prompt_line.spans);
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::from(vec![
                Span::styled(border_char, border_style),
                Span::raw(""),
            ]));
        }
    }

    if msg.parts.is_empty() {
        for line_text in msg.content.lines() {
            lines.push(Line::from(vec![
                Span::styled(border_char, border_style),
                Span::styled(line_text.to_string(), Style::default().fg(theme.text)),
            ]));
        }
    } else {
        for part in &msg.parts {
            match part {
                MessagePart::Text { text } => {
                    let md_renderer = MarkdownRenderer::new(theme.clone());
                    let md_lines = md_renderer.to_lines(text);
                    for md_line in md_lines {
                        let mut spans = vec![Span::styled(border_char, border_style)];
                        spans.extend(md_line.spans);
                        lines.push(Line::from(spans));
                    }
                }
                MessagePart::File { path, mime } => {
                    lines.push(Line::from(vec![
                        Span::styled(border_char, border_style),
                        Span::styled(
                            mime_badge(mime),
                            Style::default().fg(theme.info).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(path.clone(), Style::default().fg(theme.text)),
                    ]));
                }
                MessagePart::Image { url } => {
                    lines.push(Line::from(vec![
                        Span::styled(border_char, border_style),
                        Span::styled("[image] ", Style::default().fg(theme.info)),
                        Span::styled(url.clone(), Style::default().fg(theme.text_muted)),
                    ]));
                }
                _ => {}
            }
        }
    }

    if show_timestamps {
        let ts = msg.created_at.format("%H:%M").to_string();
        if !lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(border_char, border_style),
                Span::styled(ts, Style::default().fg(theme.text_muted)),
            ]));
        }
    }

    lines
}

fn mime_badge(mime: &str) -> String {
    let short = if let Some(sub) = mime.strip_prefix("image/") {
        sub.to_uppercase()
    } else if let Some(sub) = mime.strip_prefix("text/") {
        sub.to_uppercase()
    } else if let Some(sub) = mime.strip_prefix("application/") {
        sub.to_uppercase()
    } else {
        mime.to_uppercase()
    };
    format!("[{}]", short)
}

fn user_border_color_for_agent(agent: Option<&str>, theme: &Theme) -> Color {
    let Some(agent) = agent else {
        return theme.primary;
    };
    if theme.agent_colors.is_empty() {
        return theme.primary;
    }
    let mut hasher = DefaultHasher::new();
    agent.hash(&mut hasher);
    let idx = (hasher.finish() as usize) % theme.agent_colors.len();
    theme.agent_colors[idx]
}

fn metadata_text<'a>(msg: &'a Message, key: &str) -> Option<&'a str> {
    msg.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn compact_system_prompt_preview(
    prompt: &str,
    max_lines: usize,
    max_chars_per_line: usize,
    skip_line: Option<&str>,
) -> String {
    let skipped = skip_line
        .map(|line| line.trim())
        .filter(|line| !line.is_empty());
    let mut preview_lines = Vec::new();
    let mut truncated = false;

    for line in prompt
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with('<') && line.ends_with('>') {
            continue;
        }
        let compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if compact.is_empty() {
            continue;
        }
        if skipped.is_some_and(|skip| compact == skip) {
            continue;
        }
        if preview_lines.len() >= max_lines {
            truncated = true;
            break;
        }
        let shortened: String = compact.chars().take(max_chars_per_line).collect();
        if shortened.len() < compact.len() {
            preview_lines.push(format!("{}…", shortened.trim_end()));
            truncated = true;
        } else {
            preview_lines.push(shortened);
        }
    }

    if preview_lines.is_empty() {
        return String::new();
    }

    if truncated && !preview_lines.last().is_some_and(|line| line.ends_with('…')) {
        if let Some(last) = preview_lines.last_mut() {
            last.push('…');
        }
    }

    preview_lines.join(
        "
",
    )
}

fn system_prompt_title(prompt: &str, profile_or_agent: Option<&str>) -> Option<String> {
    let profile = profile_or_agent.map(|value| value.trim().to_ascii_lowercase());
    let lines = prompt
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();

    let heading = profile.as_deref().and_then(|profile| {
        lines.iter().find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with('#') && lower.contains(profile)
        })
    });
    if let Some(line) = heading {
        return Some(line.trim_start_matches('#').trim().to_string());
    }

    let identity = profile.as_deref().and_then(|profile| {
        lines.iter().find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("you are") && lower.contains(profile)
        })
    });
    if let Some(line) = identity {
        return Some(
            line.trim_start_matches("You are")
                .trim()
                .trim_end_matches('.')
                .to_string(),
        );
    }

    lines
        .iter()
        .find(|line| line.starts_with("You are"))
        .map(|line| {
            line.trim_start_matches("You are")
                .trim()
                .trim_end_matches('.')
                .to_string()
        })
}

fn system_prompt_subtitle(
    profile_name: Option<&str>,
    resolved_agent: Option<&str>,
) -> Option<String> {
    if let Some(profile) = profile_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("{} mode", prettify_mode_name(profile)));
    }
    resolved_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|agent| format!("{} agent", prettify_mode_name(agent)))
}

fn prettify_mode_name(raw: &str) -> String {
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

#[cfg(test)]
mod tests {
    #[test]
    fn system_prompt_title_prefers_profile_identity_line() {
        let title = system_prompt_title(
            "You are ROCode's request router.
You are Prometheus - Strategic Planning Consultant.",
            Some("prometheus"),
        );
        assert_eq!(
            title.as_deref(),
            Some("Prometheus - Strategic Planning Consultant")
        );
    }

    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn render_user_message_hides_prompt_debug_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "resolved_scheduler_profile".to_string(),
            serde_json::json!("prometheus"),
        );
        metadata.insert(
            "resolved_system_prompt".to_string(),
            serde_json::json!("You are Prometheus"),
        );
        metadata.insert("resolved_user_prompt".to_string(), serde_json::json!("hi"));
        let msg = Message {
            id: "m1".to_string(),
            role: crate::context::MessageRole::User,
            content: "hi".to_string(),
            created_at: Utc::now(),
            agent: None,
            model: None,
            mode: None,
            finish: None,
            error: None,
            completed_at: None,
            cost: 0.0,
            tokens: crate::context::TokenUsage::default(),
            metadata: Some(metadata),
            parts: Vec::new(),
        };

        let lines = render_user_message(&msg, &Theme::default(), false, None, false);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();

        assert!(!rendered.contains("[Preset] ") && !rendered.contains("[Mode] "));
        assert!(!rendered.contains("[System Prompt]"));
        assert!(!rendered.contains("[Input Prompt]"));
    }

    #[test]
    fn render_user_message_shows_first_turn_system_prompt_when_requested() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "resolved_system_prompt_preview".to_string(),
            serde_json::json!(
                "You are Prometheus — strategic planning consultant.
Bias: interview first, clarify scope, then produce one reviewed work plan.
Boundary: planner-only; never execute code or modify non-markdown files."
            ),
        );
        let msg = Message {
            id: "m2".to_string(),
            role: crate::context::MessageRole::User,
            content: "hi".to_string(),
            created_at: Utc::now(),
            agent: None,
            model: None,
            mode: None,
            finish: None,
            error: None,
            completed_at: None,
            cost: 0.0,
            tokens: crate::context::TokenUsage::default(),
            metadata: Some(metadata),
            parts: Vec::new(),
        };

        let lines = render_user_message(&msg, &Theme::default(), false, None, true);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>();

        assert!(rendered.contains("Prometheus"));
        assert!(rendered.contains("You are Prometheus"));
        assert!(rendered.contains(
            "Bias: interview first, clarify scope, then produce one reviewed work plan."
        ));
        assert!(rendered
            .contains("Boundary: planner-only; never execute code or modify non-markdown files."));
        assert!(!rendered.contains("You are Prometheus's planning review layer."));
    }

    #[test]
    fn compact_system_prompt_preview_limits_lines_and_length() {
        let preview = compact_system_prompt_preview(
            "line one

line two is much longer than the limit should allow for the preview renderer
line three
line four",
            3,
            24,
            None,
        );

        assert!(preview.contains("line one"));
        assert!(preview.contains("line two is much longer…"));
        assert!(preview.contains("line three…"));
        assert!(!preview.contains("line four"));
    }
}
