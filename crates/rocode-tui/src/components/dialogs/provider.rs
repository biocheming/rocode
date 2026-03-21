use ratatui::prelude::Stylize;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::collections::HashSet;

use crate::theme::Theme;

/// Known providers with their display name and primary env var.
/// Sorted by popularity (matching OpenCode's ordering).
const KNOWN_PROVIDERS: &[(&str, &str, &str)] = &[
    ("openai", "OpenAI", "OPENAI_API_KEY"),
    ("google", "Google AI", "GOOGLE_API_KEY"),
    ("github-copilot", "GitHub Copilot", "GITHUB_COPILOT_TOKEN"),
    ("openrouter", "OpenRouter", "OPENROUTER_API_KEY"),
    ("vercel", "Vercel AI", "VERCEL_API_KEY"),
    ("azure", "Azure OpenAI", "AZURE_API_KEY"),
    ("amazon-bedrock", "Amazon Bedrock", "AWS_ACCESS_KEY_ID"),
    ("deepseek", "DeepSeek", "DEEPSEEK_API_KEY"),
    ("mistral", "Mistral AI", "MISTRAL_API_KEY"),
    ("groq", "Groq", "GROQ_API_KEY"),
    ("xai", "X.AI (Grok)", "XAI_API_KEY"),
    ("cohere", "Cohere", "COHERE_API_KEY"),
    ("together", "Together AI", "TOGETHER_API_KEY"),
    ("deepinfra", "DeepInfra", "DEEPINFRA_API_KEY"),
    ("cerebras", "Cerebras", "CEREBRAS_API_KEY"),
    ("perplexity", "Perplexity", "PERPLEXITY_API_KEY"),
    ("gitlab", "GitLab Duo", "GITLAB_TOKEN"),
    (
        "google-vertex",
        "Google Vertex AI",
        "GOOGLE_VERTEX_ACCESS_TOKEN",
    ),
];

#[derive(Clone, Debug)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub env_hint: String,
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
    Known { provider_id: String, api_key: String },
    Custom {
        provider_id: String,
        base_url: String,
        protocol: String,
        api_key: String,
    },
}

const CUSTOM_PROVIDER_SENTINEL: &str = "+ Add custom provider...";

/// Fixed list of protocol types for custom provider selection.
const PROTOCOL_OPTIONS: &[(ProtocolId, ProtocolName)] = &[
    ("openai", "OpenAI"),
    ("anthropic", "Anthropic"),
    ("google", "Google"),
    ("bedrock", "Bedrock"),
    ("vertex", "Vertex"),
    ("github-copilot", "GitHub Copilot"),
    ("gitlab", "GitLab"),
];

type ProtocolId = &'static str;
type ProtocolName = &'static str;

pub struct ProviderDialog {
    pub providers: Vec<Provider>,
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
}

impl ProviderDialog {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            state: ListState::default(),
            open: false,
            selected_provider: None,
            api_key_input: String::new(),
            input_mode: false,
            submit_result: None,
            custom_state: None,
            protocol_index: 0,
        }
    }

    /// Build the provider list from the set of currently connected provider IDs.
    /// Always shows all known providers; marks those in `connected` as Connected.
    /// This is the fallback when the `/provider/known` endpoint is unavailable.
    pub fn populate(&mut self, connected: &HashSet<String>) {
        self.providers = KNOWN_PROVIDERS
            .iter()
            .map(|(id, name, env)| Provider {
                id: id.to_string(),
                name: name.to_string(),
                env_hint: env.to_string(),
                status: if connected.contains(*id) {
                    ProviderStatus::Connected
                } else {
                    ProviderStatus::Disconnected
                },
            })
            .collect();
    }

    /// Build the provider list from the dynamic `models.dev` catalogue.
    /// Connected providers are sorted to the top, then alphabetically.
    pub fn populate_from_known(&mut self, entries: Vec<crate::api::KnownProviderEntry>) {
        self.providers = entries
            .into_iter()
            .map(|e| Provider {
                env_hint: e.env.first().cloned().unwrap_or_default(),
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

    pub fn open(&mut self) {
        self.open = true;
        self.input_mode = false;
        self.api_key_input.clear();
        self.selected_provider = None;
        self.custom_state = None;
        self.protocol_index = 0;
        self.submit_result = None;
        self.state.select(Some(0));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input_mode = false;
        self.api_key_input.clear();
        self.selected_provider = None;
        self.custom_state = None;
        self.protocol_index = 0;
        self.submit_result = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_input_mode(&self) -> bool {
        self.input_mode
    }

    pub fn set_providers(&mut self, providers: Vec<Provider>) {
        self.providers = providers;
    }

    pub fn move_up(&mut self) {
        if let Some(selected) = self.state.selected() {
            let new = selected.saturating_sub(1);
            self.state.select(Some(new));
        }
    }

    pub fn move_down(&mut self) {
        if let Some(selected) = self.state.selected() {
            // Allow going one past providers.len() to select sentinel
            let max = self.providers.len();
            let new = (selected + 1).min(max);
            self.state.select(Some(new));
        }
    }

    pub fn selected_provider(&self) -> Option<&Provider> {
        self.state.selected().and_then(|i| self.providers.get(i))
    }

    /// Enter input mode for the currently highlighted provider.
    /// If sentinel is selected, starts custom provider flow.
    pub fn enter_input_mode(&mut self) {
        if self.state.selected() == Some(self.providers.len()) {
            // Sentinel selected - start custom flow at step 1
            self.custom_state = Some(CustomProviderState {
                provider_id: String::new(),
                base_url: String::new(),
                protocol: String::new(),
                api_key: String::new(),
                step: CustomProviderStep::ProviderId,
            });
            self.protocol_index = 0;
            self.submit_result = None;
            return;
        }
        // Known provider flow
        if let Some(p) = self.selected_provider() {
            self.selected_provider = Some(p.clone());
            self.api_key_input.clear();
            self.submit_result = None;
            self.input_mode = true;
        }
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
                    let protocol_id = PROTOCOL_OPTIONS
                        .get(self.protocol_index)
                        .map(|(id, _)| *id)
                        .unwrap_or("openai")
                        .to_string();
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
        matches!(self.custom_state.as_ref().map(|s| &s.step), Some(CustomProviderStep::ApiKey))
    }

    /// Check if currently at protocol selection step.
    pub fn is_protocol_step(&self) -> bool {
        matches!(self.custom_state.as_ref().map(|s| &s.step), Some(CustomProviderStep::Protocol))
    }

    /// Move protocol selection up.
    pub fn protocol_index_dec(&mut self) {
        self.protocol_index = self.protocol_index.saturating_sub(1);
    }

    /// Move protocol selection down.
    pub fn protocol_index_inc(&mut self) {
        self.protocol_index = (self.protocol_index + 1).min(PROTOCOL_OPTIONS.len().saturating_sub(1));
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(ref mut state) = self.custom_state {
            match state.step {
                CustomProviderStep::ProviderId => state.provider_id.push(c),
                CustomProviderStep::BaseUrl => state.base_url.push(c),
                CustomProviderStep::Protocol => state.protocol.push(c),
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
                CustomProviderStep::ProviderId => { state.provider_id.pop(); }
                CustomProviderStep::BaseUrl => { state.base_url.pop(); }
                CustomProviderStep::Protocol => { state.protocol.pop(); }
                CustomProviderStep::ApiKey => { state.api_key.pop(); }
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
                CustomProviderStep::Protocol => state.protocol = text,
                CustomProviderStep::ApiKey => state.api_key = text,
            }
        } else {
            self.api_key_input = text;
        }
        self.submit_result = None;
    }

    /// Returns the pending submit payload if ready.
    /// For known providers: checks input_mode and api_key_input.
    /// For custom providers: checks custom_state is at ApiKey step with non-empty key.
    pub fn pending_submit(&self) -> Option<PendingSubmit> {
        // Custom provider flow
        if let Some(ref state) = self.custom_state {
            if matches!(state.step, CustomProviderStep::ApiKey) && !state.api_key.trim().is_empty() {
                let protocol = if state.protocol.is_empty() {
                    // Use currently selected protocol
                    PROTOCOL_OPTIONS
                        .get(self.protocol_index)
                        .map(|(id, _)| id.to_string())
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
        self.selected_provider.as_ref().map(|p| {
            PendingSubmit::Known {
                provider_id: p.id.clone(),
                api_key: self.api_key_input.trim().to_string(),
            }
        })
    }

    pub fn set_submit_result(&mut self, result: SubmitResult) {
        self.submit_result = Some(result);
    }
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.open {
            return;
        }

        let height = 22u16.min(area.height.saturating_sub(4));
        let width = 56u16.min(area.width.saturating_sub(4));
        let popup_area = super::centered_rect(width, height, area);
        let block = Block::default()
            .title(" Connect Provider ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border));
        let content_area = super::dialog_inner(block.inner(popup_area));

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
        let mut items: Vec<ListItem> = self
            .providers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let status_icon = match p.status {
                    ProviderStatus::Connected => "●",
                    ProviderStatus::Disconnected => "◯",
                    ProviderStatus::Error => "✗",
                };
                let status_color = match p.status {
                    ProviderStatus::Connected => theme.success,
                    ProviderStatus::Disconnected => theme.text_muted,
                    ProviderStatus::Error => theme.error,
                };
                let is_selected = self.state.selected() == Some(i);
                let name_style = if is_selected {
                    Style::default()
                        .fg(theme.primary)
                        .bg(theme.background_element)
                } else {
                    Style::default().fg(theme.text)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(status_icon, Style::default().fg(status_color)),
                    Span::raw(" "),
                    Span::styled(&p.name, name_style),
                ]))
            })
            .collect();

        // Append sentinel row for "Add custom provider..."
        let sentinel_idx = self.providers.len();
        let is_sentinel_selected = self.state.selected() == Some(sentinel_idx);
        let sentinel_style = if is_sentinel_selected {
            Style::default()
                .fg(theme.primary)
                .bg(theme.background_element)
        } else {
            Style::default().fg(theme.text_muted)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled("+", Style::default().fg(theme.success)),
            Span::raw(" "),
            Span::styled(CUSTOM_PROVIDER_SENTINEL, sentinel_style),
        ])));

        frame.render_widget(
            block.style(Style::default().bg(theme.background_panel)),
            popup_area,
        );
        let list = List::new(items).highlight_style(Style::default().fg(theme.primary));
        frame.render_widget(list, content_area);
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
                format!("Add Custom Provider (Step {}/{})" , step_num, total_steps),
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
                for (i, (_, name)) in PROTOCOL_OPTIONS.iter().enumerate() {
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
                        format!("{}{}", prefix, name),
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
                Span::styled(if step_num == total_steps { " connect  " } else { " next  " }, Style::default().fg(theme.text_muted)),
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
