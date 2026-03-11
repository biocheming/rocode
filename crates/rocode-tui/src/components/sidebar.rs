use std::collections::HashMap;
use std::sync::Arc;

use ratatui::prelude::Stylize;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use crate::branding::{APP_NAME, APP_SHORT_NAME, APP_VERSION_DATE};
use crate::context::{
    AppContext, LspConnectionStatus, McpConnectionStatus, MessageRole, TodoStatus,
};
use crate::theme::Theme;
use rocode_core::process_registry::ProcessKind;

pub struct Sidebar {
    context: Arc<AppContext>,
    session_id: String,
}

#[derive(Clone)]
struct SidebarToggleHit {
    line_index: usize,
    section_key: &'static str,
}

#[derive(Default)]
pub struct SidebarState {
    collapsed_sections: HashMap<&'static str, bool>,
    scroll_offset: usize,
    content_lines: usize,
    viewport_lines: usize,
    sidebar_area: Option<Rect>,
    sections_area: Option<Rect>,
    toggle_hits: Vec<SidebarToggleHit>,
    /// Index of the currently selected process in the process list.
    pub process_selected: usize,
    /// Whether the process panel has keyboard focus.
    pub process_focus: bool,
    /// Maps rendered line index → process list index (for click selection).
    process_line_hits: Vec<(usize, usize)>,
}

impl SidebarState {
    pub fn reset_hidden(&mut self) {
        self.sidebar_area = None;
        self.sections_area = None;
        self.toggle_hits.clear();
        self.scroll_offset = 0;
        self.content_lines = 0;
        self.viewport_lines = 0;
    }

    fn set_sidebar_area(&mut self, area: Rect) {
        self.sidebar_area = Some(area);
    }

    fn set_sections_layout(
        &mut self,
        sections_area: Rect,
        content_lines: usize,
        toggle_hits: Vec<SidebarToggleHit>,
    ) {
        self.sections_area = Some(sections_area);
        self.content_lines = content_lines;
        self.viewport_lines = usize::from(sections_area.height);
        self.toggle_hits = toggle_hits;
        self.clamp_scroll();
    }

    pub fn contains_sidebar_point(&self, col: u16, row: u16) -> bool {
        contains_point(self.sidebar_area, col, row)
    }

    pub fn handle_click(&mut self, col: u16, row: u16) -> bool {
        let Some(area) = self.sections_area else {
            return false;
        };
        if !contains_point(Some(area), col, row) {
            return false;
        }

        let relative_row = usize::from(row.saturating_sub(area.y));
        let line_index = self.scroll_offset.saturating_add(relative_row);

        // Check if the click is on a process item
        if let Some((_line_idx, proc_idx)) = self
            .process_line_hits
            .iter()
            .find(|(li, _)| *li == line_index)
        {
            self.process_selected = *proc_idx;
            self.process_focus = true;
            return true;
        }

        let Some(section_key) = self
            .toggle_hits
            .iter()
            .find(|hit| hit.line_index == line_index)
            .map(|hit| hit.section_key)
        else {
            return false;
        };

        let collapsed = self.collapsed_sections.entry(section_key).or_insert(false);
        *collapsed = !*collapsed;
        true
    }

    pub fn scroll_up_at(&mut self, col: u16, row: u16) -> bool {
        if !self.contains_sidebar_point(col, row) {
            return false;
        }
        self.scroll_up();
        true
    }

    pub fn scroll_down_at(&mut self, col: u16, row: u16) -> bool {
        if !self.contains_sidebar_point(col, row) {
            return false;
        }
        self.scroll_down();
        true
    }

    fn is_collapsed(&self, section_key: &'static str) -> bool {
        self.collapsed_sections
            .get(section_key)
            .copied()
            .unwrap_or(false)
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_down(&mut self) {
        let max_scroll = self.max_scroll();
        if self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
        }
    }

    fn max_scroll(&self) -> usize {
        self.content_lines.saturating_sub(self.viewport_lines)
    }

    fn clamp_scroll(&mut self) {
        let max_scroll = self.max_scroll();
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
    }

    pub fn process_select_up(&mut self) {
        self.process_selected = self.process_selected.saturating_sub(1);
    }

    pub fn process_select_down(&mut self, count: usize) {
        if count > 0 {
            self.process_selected = (self.process_selected + 1).min(count - 1);
        }
    }

    pub fn clamp_process_selected(&mut self, count: usize) {
        if count == 0 {
            self.process_selected = 0;
        } else if self.process_selected >= count {
            self.process_selected = count - 1;
        }
    }
}

struct SidebarSection {
    key: &'static str,
    title: &'static str,
    lines: Vec<Line<'static>>,
    summary: Option<String>,
    collapsible: bool,
}

impl Sidebar {
    pub fn new(context: Arc<AppContext>, session_id: String) -> Self {
        Self {
            context,
            session_id,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &mut SidebarState, floating: bool) {
        if area.width == 0 || area.height == 0 {
            state.reset_hidden();
            return;
        }

        state.set_sidebar_area(area);
        let theme = self.context.theme.read().clone();

        if !floating {
            let block = Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(theme.background_panel));
            frame.render_widget(block, area);
        }

        let inner = Rect {
            x: area.x.saturating_add(2),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(2),
        };
        if inner.width == 0 || inner.height == 0 {
            state.reset_hidden();
            return;
        }

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(inner);

        self.render_sections(frame, layout[0], &theme, state, floating);
        self.render_footer(frame, layout[1], &theme, floating);
    }

    fn render_sections(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        state: &mut SidebarState,
        floating: bool,
    ) {
        if area.width == 0 || area.height == 0 {
            state.set_sections_layout(area, 0, Vec::new());
            return;
        }

        let session_ctx = self.context.session.read();
        let mcp_servers = self.context.mcp_servers.read();
        let lsp_status = self.context.lsp_status.read();

        let session = session_ctx.sessions.get(&self.session_id);
        let messages = session_ctx
            .messages
            .get(&self.session_id)
            .cloned()
            .unwrap_or_default();

        let title = session
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "New Session".to_string());
        let mut session_lines = vec![Line::from(Span::styled(
            truncate_text(&title, area.width as usize),
            Style::default().fg(theme.text).bold(),
        ))];
        if let Some(session_meta) = session.and_then(|s| s.metadata.as_ref()) {
            if let Some(agent) = sidebar_metadata_text(session_meta, "agent") {
                session_lines.push(sidebar_meta_line(theme, "agent", agent));
            }
            if let Some(model) = sidebar_model_summary(session_meta) {
                session_lines.push(sidebar_meta_line(theme, "model", model));
            }
            if let Some(scheduler) = sidebar_scheduler_summary(session_meta) {
                session_lines.push(sidebar_meta_line(theme, "scheduler", scheduler));
            }
        }
        let mut sections: Vec<SidebarSection> = vec![SidebarSection {
            key: "session",
            title: "Session",
            lines: session_lines,
            summary: None,
            collapsible: false,
        }];

        if let Some(share) = session.and_then(|s| s.share.as_ref()) {
            sections.push(SidebarSection {
                key: "share",
                title: "Share",
                lines: vec![Line::from(Span::styled(
                    truncate_text(&share.url, area.width as usize),
                    Style::default().fg(theme.info),
                ))],
                summary: None,
                collapsible: false,
            });
        }

        let total_cost: f64 = messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::Assistant))
            .map(|m| m.cost)
            .sum();
        let total_tokens = messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::Assistant))
            .map(|m| {
                m.tokens.input
                    + m.tokens.output
                    + m.tokens.reasoning
                    + m.tokens.cache_read
                    + m.tokens.cache_write
            })
            .sum::<u64>();
        let model_context_limit = {
            let providers = self.context.providers.read();
            let current_model = self.context.current_model.read();
            let active_model = messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, MessageRole::Assistant))
                .and_then(|m| m.model.as_deref())
                .or(current_model.as_deref());
            active_model
                .and_then(|model_id| {
                    providers.iter().find_map(|provider| {
                        provider
                            .models
                            .iter()
                            .find(|model| {
                                model.id == *model_id
                                    || model
                                        .id
                                        .rsplit_once('/')
                                        .map(|(_, suffix)| suffix == model_id)
                                        .unwrap_or(false)
                            })
                            .map(|model| model.context_window)
                    })
                })
                .unwrap_or(0)
        };
        sections.push(SidebarSection {
            key: "context",
            title: "Context",
            lines: vec![
                {
                    let mut spans = vec![
                        Span::styled("Tokens ", Style::default().fg(theme.text_muted)),
                        Span::styled(format_number(total_tokens), Style::default().fg(theme.text)),
                    ];
                    if model_context_limit > 0 && total_tokens > 0 {
                        let used_pct = ((total_tokens as f64 / model_context_limit as f64) * 100.0)
                            .round() as u64;
                        spans.push(Span::styled(
                            format!("  {}%", used_pct),
                            Style::default().fg(theme.text_muted),
                        ));
                    }
                    Line::from(spans)
                },
                Line::from(vec![
                    Span::styled("Cost   ", Style::default().fg(theme.text_muted)),
                    Span::styled(
                        format!("${:.2}", total_cost),
                        Style::default().fg(theme.text),
                    ),
                ]),
            ],
            summary: None,
            collapsible: false,
        });

        let connected_mcp = mcp_servers
            .iter()
            .filter(|s| matches!(s.status, McpConnectionStatus::Connected))
            .count();
        let failed_mcp = mcp_servers
            .iter()
            .filter(|s| matches!(s.status, McpConnectionStatus::Failed))
            .count();
        let registration_needed_mcp = mcp_servers
            .iter()
            .filter(|s| matches!(s.status, McpConnectionStatus::NeedsClientRegistration))
            .count();
        let problematic_mcp = failed_mcp + registration_needed_mcp;
        let mut mcp_lines: Vec<Line<'static>> = Vec::new();
        if mcp_servers.is_empty() {
            mcp_lines.push(Line::from(Span::styled(
                "No MCP servers",
                Style::default().fg(theme.text_muted),
            )));
        } else {
            for server in mcp_servers.iter() {
                let (status_text, color) = match server.status {
                    McpConnectionStatus::Connected => ("connected", theme.success),
                    McpConnectionStatus::Failed => ("failed", theme.error),
                    McpConnectionStatus::NeedsAuth => ("needs auth", theme.warning),
                    McpConnectionStatus::NeedsClientRegistration => {
                        ("needs client ID", theme.warning)
                    }
                    McpConnectionStatus::Disabled => ("disabled", theme.text_muted),
                    McpConnectionStatus::Disconnected => ("disconnected", theme.text_muted),
                };
                mcp_lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(color)),
                    Span::styled(
                        truncate_text(&server.name, area.width.saturating_sub(14) as usize),
                        Style::default().fg(theme.text),
                    ),
                    Span::styled(
                        format!(" {}", status_text),
                        Style::default().fg(theme.text_muted),
                    ),
                ]));
            }
        }
        sections.push(SidebarSection {
            key: "mcp",
            title: "MCP",
            lines: mcp_lines,
            summary: Some(format!(
                "{} active, {} errors",
                connected_mcp, problematic_mcp
            )),
            collapsible: mcp_servers.len() > 2,
        });

        let connected_lsp = lsp_status
            .iter()
            .filter(|s| matches!(s.status, LspConnectionStatus::Connected))
            .count();
        let errored_lsp = lsp_status
            .iter()
            .filter(|s| matches!(s.status, LspConnectionStatus::Error))
            .count();
        let mut lsp_lines: Vec<Line<'static>> = Vec::new();
        if lsp_status.is_empty() {
            lsp_lines.push(Line::from(Span::styled(
                "No active LSP",
                Style::default().fg(theme.text_muted),
            )));
        } else {
            for server in lsp_status.iter() {
                let (status_text, color) = match server.status {
                    LspConnectionStatus::Connected => ("connected", theme.success),
                    LspConnectionStatus::Error => ("error", theme.error),
                };
                lsp_lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(color)),
                    Span::styled(
                        truncate_text(&server.id, area.width.saturating_sub(14) as usize),
                        Style::default().fg(theme.text),
                    ),
                    Span::styled(
                        format!(" {}", status_text),
                        Style::default().fg(theme.text_muted),
                    ),
                ]));
            }
        }
        sections.push(SidebarSection {
            key: "lsp",
            title: "LSP",
            lines: lsp_lines,
            summary: Some(format!(
                "{} connected, {} errors",
                connected_lsp, errored_lsp
            )),
            collapsible: lsp_status.len() > 2,
        });

        if let Some(todos) = session_ctx.todos.get(&self.session_id) {
            let pending = todos
                .iter()
                .filter(|todo| {
                    !matches!(todo.status, TodoStatus::Completed | TodoStatus::Cancelled)
                })
                .collect::<Vec<_>>();
            if !pending.is_empty() {
                let mut todo_lines: Vec<Line<'static>> = Vec::new();
                for todo in pending.iter().take(5) {
                    todo_lines.push(Line::from(vec![
                        Span::styled("☐ ", Style::default().fg(theme.warning)),
                        Span::styled(
                            truncate_text(&todo.content, area.width.saturating_sub(2) as usize),
                            Style::default().fg(theme.text_muted),
                        ),
                    ]));
                }
                sections.push(SidebarSection {
                    key: "todo",
                    title: "Todo",
                    lines: todo_lines,
                    summary: Some(format!("{} pending", pending.len())),
                    collapsible: pending.len() > 2,
                });
            }
        }

        if let Some(entries) = session_ctx.session_diff.get(&self.session_id) {
            if !entries.is_empty() {
                let mut file_lines: Vec<Line<'static>> = Vec::new();
                for entry in entries.iter().take(8) {
                    file_lines.push(Line::from(vec![
                        Span::styled(
                            truncate_text(&entry.file, area.width.saturating_sub(14) as usize),
                            Style::default().fg(theme.text),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("+{}", entry.additions),
                            Style::default().fg(theme.success),
                        ),
                        Span::raw("/"),
                        Span::styled(
                            format!("-{}", entry.deletions),
                            Style::default().fg(theme.error),
                        ),
                    ]));
                }
                sections.push(SidebarSection {
                    key: "diff",
                    title: "Modified Files",
                    lines: file_lines,
                    summary: Some(format!("{} files changed", entries.len())),
                    collapsible: entries.len() > 2,
                });
            }
        }

        // Processes section
        let proc_list = self.context.processes.read().clone();
        state.clamp_process_selected(proc_list.len());
        if !proc_list.is_empty() {
            let mut proc_lines: Vec<Line<'static>> = Vec::new();
            for (idx, proc) in proc_list.iter().enumerate() {
                let selected = state.process_focus && idx == state.process_selected;
                let prefix = if selected { "▸ " } else { "  " };
                let kind_color = match proc.kind {
                    ProcessKind::Plugin => theme.info,
                    ProcessKind::Bash => theme.success,
                    ProcessKind::Agent => theme.warning,
                    ProcessKind::Mcp => theme.info, // Same category as Plugin
                    ProcessKind::Lsp => theme.warning, // Same category as Agent
                };
                let name_width = area.width.saturating_sub(18) as usize;
                let stats = format!("{:4.1}% {:>3}MB", proc.cpu_percent, proc.memory_kb / 1024);
                let fg = if selected {
                    theme.text
                } else {
                    theme.text_muted
                };
                let row_bg = if selected {
                    Some(theme.background_element)
                } else {
                    None
                };
                let mk_style = |base: Style| -> Style {
                    if let Some(bg) = row_bg {
                        base.bg(bg)
                    } else {
                        base
                    }
                };
                proc_lines.push(Line::from(vec![
                    Span::styled(
                        prefix,
                        mk_style(Style::default().fg(if selected {
                            theme.primary
                        } else {
                            theme.text_muted
                        })),
                    ),
                    Span::styled("● ", mk_style(Style::default().fg(kind_color))),
                    Span::styled(
                        truncate_text(&proc.name, name_width),
                        mk_style(Style::default().fg(fg)),
                    ),
                    Span::styled(
                        format!(" {}", stats),
                        mk_style(Style::default().fg(theme.text_muted)),
                    ),
                ]));
            }
            sections.push(SidebarSection {
                key: "processes",
                title: "Processes",
                lines: proc_lines,
                summary: Some(format!("{} running", proc_list.len())),
                collapsible: proc_list.len() > 2,
            });
        }

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut line_index = 0usize;
        let mut toggle_hits: Vec<SidebarToggleHit> = Vec::new();
        let mut process_line_hits: Vec<(usize, usize)> = Vec::new();
        for section in sections {
            if !lines.is_empty() {
                lines.push(Line::from(""));
                line_index += 1;
            }

            let collapsed = section.collapsible && state.is_collapsed(section.key);
            let mut header = Vec::new();
            if section.collapsible {
                toggle_hits.push(SidebarToggleHit {
                    line_index,
                    section_key: section.key,
                });
                header.push(Span::styled(
                    if collapsed { "▶ " } else { "▼ " },
                    Style::default().fg(theme.text_muted),
                ));
            }
            header.push(Span::styled(
                section.title.to_string(),
                Style::default().fg(theme.text).bold(),
            ));
            if collapsed {
                if let Some(summary) = section.summary {
                    header.push(Span::styled(" · ", Style::default().fg(theme.text_muted)));
                    header.push(Span::styled(summary, Style::default().fg(theme.text_muted)));
                }
            }
            lines.push(Line::from(header));
            line_index += 1;

            if !collapsed {
                let is_processes = section.key == "processes";
                for (row_idx, row) in section.lines.into_iter().enumerate() {
                    if is_processes {
                        process_line_hits.push((line_index, row_idx));
                    }
                    lines.push(row);
                    line_index += 1;
                }
            }
        }

        let has_overflow = lines.len() > usize::from(area.height);
        let sections_text_area = if has_overflow && area.width > 1 {
            Rect {
                x: area.x,
                y: area.y,
                width: area.width.saturating_sub(1),
                height: area.height,
            }
        } else {
            area
        };
        let scrollbar_area = if has_overflow && area.width > 1 {
            Some(Rect {
                x: area.x + area.width.saturating_sub(1),
                y: area.y,
                width: 1,
                height: area.height,
            })
        } else {
            None
        };

        state.set_sections_layout(sections_text_area, lines.len(), toggle_hits);
        state.process_line_hits = process_line_hits;

        let mut paragraph = Paragraph::new(lines)
            .scroll((state.scroll_offset.min(usize::from(u16::MAX)) as u16, 0));
        if !floating {
            paragraph = paragraph
                .block(Block::default().borders(Borders::NONE))
                .style(Style::default().bg(theme.background_panel));
        }
        frame.render_widget(paragraph, sections_text_area);

        if let Some(scroll_area) = scrollbar_area {
            let mut scrollbar_state = ScrollbarState::new(state.content_lines)
                .position(state.scroll_offset)
                .viewport_content_length(state.viewport_lines.max(1));
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .track_style(Style::default().fg(theme.border_subtle))
                .thumb_symbol("█")
                .thumb_style(Style::default().fg(theme.primary));
            frame.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect, theme: &Theme, floating: bool) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let directory = self.context.directory.read().clone();
        let (prefix, leaf) = split_path_segments(&directory);
        let lines = vec![
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme.text_muted)),
                Span::styled(leaf, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("• ", Style::default().fg(theme.success)),
                Span::styled(
                    format!("{} ({}) ", APP_NAME, APP_SHORT_NAME),
                    Style::default().fg(theme.text).bold(),
                ),
                Span::styled(APP_VERSION_DATE, Style::default().fg(theme.text_muted)),
            ]),
        ];

        let mut paragraph = Paragraph::new(lines);
        if !floating {
            paragraph = paragraph.style(Style::default().bg(theme.background_panel));
        }
        frame.render_widget(paragraph, area);
    }
}

fn contains_point(area: Option<Rect>, col: u16, row: u16) -> bool {
    let Some(area) = area else {
        return false;
    };
    let max_x = area.x.saturating_add(area.width);
    let max_y = area.y.saturating_add(area.height);
    col >= area.x && col < max_x && row >= area.y && row < max_y
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::with_capacity(max_chars + 1);
    for ch in text.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn sidebar_metadata_text(
    metadata: &HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn sidebar_metadata_bool(metadata: &HashMap<String, serde_json::Value>, key: &str) -> bool {
    metadata
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn sidebar_model_summary(metadata: &HashMap<String, serde_json::Value>) -> Option<String> {
    let provider = sidebar_metadata_text(metadata, "model_provider");
    let model_id = sidebar_metadata_text(metadata, "model_id");
    match (provider, model_id) {
        (Some(provider), Some(model_id)) => Some(format!("{}/{}", provider, model_id)),
        (None, Some(model_id)) => Some(model_id),
        _ => None,
    }
}

fn sidebar_scheduler_summary(metadata: &HashMap<String, serde_json::Value>) -> Option<String> {
    if !sidebar_metadata_bool(metadata, "scheduler_applied") {
        return None;
    }

    let profile = sidebar_metadata_text(metadata, "scheduler_profile");
    let root_agent = sidebar_metadata_text(metadata, "scheduler_root_agent");
    let skill_tree_applied = sidebar_metadata_bool(metadata, "scheduler_skill_tree_applied");

    let mut parts = Vec::new();
    if let Some(profile) = profile {
        parts.push(profile);
    } else {
        parts.push("active".to_string());
    }
    if let Some(root_agent) = root_agent {
        parts.push(format!("root={}", root_agent));
    }
    if skill_tree_applied {
        parts.push("skill-tree".to_string());
    }
    Some(parts.join(" · "))
}

fn sidebar_meta_line(theme: &Theme, label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{} ", label.to_uppercase()),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(value, Style::default().fg(theme.text)),
    ])
}

fn split_path_segments(path: &str) -> (String, String) {
    if path.is_empty() {
        return (String::new(), String::new());
    }

    if let Some((prefix, leaf)) = path.rsplit_once('/') {
        if prefix.is_empty() {
            return ("/".to_string(), leaf.to_string());
        }
        return (format!("{}/", prefix), leaf.to_string());
    }

    if let Some((prefix, leaf)) = path.rsplit_once('\\') {
        if prefix.is_empty() {
            return ("\\".to_string(), leaf.to_string());
        }
        return (format!("{}\\", prefix), leaf.to_string());
    }

    (String::new(), path.to_string())
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + (digits.len() / 3));
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}
