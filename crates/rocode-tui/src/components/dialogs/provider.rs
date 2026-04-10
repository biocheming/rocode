use ratatui::prelude::Stylize;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};
use std::collections::HashSet;

use crate::api::{
    ConnectProtocolOption, ProviderConnectDraft, ProviderConnectSchemaResponse,
    ResolveProviderConnectResponse,
};
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub env_hint: String,
    pub base_url: Option<String>,
    pub protocol: Option<String>,
    pub model_count: usize,
    pub status: ProviderStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderStatus {
    Connected,
    Disconnected,
    Error,
}
/// Result of an API key submission attempt.
#[derive(Clone, Debug)]
pub enum SubmitResult {
    Success,
    Failed(String),
}

/// Tracks the current step in the "Add custom provider..." flow.
#[derive(Clone, Debug)]
pub enum CustomProviderStep {
    ProviderId,
    BaseUrl,
    Protocol,
    ApiKey,
}

/// Accumulates values across the 4-step custom provider flow.
#[derive(Clone, Debug)]
pub struct CustomProviderState {
    pub provider_id: String,
    pub base_url: String,
    /// Selected protocol ID (e.g., "openai", "anthropic")
    pub protocol: String,
    pub api_key: String,
    pub step: CustomProviderStep,
}

/// Pending submit payload - either known provider or custom provider.
#[derive(Clone, Debug)]
pub enum PendingSubmit {
    Known {
        provider_id: String,
        api_key: String,
    },
    Custom {
        provider_id: String,
        base_url: String,
        protocol: String,
        api_key: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderConnectMode {
    Known,
    Custom,
}

pub struct ProviderDialog {
    pub providers: Vec<Provider>,
    pub resolved_matches: Vec<Provider>,
    pub protocol_options: Vec<ConnectProtocolOption>,
    pub state: ListState,
    pub open: bool,
    pub selected_provider: Option<Provider>,
    pub api_key_input: String,
    pub input_mode: bool,
    /// Brief feedback after submitting a key.
    pub submit_result: Option<SubmitResult>,
    /// Set when the user selects "Add custom provider..." from the list.
    /// Contains the accumulated input values and current step.
    pub custom_state: Option<CustomProviderState>,
    /// Index into the fixed protocol list during Protocol step.
    pub protocol_index: usize,
    pub connect_mode: ProviderConnectMode,
    pub search_query: String,
    pub resolve_error: Option<String>,
}

impl ProviderDialog {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            resolved_matches: Vec::new(),
            protocol_options: Vec::new(),
            state: ListState::default(),
            open: false,
            selected_provider: None,
            api_key_input: String::new(),
            input_mode: false,
            submit_result: None,
            custom_state: None,
            protocol_index: 0,
            connect_mode: ProviderConnectMode::Known,
            search_query: String::new(),
            resolve_error: None,
        }
    }

    /// Build the provider list from the set of currently connected provider IDs.
    /// Always shows all known providers; marks those in `connected` as Connected.
    /// This is the fallback when the `/provider/known` endpoint is unavailable.
    pub fn populate(&mut self, connected: &HashSet<String>) {
        self.providers
            .retain(|provider| connected.contains(&provider.id));
        for provider in &mut self.providers {
            provider.status = if connected.contains(&provider.id) {
                ProviderStatus::Connected
            } else {
                ProviderStatus::Disconnected
            };
        }
    }

    /// Build the provider list from the dynamic `models.dev` catalogue.
    /// Connected providers are sorted to the top, then alphabetically.
    pub fn populate_from_known(&mut self, entries: Vec<crate::api::KnownProviderEntry>) {
        self.providers = entries
            .into_iter()
            .map(|e| Provider {
                env_hint: e.env.first().cloned().unwrap_or_default(),
                base_url: e.base_url,
                protocol: e.protocol,
                model_count: e.model_count,
                status: if e.connected {
                    ProviderStatus::Connected
                } else {
                    ProviderStatus::Disconnected
                },
                id: e.id,
                name: e.name,
            })
            .collect();
        // Sort: connected first, then alphabetically by name
        self.providers.sort_by(|a, b| {
            let a_connected = matches!(a.status, ProviderStatus::Connected);
            let b_connected = matches!(b.status, ProviderStatus::Connected);
            b_connected
                .cmp(&a_connected)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    pub fn populate_from_connect_schema(&mut self, schema: ProviderConnectSchemaResponse) {
        self.populate_from_known(schema.providers);
        self.protocol_options = schema.protocols;
        if self.protocol_index >= self.protocol_options.len() {
            self.protocol_index = 0;
        }
        self.clear_resolution();
    }

    pub fn open(&mut self) {
        self.open = true;
        self.input_mode = false;
        self.api_key_input.clear();
        self.selected_provider = None;
        self.custom_state = None;
        self.protocol_index = 0;
        self.submit_result = None;
        self.connect_mode = ProviderConnectMode::Known;
        self.search_query.clear();
        self.clear_resolution();
        self.state
            .select((!self.visible_providers().is_empty()).then_some(0));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input_mode = false;
        self.api_key_input.clear();
        self.selected_provider = None;
        self.custom_state = None;
        self.protocol_index = 0;
        self.submit_result = None;
        self.connect_mode = ProviderConnectMode::Known;
        self.search_query.clear();
        self.clear_resolution();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_input_mode(&self) -> bool {
        self.input_mode
    }

    pub fn accepts_text_input(&self) -> bool {
        self.input_mode || (self.custom_state.is_some() && !self.is_protocol_step())
    }

    pub fn set_providers(&mut self, providers: Vec<Provider>) {
        self.providers = providers;
        let visible_len = self.visible_providers().len();
        if visible_len == 0 {
            self.state.select(None);
        } else if self.state.selected().is_none() {
            self.state.select(Some(0));
        } else if let Some(selected) = self.state.selected() {
            self.state
                .select(Some(selected.min(visible_len.saturating_sub(1))));
        }
    }

    fn visible_providers(&self) -> &[Provider] {
        if self.search_query.trim().is_empty() {
            &self.providers
        } else {
            &self.resolved_matches
        }
    }

    fn sync_selection_to_visible(&mut self) {
        let visible_len = self.visible_providers().len();
        if visible_len == 0 {
            self.state.select(None);
            return;
        }
        let next = self
            .state
            .selected()
            .unwrap_or(0)
            .min(visible_len.saturating_sub(1));
        self.state.select(Some(next));
    }

    pub fn move_up(&mut self) {
        if self.connect_mode != ProviderConnectMode::Known {
            return;
        }
        if let Some(selected) = self.state.selected() {
            let new = selected.saturating_sub(1);
            self.state.select(Some(new));
        }
    }

    pub fn move_down(&mut self) {
        if self.connect_mode != ProviderConnectMode::Known {
            return;
        }
        if let Some(selected) = self.state.selected() {
            let max = self.visible_providers().len().saturating_sub(1);
            let new = (selected + 1).min(max);
            self.state.select(Some(new));
        }
    }

    pub fn selected_provider(&self) -> Option<Provider> {
        self.state
            .selected()
            .and_then(|index| self.visible_providers().get(index))
            .cloned()
    }

    /// Enter input mode for the currently highlighted provider.
    pub fn enter_input_mode(&mut self) {
        if self.connect_mode == ProviderConnectMode::Custom {
            self.start_custom_flow();
            return;
        }
        // Known provider flow
        if let Some(provider) = self.selected_provider() {
            self.enter_input_mode_for_provider(provider);
        }
    }

    pub fn enter_input_mode_for_provider(&mut self, provider: Provider) {
        self.selected_provider = Some(provider);
        self.api_key_input.clear();
        self.submit_result = None;
        self.input_mode = true;
    }

    /// Go back from input mode to the provider list.
    pub fn exit_input_mode(&mut self) {
        self.input_mode = false;
        self.api_key_input.clear();
        self.submit_result = None;
    }

    /// Exit custom flow and return to list.
    pub fn exit_custom_flow(&mut self) {
        self.custom_state = None;
        self.protocol_index = 0;
        self.submit_result = None;
    }

    pub fn toggle_mode_next(&mut self) {
        match self.connect_mode {
            ProviderConnectMode::Known => self.set_mode(ProviderConnectMode::Custom),
            ProviderConnectMode::Custom => self.set_mode(ProviderConnectMode::Known),
        }
    }

    pub fn toggle_mode_prev(&mut self) {
        self.toggle_mode_next();
    }

    pub fn set_mode(&mut self, mode: ProviderConnectMode) {
        if self.connect_mode == mode {
            return;
        }
        self.connect_mode = mode;
        self.submit_result = None;
        if self.connect_mode == ProviderConnectMode::Known {
            self.sync_selection_to_visible();
        }
    }

    fn start_custom_flow(&mut self) {
        self.start_custom_flow_with_prefill(String::new(), String::new(), String::new());
    }

    pub fn start_custom_flow_with_prefill(
        &mut self,
        provider_id: String,
        base_url: String,
        protocol: String,
    ) {
        let protocol_index = self
            .protocol_options
            .iter()
            .position(|option| option.id == protocol)
            .unwrap_or(0);
        self.custom_state = Some(CustomProviderState {
            provider_id,
            base_url,
            protocol,
            api_key: String::new(),
            step: CustomProviderStep::ProviderId,
        });
        self.protocol_index = protocol_index;
        self.submit_result = None;
    }

    /// Go back to previous step in custom flow.
    pub fn back_custom_flow(&mut self) {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => {
                    // At first step - exit custom flow entirely
                    self.exit_custom_flow();
                }
                CustomProviderStep::BaseUrl => {
                    state.step = CustomProviderStep::ProviderId;
                }
                CustomProviderStep::Protocol => {
                    state.step = CustomProviderStep::BaseUrl;
                }
                CustomProviderStep::ApiKey => {
                    state.step = CustomProviderStep::Protocol;
                }
            }
        }
    }

    /// Advance to next step in custom flow. Returns true if now at final step.
    pub fn advance_custom_flow(&mut self) -> bool {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => {
                    state.step = CustomProviderStep::BaseUrl;
                    false
                }
                CustomProviderStep::BaseUrl => {
                    state.step = CustomProviderStep::Protocol;
                    false
                }
                CustomProviderStep::Protocol => {
                    // Store selected protocol before advancing
                    let protocol_id = self
                        .protocol_options
                        .get(self.protocol_index)
                        .map(|option| option.id.clone())
                        .unwrap_or_else(|| "openai".to_string());
                    state.protocol = protocol_id;
                    state.step = CustomProviderStep::ApiKey;
                    false
                }
                CustomProviderStep::ApiKey => {
                    true // Already at final step
                }
            }
        } else {
            false
        }
    }

    /// Check if currently at the final step (API key entry).
    pub fn is_final_step(&self) -> bool {
        matches!(
            self.custom_state.as_ref().map(|s| &s.step),
            Some(CustomProviderStep::ApiKey)
        )
    }

    /// Check if currently at protocol selection step.
    pub fn is_protocol_step(&self) -> bool {
        matches!(
            self.custom_state.as_ref().map(|s| &s.step),
            Some(CustomProviderStep::Protocol)
        )
    }

    /// Move protocol selection up.
    pub fn protocol_index_dec(&mut self) {
        self.protocol_index = self.protocol_index.saturating_sub(1);
    }

    /// Move protocol selection down.
    pub fn protocol_index_inc(&mut self) {
        self.protocol_index =
            (self.protocol_index + 1).min(self.protocol_options.len().saturating_sub(1));
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => state.provider_id.push(c),
                CustomProviderStep::BaseUrl => state.base_url.push(c),
                CustomProviderStep::Protocol => {}
                CustomProviderStep::ApiKey => state.api_key.push(c),
            }
        } else {
            self.api_key_input.push(c);
        }
        self.submit_result = None;
    }

    pub fn pop_char(&mut self) {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => {
                    state.provider_id.pop();
                }
                CustomProviderStep::BaseUrl => {
                    state.base_url.pop();
                }
                CustomProviderStep::Protocol => {}
                CustomProviderStep::ApiKey => {
                    state.api_key.pop();
                }
            }
        } else {
            self.api_key_input.pop();
        }
        self.submit_result = None;
    }

    /// Set the input directly (for clipboard paste).
    pub fn set_input(&mut self, text: String) {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => state.provider_id = text,
                CustomProviderStep::BaseUrl => state.base_url = text,
                CustomProviderStep::Protocol => {}
                CustomProviderStep::ApiKey => state.api_key = text,
            }
        } else {
            self.api_key_input = text;
        }
        self.submit_result = None;
    }

    pub fn push_search_char(&mut self, c: char) {
        self.search_query.push(c);
        self.state.select(Some(0));
        self.submit_result = None;
        self.resolve_error = None;
    }

    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
        if self.search_query.trim().is_empty() {
            self.clear_resolution();
        } else {
            self.state.select(Some(0));
        }
        self.submit_result = None;
        self.resolve_error = None;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.clear_resolution();
        self.submit_result = None;
    }

    pub fn clear_resolution(&mut self) {
        self.resolved_matches.clear();
        self.resolve_error = None;
        if self.providers.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }

    pub fn apply_resolve_response(&mut self, response: ResolveProviderConnectResponse) {
        self.resolved_matches = response
            .matches
            .into_iter()
            .map(provider_from_draft_match)
            .collect();
        self.resolve_error = None;
        if self.visible_providers().is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(0));
        }
    }

    pub fn set_resolve_error(&mut self, error: String) {
        self.resolved_matches.clear();
        self.resolve_error = Some(error);
        self.state.select(None);
    }

    /// Returns the pending submit payload if ready.
    /// For known providers: checks input_mode and api_key_input.
    /// For custom providers: checks custom_state is at ApiKey step with non-empty key.
    pub fn pending_submit(&self) -> Option<PendingSubmit> {
        // Custom provider flow
        if let Some(ref state) = self.custom_state {
            if matches!(state.step, CustomProviderStep::ApiKey) && !state.api_key.trim().is_empty()
            {
                let protocol = if state.protocol.is_empty() {
                    // Use currently selected protocol
                    self.protocol_options
                        .get(self.protocol_index)
                        .map(|option| option.id.clone())
                        .unwrap_or_else(|| "openai".to_string())
                } else {
                    state.protocol.clone()
                };
                return Some(PendingSubmit::Custom {
                    provider_id: state.provider_id.clone(),
                    base_url: state.base_url.clone(),
                    protocol,
                    api_key: state.api_key.trim().to_string(),
                });
            }
            return None;
        }

        // Known provider flow
        if !self.input_mode || self.api_key_input.trim().is_empty() {
            return None;
        }
        self.selected_provider
            .as_ref()
            .map(|p| PendingSubmit::Known {
                provider_id: p.id.clone(),
                api_key: self.api_key_input.trim().to_string(),
            })
    }

    pub fn set_submit_result(&mut self, result: SubmitResult) {
        self.submit_result = Some(result);
    }
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.open {
            return;
        }

        let visible_count = self.visible_providers().len().max(1) as u16;
        let height = (visible_count + 8)
            .clamp(12, 22)
            .min(area.height.saturating_sub(4));
        let width = 56u16.min(area.width.saturating_sub(4));
        let popup_area = super::centered_rect(width, height, area);
        let block = Block::default()
            .title(" Connect Provider ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border));
        let content_area = super::dialog_inner(block.inner(popup_area));
        frame.render_widget(Clear, popup_area);

        if self.custom_state.is_some() {
            self.render_custom_input_mode(frame, popup_area, content_area, block, theme);
        } else if self.input_mode {
            self.render_input_mode(frame, popup_area, content_area, block, theme);
        } else {
            self.render_list_mode(frame, popup_area, content_area, block, theme);
        }
    }

    fn render_input_mode(
        &self,
        frame: &mut Frame,
        popup_area: Rect,
        content_area: Rect,
        block: Block,
        theme: &Theme,
    ) {
        let provider_name = self
            .selected_provider
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("");
        let env_hint = self
            .selected_provider
            .as_ref()
            .map(|p| p.env_hint.as_str())
            .unwrap_or("");
        let base_url = self
            .selected_provider
            .as_ref()
            .and_then(|p| p.base_url.as_deref())
            .unwrap_or("");
        let protocol = self
            .selected_provider
            .as_ref()
            .and_then(|p| p.protocol.as_deref())
            .unwrap_or("");

        // Mask the key: show first 4 chars then asterisks
        let masked = if self.api_key_input.len() > 4 {
            let (head, tail) = self.api_key_input.split_at(4);
            format!("{}{}", head, "*".repeat(tail.len()))
        } else {
            self.api_key_input.clone()
        };

        let mut lines = vec![
            Line::from(Span::styled(
                provider_name,
                Style::default().fg(theme.primary).bold(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Env: ", Style::default().fg(theme.text_muted)),
                Span::styled(env_hint, Style::default().fg(theme.warning)),
            ]),
            Line::from(vec![
                Span::styled("Base URL: ", Style::default().fg(theme.text_muted)),
                Span::styled(base_url, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("Protocol: ", Style::default().fg(theme.text_muted)),
                Span::styled(protocol, Style::default().fg(theme.text)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter API Key:",
                Style::default().fg(theme.text),
            )),
            Line::from(Span::styled(
                format!("> {}█", masked),
                Style::default().fg(theme.primary),
            )),
            Line::from(""),
        ];

        // Show submit result feedback
        if let Some(ref result) = self.submit_result {
            match result {
                SubmitResult::Success => {
                    lines.push(Line::from(Span::styled(
                        "✓ Connected successfully!",
                        Style::default().fg(theme.success),
                    )));
                }
                SubmitResult::Failed(msg) => {
                    let truncated = if msg.len() > 48 {
                        format!("{}...", &msg[..45])
                    } else {
                        msg.clone()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("✗ {}", truncated),
                        Style::default().fg(theme.error),
                    )));
                }
            }
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.text)),
            Span::styled(" connect  ", Style::default().fg(theme.text_muted)),
            Span::styled("Esc", Style::default().fg(theme.text)),
            Span::styled(" back", Style::default().fg(theme.text_muted)),
        ]));

        frame.render_widget(
            block.style(Style::default().bg(theme.background_panel)),
            popup_area,
        );
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(theme.background_panel));
        frame.render_widget(paragraph, content_area);
    }

    fn render_list_mode(
        &self,
        frame: &mut Frame,
        popup_area: Rect,
        content_area: Rect,
        block: Block,
        theme: &Theme,
    ) {
        frame.render_widget(
            block.style(Style::default().bg(theme.background_panel)),
            popup_area,
        );

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(6),
                Constraint::Length(3),
            ])
            .split(content_area);

        let known_style = if self.connect_mode == ProviderConnectMode::Known {
            Style::default()
                .fg(theme.primary)
                .bg(theme.background_element)
                .bold()
        } else {
            Style::default().fg(theme.text_muted)
        };
        let custom_style = if self.connect_mode == ProviderConnectMode::Custom {
            Style::default()
                .fg(theme.primary)
                .bg(theme.background_element)
                .bold()
        } else {
            Style::default().fg(theme.text_muted)
        };

        let subtitle = match self.connect_mode {
            ProviderConnectMode::Known => {
                "Search a known provider. Enter connects quickly; A opens advanced editing."
            }
            ProviderConnectMode::Custom => {
                "Create a custom provider with provider id, base URL, protocol and API key."
            }
        };

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("Known", known_style),
                    Span::raw("  "),
                    Span::styled("Custom", custom_style),
                ]),
                Line::from(Span::styled(
                    subtitle,
                    Style::default().fg(theme.text_muted),
                )),
                Line::from(Span::styled(
                    format!("Search: {}", self.search_query),
                    Style::default().fg(theme.text),
                )),
            ])
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(theme.background_panel)),
            sections[0],
        );

        match self.connect_mode {
            ProviderConnectMode::Known => {
                if self.providers.is_empty() {
                    frame.render_widget(
                        Paragraph::new("No known providers available. Switch to Custom to enter an endpoint manually.")
                            .wrap(Wrap { trim: false })
                            .style(Style::default().fg(theme.text_muted).bg(theme.background_panel)),
                        sections[1],
                    );
                } else if self.visible_providers().is_empty() {
                    let message = if let Some(error) = &self.resolve_error {
                        format!("Resolve failed: {}", error)
                    } else if self.search_query.trim().is_empty() {
                        "No known providers available.".to_string()
                    } else {
                        "No known match. Press Enter to use the current search text as a custom provider id."
                            .to_string()
                    };
                    frame.render_widget(
                        Paragraph::new(message).wrap(Wrap { trim: false }).style(
                            Style::default()
                                .fg(theme.text_muted)
                                .bg(theme.background_panel),
                        ),
                        sections[1],
                    );
                } else {
                    let visible = self.visible_providers();
                    let selected = self.state.selected().unwrap_or(0);
                    let list_area = Rect {
                        x: sections[1].x,
                        y: sections[1].y,
                        width: sections[1].width.saturating_sub(1),
                        height: sections[1].height,
                    };

                    if list_area.height > 0 {
                        let viewport = list_area.height as usize;
                        let mut scroll = 0usize;
                        if selected >= viewport {
                            scroll = selected.saturating_sub(viewport.saturating_sub(1));
                        }

                        for (row, (index, provider)) in visible
                            .iter()
                            .enumerate()
                            .skip(scroll)
                            .take(viewport)
                            .enumerate()
                        {
                            let is_selected = index == selected;
                            let status_icon = match provider.status {
                                ProviderStatus::Connected => "●",
                                ProviderStatus::Disconnected => "◯",
                                ProviderStatus::Error => "✗",
                            };
                            let status_color = match provider.status {
                                ProviderStatus::Connected => theme.success,
                                ProviderStatus::Disconnected => theme.text_muted,
                                ProviderStatus::Error => theme.error,
                            };
                            let row_area = Rect {
                                x: list_area.x,
                                y: list_area.y + row as u16,
                                width: list_area.width,
                                height: 1,
                            };
                            let line = Line::from(vec![
                                Span::styled(status_icon, Style::default().fg(status_color)),
                                Span::raw(" "),
                                Span::styled(
                                    &provider.name,
                                    Style::default()
                                        .fg(if is_selected {
                                            theme.primary
                                        } else {
                                            theme.text
                                        })
                                        .bg(if is_selected {
                                            theme.background_element
                                        } else {
                                            theme.background_panel
                                        }),
                                ),
                                Span::styled(
                                    format!(" · {}", provider.id),
                                    Style::default().fg(theme.text_muted).bg(if is_selected {
                                        theme.background_element
                                    } else {
                                        theme.background_panel
                                    }),
                                ),
                            ]);
                            frame.render_widget(Paragraph::new(line), row_area);
                        }

                        if visible.len() > viewport {
                            let scroll_area = Rect {
                                x: list_area.x + list_area.width,
                                y: list_area.y,
                                width: 1,
                                height: list_area.height,
                            };
                            let mut scrollbar_state = ScrollbarState::new(visible.len())
                                .position(scroll)
                                .viewport_content_length(viewport);
                            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                                .begin_symbol(None)
                                .end_symbol(None)
                                .track_symbol(Some("│"))
                                .track_style(Style::default().fg(theme.border_subtle))
                                .thumb_symbol("█")
                                .thumb_style(Style::default().fg(theme.primary));
                            frame.render_stateful_widget(
                                scrollbar,
                                scroll_area,
                                &mut scrollbar_state,
                            );
                        }
                    }
                }
            }
            ProviderConnectMode::Custom => {
                let mut lines = vec![
                    Line::from(Span::styled(
                        "Custom provider setup",
                        Style::default().fg(theme.text).bold(),
                    )),
                    Line::from(""),
                    Line::from("You will be prompted for:"),
                    Line::from("  1. Provider ID"),
                    Line::from("  2. Base URL"),
                    Line::from("  3. Protocol"),
                    Line::from("  4. API Key"),
                ];
                if let Some(result) = &self.submit_result {
                    lines.push(Line::from(""));
                    match result {
                        SubmitResult::Success => lines.push(Line::from(Span::styled(
                            "✓ Connected successfully!",
                            Style::default().fg(theme.success),
                        ))),
                        SubmitResult::Failed(msg) => lines.push(Line::from(Span::styled(
                            format!("✗ {}", msg),
                            Style::default().fg(theme.error),
                        ))),
                    }
                }

                frame.render_widget(
                    Paragraph::new(lines)
                        .wrap(Wrap { trim: false })
                        .style(Style::default().bg(theme.background_panel)),
                    sections[1],
                );
            }
        }

        let footer = match self.connect_mode {
            ProviderConnectMode::Known => {
                let total = self.providers.len();
                let visible = self.visible_providers().len();
                let selected = self.state.selected().map(|index| index + 1).unwrap_or(0);
                vec![
                    Line::from(Span::styled(
                        if self.search_query.trim().is_empty() {
                            format!("{total} known providers · {selected}/{visible} selected")
                        } else {
                            format!(
                                "{visible} matches · {selected}/{visible} selected · {total} total known"
                            )
                        },
                        Style::default().fg(theme.text_muted),
                    )),
                    Line::from(Span::styled(
                        "Type to search  ←/→ or Tab switch mode  ↑↓ select  Enter quick connect/custom fallback  A advanced  Esc clear/close",
                        Style::default().fg(theme.text_muted),
                    )),
                ]
            }
            ProviderConnectMode::Custom => vec![
                Line::from(Span::styled(
                    "Manual provider setup",
                    Style::default().fg(theme.text_muted),
                )),
                Line::from(Span::styled(
                    "←/→ or Tab switch mode  Enter start custom setup  Esc close",
                    Style::default().fg(theme.text_muted),
                )),
            ],
        };
        frame.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }).style(
                Style::default()
                    .fg(theme.text_muted)
                    .bg(theme.background_panel),
            ),
            sections[2],
        );
    }

    fn render_custom_input_mode(
        &self,
        frame: &mut Frame,
        popup_area: Rect,
        content_area: Rect,
        block: Block,
        theme: &Theme,
    ) {
        let Some(ref state) = self.custom_state else {
            return;
        };

        // Determine step indicator text
        let (step_label, step_num, total_steps) = match state.step {
            CustomProviderStep::ProviderId => ("Provider ID", 1, 4),
            CustomProviderStep::BaseUrl => ("Base URL", 2, 4),
            CustomProviderStep::Protocol => ("Protocol", 3, 4),
            CustomProviderStep::ApiKey => ("API Key", 4, 4),
        };

        let mut lines = vec![
            Line::from(Span::styled(
                format!("Add Custom Provider (Step {}/{})", step_num, total_steps),
                Style::default().fg(theme.primary).bold(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("{}:", step_label),
                Style::default().fg(theme.text),
            )),
        ];

        // Render current input field
        match state.step {
            CustomProviderStep::ProviderId => {
                lines.push(Line::from(Span::styled(
                    format!("> {}█", state.provider_id),
                    Style::default().fg(theme.primary),
                )));
            }
            CustomProviderStep::BaseUrl => {
                lines.push(Line::from(Span::styled(
                    format!("> {}█", state.base_url),
                    Style::default().fg(theme.primary),
                )));
            }
            CustomProviderStep::Protocol => {
                // Render protocol list with selection
                lines.push(Line::from(""));
                for (i, option) in self.protocol_options.iter().enumerate() {
                    let is_selected = i == self.protocol_index;
                    let style = if is_selected {
                        Style::default()
                            .fg(theme.primary)
                            .bg(theme.background_element)
                    } else {
                        Style::default().fg(theme.text_muted)
                    };
                    let prefix = if is_selected { "› " } else { "  " };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, option.name),
                        style,
                    )));
                }
            }
            CustomProviderStep::ApiKey => {
                // Mask the API key
                let masked = if state.api_key.len() > 4 {
                    let (head, tail) = state.api_key.split_at(4);
                    format!("{}{}", head, "*".repeat(tail.len()))
                } else {
                    state.api_key.clone()
                };
                lines.push(Line::from(Span::styled(
                    format!("> {}█", masked),
                    Style::default().fg(theme.primary),
                )));
            }
        }

        lines.push(Line::from(""));

        // Show submit result feedback
        if let Some(ref result) = self.submit_result {
            match result {
                SubmitResult::Success => {
                    lines.push(Line::from(Span::styled(
                        "✓ Connected successfully!",
                        Style::default().fg(theme.success),
                    )));
                }
                SubmitResult::Failed(msg) => {
                    let truncated = if msg.len() > 48 {
                        format!("{}...", &msg[..45])
                    } else {
                        msg.clone()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("✗ {}", truncated),
                        Style::default().fg(theme.error),
                    )));
                }
            }
            lines.push(Line::from(""));
        }

        // Navigation hints
        if matches!(state.step, CustomProviderStep::Protocol) {
            lines.push(Line::from(vec![
                Span::styled("↑↓", Style::default().fg(theme.text)),
                Span::styled(" select  ", Style::default().fg(theme.text_muted)),
                Span::styled("Enter", Style::default().fg(theme.text)),
                Span::styled(" next  ", Style::default().fg(theme.text_muted)),
                Span::styled("Esc", Style::default().fg(theme.text)),
                Span::styled(" back", Style::default().fg(theme.text_muted)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Enter", Style::default().fg(theme.text)),
                Span::styled(
                    if step_num == total_steps {
                        " connect  "
                    } else {
                        " next  "
                    },
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled("Esc", Style::default().fg(theme.text)),
                Span::styled(" back", Style::default().fg(theme.text_muted)),
            ]));
        }

        frame.render_widget(
            block.style(Style::default().bg(theme.background_panel)),
            popup_area,
        );
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(theme.background_panel));
        frame.render_widget(paragraph, content_area);
    }
}

impl Default for ProviderDialog {
    fn default() -> Self {
        Self::new()
    }
}

fn provider_from_draft_match(entry: crate::api::KnownProviderEntry) -> Provider {
    Provider {
        env_hint: entry.env.first().cloned().unwrap_or_default(),
        base_url: entry.base_url,
        protocol: entry.protocol,
        model_count: entry.model_count,
        status: if entry.connected {
            ProviderStatus::Connected
        } else {
            ProviderStatus::Disconnected
        },
        id: entry.id,
        name: entry.name,
    }
}

pub fn provider_from_connect_draft(draft: &ProviderConnectDraft) -> Provider {
    Provider {
        id: draft.provider_id.clone(),
        name: draft
            .name
            .clone()
            .unwrap_or_else(|| draft.provider_id.clone()),
        env_hint: draft.env.first().cloned().unwrap_or_default(),
        base_url: draft.base_url.clone(),
        protocol: draft.protocol.clone(),
        model_count: draft.model_count,
        status: if draft.connected {
            ProviderStatus::Connected
        } else {
            ProviderStatus::Disconnected
        },
    }
}
