use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::ApiClient;
use crate::api::SessionExecutionTopology;
use crate::context::{ChildSessionInfo, KeybindRegistry, SessionContext};
use crate::event::EventBus;
use crate::router::Router;
use crate::theme::Theme;
use rocode_config::{Config as AppConfig, UiPreferencesConfig, UiRecentModelConfig};
use rocode_core::process_registry::ProcessInfo;

#[derive(Clone)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

#[derive(Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub status: McpConnectionStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum McpConnectionStatus {
    Connected,
    Disconnected,
    Failed,
    NeedsAuth,
    NeedsClientRegistration,
    Disabled,
}

#[derive(Clone)]
pub struct LspStatus {
    pub id: String,
    pub root: String,
    pub status: LspConnectionStatus,
}

#[derive(Clone, Debug)]
pub enum LspConnectionStatus {
    Connected,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarMode {
    Auto,
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageDensity {
    Compact,
    Cozy,
}

impl MessageDensity {
    pub fn from_str_lossy(s: &str) -> Self {
        if s.eq_ignore_ascii_case("cozy") {
            Self::Cozy
        } else {
            Self::Compact
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Cozy => "cozy",
        }
    }
}

pub struct AppContext {
    pub theme: RwLock<Theme>,
    pub theme_name: RwLock<String>,
    pub router: RwLock<Router>,
    pub keybind: RwLock<KeybindRegistry>,
    pub session: RwLock<SessionContext>,
    pub providers: RwLock<Vec<ProviderInfo>>,
    pub mcp_servers: RwLock<Vec<McpServerStatus>>,
    pub lsp_status: RwLock<Vec<LspStatus>>,
    pub event_bus: EventBus,
    pub current_agent: RwLock<String>,
    pub current_scheduler_profile: RwLock<Option<String>>,
    pub current_model: RwLock<Option<String>>,
    pub current_provider: RwLock<Option<String>>,
    pub current_variant: RwLock<Option<String>>,
    pub directory: RwLock<String>,
    pub show_sidebar: RwLock<bool>,
    pub show_header: RwLock<bool>,
    pub show_scrollbar: RwLock<bool>,
    pub tips_hidden: RwLock<bool>,
    pub sidebar_mode: RwLock<SidebarMode>,
    pub animations_enabled: RwLock<bool>,
    pub pending_permissions: RwLock<usize>,
    pub queued_prompts: RwLock<HashMap<String, usize>>,
    pub show_timestamps: RwLock<bool>,
    pub show_thinking: RwLock<bool>,
    pub show_tool_details: RwLock<bool>,
    pub message_density: RwLock<MessageDensity>,
    pub semantic_highlight: RwLock<bool>,
    recent_models: RwLock<Vec<(String, String)>>,
    pub has_connected_provider: RwLock<bool>,
    pub processes: RwLock<Vec<ProcessInfo>>,
    pub child_sessions: RwLock<Vec<ChildSessionInfo>>,
    pub execution_topology: RwLock<Option<SessionExecutionTopology>>,
    /// Server-side aggregated runtime state, fetched via `GET /session/{id}/runtime`.
    /// Replaces local derivation of active_tool_calls, pending_question,
    /// pending_permission, and child_sessions from individual SSE events.
    pub session_runtime: RwLock<Option<crate::api::SessionRuntimeState>>,
    pub api_client: RwLock<Option<Arc<ApiClient>>>,
}

impl AppContext {
    pub fn new() -> Self {
        let default_theme_name = default_theme_name();
        let default_theme = Theme::by_name(&default_theme_name).unwrap_or_else(Theme::dark);
        Self {
            theme: RwLock::new(default_theme),
            theme_name: RwLock::new(default_theme_name),
            router: RwLock::new(Router::new()),
            keybind: RwLock::new(KeybindRegistry::new()),
            session: RwLock::new(SessionContext::new()),
            providers: RwLock::new(Vec::new()),
            mcp_servers: RwLock::new(Vec::new()),
            lsp_status: RwLock::new(Vec::new()),
            event_bus: EventBus::new(),
            current_agent: RwLock::new(String::new()),
            current_scheduler_profile: RwLock::new(None),
            current_model: RwLock::new(None),
            current_provider: RwLock::new(None),
            current_variant: RwLock::new(None),
            directory: RwLock::new(String::new()),
            show_sidebar: RwLock::new(true),
            show_header: RwLock::new(true),
            show_scrollbar: RwLock::new(true),
            tips_hidden: RwLock::new(false),
            sidebar_mode: RwLock::new(SidebarMode::Auto),
            animations_enabled: RwLock::new(true),
            pending_permissions: RwLock::new(0),
            queued_prompts: RwLock::new(HashMap::new()),
            show_timestamps: RwLock::new(false),
            show_thinking: RwLock::new(true),
            show_tool_details: RwLock::new(true),
            message_density: RwLock::new(MessageDensity::Compact),
            semantic_highlight: RwLock::new(false),
            recent_models: RwLock::new(Vec::new()),
            has_connected_provider: RwLock::new(false),
            processes: RwLock::new(Vec::new()),
            child_sessions: RwLock::new(Vec::new()),
            execution_topology: RwLock::new(None),
            session_runtime: RwLock::new(None),
            api_client: RwLock::new(None),
        }
    }

    pub fn navigate(&self, route: crate::router::Route) {
        self.router.write().navigate(route);
    }

    pub fn current_route(&self) -> crate::router::Route {
        self.router.read().current().clone()
    }

    pub fn toggle_sidebar(&self) {
        let mut sidebar = self.show_sidebar.write();
        *sidebar = !*sidebar;
    }

    pub fn toggle_header(&self) {
        let value = {
            let mut show = self.show_header.write();
            *show = !*show;
            *show
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            show_header: Some(value),
            ..Default::default()
        });
    }

    pub fn toggle_scrollbar(&self) {
        let value = {
            let mut show = self.show_scrollbar.write();
            *show = !*show;
            *show
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            show_scrollbar: Some(value),
            ..Default::default()
        });
    }

    pub fn toggle_tips_hidden(&self) {
        let value = {
            let mut hidden = self.tips_hidden.write();
            *hidden = !*hidden;
            *hidden
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            tips_hidden: Some(value),
            ..Default::default()
        });
    }

    pub fn set_model(&self, model: String, provider: String) {
        self.set_model_selection(model, Some(provider));
    }

    pub fn set_model_selection(&self, model: String, provider: Option<String>) {
        *self.current_model.write() = Some(model);
        *self.current_provider.write() = provider;
    }

    pub fn set_model_variant(&self, variant: Option<String>) {
        *self.current_variant.write() = variant;
    }

    pub fn current_model_variant(&self) -> Option<String> {
        self.current_variant.read().clone()
    }

    pub fn set_agent(&self, agent: String) {
        *self.current_agent.write() = agent;
        *self.current_scheduler_profile.write() = None;
    }

    pub fn set_scheduler_profile(&self, profile: Option<String>) {
        *self.current_scheduler_profile.write() = profile;
        if self.current_scheduler_profile.read().is_some() {
            self.current_agent.write().clear();
        }
    }

    pub fn toggle_animations(&self) {
        let mut enabled = self.animations_enabled.write();
        *enabled = !*enabled;
    }

    pub fn set_pending_permissions(&self, count: usize) {
        *self.pending_permissions.write() = count;
    }

    pub fn set_queued_prompts(&self, session_id: &str, count: usize) {
        let mut queued = self.queued_prompts.write();
        if count == 0 {
            queued.remove(session_id);
        } else {
            queued.insert(session_id.to_string(), count);
        }
    }

    pub fn queued_prompts_for_session(&self, session_id: &str) -> usize {
        self.queued_prompts
            .read()
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    pub fn set_has_connected_provider(&self, connected: bool) {
        *self.has_connected_provider.write() = connected;
    }

    pub fn toggle_timestamps(&self) {
        let mut show = self.show_timestamps.write();
        *show = !*show;
        self.persist_ui_preferences(UiPreferencesConfig {
            show_timestamps: Some(*show),
            ..Default::default()
        });
    }

    pub fn toggle_thinking(&self) {
        let value = {
            let mut show = self.show_thinking.write();
            *show = !*show;
            *show
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            show_thinking: Some(value),
            ..Default::default()
        });
    }

    pub fn toggle_tool_details(&self) {
        let value = {
            let mut show = self.show_tool_details.write();
            *show = !*show;
            *show
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            show_tool_details: Some(value),
            ..Default::default()
        });
    }

    pub fn toggle_message_density(&self) {
        let density_str = {
            let mut density = self.message_density.write();
            *density = match *density {
                MessageDensity::Compact => MessageDensity::Cozy,
                MessageDensity::Cozy => MessageDensity::Compact,
            };
            density.as_str().to_string()
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            message_density: Some(density_str),
            ..Default::default()
        });
    }

    pub fn toggle_semantic_highlight(&self) {
        let value = {
            let mut enabled = self.semantic_highlight.write();
            *enabled = !*enabled;
            *enabled
        };
        self.persist_ui_preferences(UiPreferencesConfig {
            semantic_highlight: Some(value),
            ..Default::default()
        });
    }

    pub fn load_recent_models(&self) -> Vec<(String, String)> {
        self.recent_models.read().clone()
    }

    pub fn save_recent_models(&self, recent: &[(String, String)]) {
        let updated = recent.to_vec();
        {
            *self.recent_models.write() = updated.clone();
        }
        self.persist_ui_preferences(UiPreferencesConfig {
            recent_models: updated
                .into_iter()
                .map(|(provider, model)| UiRecentModelConfig { provider, model })
                .collect(),
            ..Default::default()
        });
    }

    pub fn toggle_theme_mode(&self) -> bool {
        let current = normalize_theme_name(&self.current_theme_name());
        let Some((base, variant)) = split_theme_variant(&current) else {
            return false;
        };
        let next = if variant == "dark" { "light" } else { "dark" };
        self.commit_theme_by_name(&format!("{base}@{next}"))
    }

    pub fn set_theme_by_name(&self, name: &str) -> bool {
        if let Some(theme) = Theme::by_name(name) {
            *self.theme.write() = theme;
            *self.theme_name.write() = normalize_theme_name(name);
            return true;
        }
        false
    }

    pub fn commit_theme_by_name(&self, name: &str) -> bool {
        if !self.set_theme_by_name(name) {
            return false;
        }
        self.persist_ui_preferences(UiPreferencesConfig {
            theme: Some(normalize_theme_name(name)),
            ..Default::default()
        });
        true
    }

    pub fn current_theme_name(&self) -> String {
        self.theme_name.read().clone()
    }

    pub fn available_theme_names(&self) -> Vec<String> {
        let mut names = Theme::builtin_theme_names()
            .into_iter()
            .flat_map(|name| [format!("{name}@dark"), format!("{name}@light")])
            .collect::<Vec<_>>();
        names.sort_by_key(|a| a.to_lowercase());
        names
    }

    pub fn set_api_client(&self, client: Arc<ApiClient>) {
        *self.api_client.write() = Some(client);
    }

    pub fn get_api_client(&self) -> Option<Arc<ApiClient>> {
        self.api_client.read().clone()
    }

    pub fn apply_config(&self, config: &AppConfig) {
        let ui = config.ui_preferences.as_ref();
        let theme_name = ui
            .and_then(|prefs| prefs.theme.as_deref())
            .map(normalize_theme_name)
            .unwrap_or_else(default_theme_name);
        if !self.set_theme_by_name(&theme_name) {
            let fallback = default_theme_name();
            let _ = self.set_theme_by_name(&fallback);
        }

        *self.show_header.write() = ui.and_then(|prefs| prefs.show_header).unwrap_or(true);
        *self.show_scrollbar.write() = ui.and_then(|prefs| prefs.show_scrollbar).unwrap_or(true);
        *self.tips_hidden.write() = ui.and_then(|prefs| prefs.tips_hidden).unwrap_or(false);
        *self.show_timestamps.write() = ui.and_then(|prefs| prefs.show_timestamps).unwrap_or(false);
        *self.show_thinking.write() = ui.and_then(|prefs| prefs.show_thinking).unwrap_or(true);
        *self.show_tool_details.write() =
            ui.and_then(|prefs| prefs.show_tool_details).unwrap_or(true);
        *self.message_density.write() = MessageDensity::from_str_lossy(
            ui.and_then(|prefs| prefs.message_density.as_deref())
                .unwrap_or("compact"),
        );
        *self.semantic_highlight.write() = ui
            .and_then(|prefs| prefs.semantic_highlight)
            .unwrap_or(false);
        *self.recent_models.write() = ui
            .map(|prefs| {
                prefs
                    .recent_models
                    .iter()
                    .map(|entry| (entry.provider.clone(), entry.model.clone()))
                    .collect()
            })
            .unwrap_or_default();
    }

    pub fn sync_ui_preferences_from_server(&self) -> anyhow::Result<()> {
        let client = self
            .get_api_client()
            .ok_or_else(|| anyhow::anyhow!("API client unavailable"))?;
        let config = client.get_config()?;
        self.apply_config(&config);
        Ok(())
    }

    fn persist_ui_preferences(&self, prefs: UiPreferencesConfig) {
        if let Err(err) = self.patch_ui_preferences(prefs) {
            tracing::warn!(%err, "failed to persist TUI ui preferences");
        }
    }

    fn patch_ui_preferences(&self, prefs: UiPreferencesConfig) -> anyhow::Result<()> {
        let client = self
            .get_api_client()
            .ok_or_else(|| anyhow::anyhow!("API client unavailable"))?;
        let patch = serde_json::to_value(AppConfig {
            ui_preferences: Some(prefs),
            ..Default::default()
        })?;
        let updated = client.patch_config(&patch)?;
        self.apply_config(&updated);
        Ok(())
    }

    /// Get active tool calls from the server-side session runtime state.
    /// Returns an empty HashMap if session_runtime is not available.
    pub fn get_active_tool_calls(&self) -> HashMap<String, ToolCallInfo> {
        self.session_runtime
            .read()
            .as_ref()
            .map(|runtime| {
                runtime
                    .active_tools
                    .iter()
                    .map(|tool| {
                        (
                            tool.tool_call_id.clone(),
                            ToolCallInfo {
                                id: tool.tool_call_id.clone(),
                                tool_name: tool.tool_name.clone(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get pending permission from the server-side session runtime state.
    /// Returns None if session_runtime is not available or no pending permission.
    pub fn get_pending_permission(&self) -> Option<(String, PermissionRequestInfo)> {
        self.session_runtime.read().as_ref().and_then(|runtime| {
            runtime.pending_permission.as_ref().map(|perm| {
                (
                    perm.permission_id.clone(),
                    PermissionRequestInfo {
                        id: perm.permission_id.clone(),
                        session_id: runtime.session_id.clone(),
                        tool: String::new(), // Extract from info if needed
                        input: perm.info.clone(),
                        message: String::new(),
                    },
                )
            })
        })
    }

    /// Check if there's a pending question from the server-side session runtime state.
    pub fn has_pending_question(&self) -> bool {
        self.session_runtime
            .read()
            .as_ref()
            .map(|r| r.pending_question.is_some())
            .unwrap_or(false)
    }

    /// Get pending question request_id from the server-side session runtime state.
    pub fn get_pending_question_id(&self) -> Option<String> {
        self.session_runtime
            .read()
            .as_ref()
            .and_then(|r| r.pending_question.as_ref().map(|q| q.request_id.clone()))
    }
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about an active tool call, used for cancel dialog.
#[derive(Clone, Debug)]
pub struct ToolCallInfo {
    pub id: String,
    pub tool_name: String,
}

/// Information about a permission request.
#[derive(Clone, Debug)]
pub struct PermissionRequestInfo {
    pub id: String,
    pub session_id: String,
    pub tool: String,
    pub input: serde_json::Value,
    pub message: String,
}

fn default_theme_name() -> String {
    format!("opencode@{}", detect_terminal_theme_mode())
}

fn normalize_theme_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return default_theme_name();
    }

    if let Some((base, variant)) = split_theme_variant(trimmed) {
        return format!("{base}@{variant}");
    }

    if trimmed.eq_ignore_ascii_case("dark") {
        return "opencode@dark".to_string();
    }
    if trimmed.eq_ignore_ascii_case("light") {
        return "opencode@light".to_string();
    }

    format!("{trimmed}@dark")
}

fn detect_terminal_theme_mode() -> &'static str {
    if let Ok(mode) =
        std::env::var("ROCODE_THEME_MODE").or_else(|_| std::env::var("OPENCODE_THEME_MODE"))
    {
        if mode.eq_ignore_ascii_case("light") {
            return "light";
        }
        if mode.eq_ignore_ascii_case("dark") {
            return "dark";
        }
    }

    // Common terminal convention: COLORFGBG="fg;bg", where bg in 0..=6 is dark
    // and 7..=15 is light.
    if let Ok(colorfgbg) = std::env::var("COLORFGBG") {
        if let Some(last) = colorfgbg.split(';').next_back() {
            if let Ok(code) = last.parse::<u8>() {
                return if code <= 6 { "dark" } else { "light" };
            }
        }
    }

    "dark"
}

fn split_theme_variant(name: &str) -> Option<(&str, &str)> {
    let (base, variant) = name.rsplit_once('@').or_else(|| name.rsplit_once(':'))?;
    if base.is_empty() || !matches!(variant, "dark" | "light") {
        return None;
    }
    Some((base, variant))
}
