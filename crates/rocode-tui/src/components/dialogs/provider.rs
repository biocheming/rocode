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
    ("anthropic", "Anthropic", "ANTHROPIC_API_KEY"),
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

pub struct ProviderDialog {
    pub providers: Vec<Provider>,
    pub state: ListState,
    pub open: bool,
    pub selected_provider: Option<Provider>,
    pub api_key_input: String,
    pub input_mode: bool,
    /// Brief feedback after submitting a key.
    pub submit_result: Option<SubmitResult>,
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
        self.submit_result = None;
        self.state.select(Some(0));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.input_mode = false;
        self.api_key_input.clear();
        self.selected_provider = None;
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
            let new = (selected + 1).min(self.providers.len().saturating_sub(1));
            self.state.select(Some(new));
        }
    }

    pub fn selected_provider(&self) -> Option<&Provider> {
        self.state.selected().and_then(|i| self.providers.get(i))
    }

    /// Enter API-key input mode for the currently highlighted provider.
    pub fn enter_input_mode(&mut self) {
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

    pub fn push_char(&mut self, c: char) {
        self.api_key_input.push(c);
        self.submit_result = None;
    }

    pub fn pop_char(&mut self) {
        self.api_key_input.pop();
        self.submit_result = None;
    }

    /// Returns the (provider_id, api_key) pair if ready to submit.
    pub fn pending_submit(&self) -> Option<(String, String)> {
        if !self.input_mode || self.api_key_input.trim().is_empty() {
            return None;
        }
        self.selected_provider
            .as_ref()
            .map(|p| (p.id.clone(), self.api_key_input.trim().to_string()))
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

        if self.input_mode {
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
        let items: Vec<ListItem> = self
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

        frame.render_widget(
            block.style(Style::default().bg(theme.background_panel)),
            popup_area,
        );
        let list = List::new(items).highlight_style(Style::default().fg(theme.primary));
        frame.render_widget(list, content_area);
    }
}

impl Default for ProviderDialog {
    fn default() -> Self {
        Self::new()
    }
}
