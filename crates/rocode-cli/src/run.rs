use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rocode_agent::executor::AgentError as AgentExecutorError;
use rocode_agent::{
    AgentExecutor, AgentInfo, AgentMessage, AgentRegistry, Conversation, MessageRole,
    PersistedSubsessionState,
};
use rocode_command::cli_markdown::MarkdownStreamer;
use rocode_command::cli_panel::{
    CliPanelFrame, pad_right_display, truncate_display, wrap_display_text,
};
use rocode_command::cli_permission::build_cli_permission_callback;
use rocode_command::cli_prompt::{PromptFrame, PromptSession, PromptSessionEvent};
use rocode_command::cli_select::{
    interactive_multi_select, interactive_select, SelectOption, SelectResult,
};
use rocode_command::cli_spinner::SpinnerGuard;
use rocode_command::cli_style::CliStyle;
use rocode_command::interactive::{parse_interactive_command, InteractiveCommand};
use rocode_command::output_blocks::{
    render_cli_block_rich, MessageBlock, MessagePhase, MessageRole as OutputMessageRole,
    OutputBlock, QueueItemBlock, SchedulerStageBlock, StatusBlock,
};
use rocode_command::{CommandContext, CommandRegistry};
use rocode_config::loader::load_config;
use rocode_config::{Config, SkillTreeNodeConfig};
use rocode_core::agent_task_registry::{global_task_registry, AgentTaskStatus};
use rocode_orchestrator::{
    resolve_skill_markdown_repo, scheduler_plan_from_profile, scheduler_request_defaults_from_plan,
    SchedulerConfig, SchedulerPresetKind, SchedulerProfileConfig, SchedulerRequestDefaults,
    SkillTreeNode, SkillTreeRequestPlan,
};
use rocode_provider::ProviderRegistry;
use rocode_server::{
    abort_local_session_execution, run_local_scheduler_prompt, LocalSchedulerPromptRequest,
    ServerState,
};
use rocode_session::system::{EnvironmentContext, SystemPrompt};
use rocode_tool::registry::create_default_registry;
use rocode_util::util::color::strip_ansi;
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::agent_stream_adapter::stream_prompt_to_blocks_with_cancel;
use crate::api_client::{CliApiClient, McpStatusInfo, MessageTokensInfo};
use crate::cli::RunOutputFormat;
use crate::event_stream::{self, CliServerEvent};
use crate::providers::{setup_providers, show_help};
use crate::remote::run_non_interactive_attach;
use crate::server_lifecycle::discover_or_start_server;
use crate::util::{
    append_cli_file_attachments, collect_run_input, parse_model_and_provider, truncate_text,
};

fn to_orchestrator_skill_tree(node: &SkillTreeNodeConfig) -> SkillTreeNode {
    SkillTreeNode {
        node_id: node.node_id.clone(),
        markdown_path: node.markdown_path.clone(),
        children: node
            .children
            .iter()
            .map(to_orchestrator_skill_tree)
            .collect(),
    }
}

fn resolve_request_skill_tree_plan(
    config: &Config,
    scheduler_defaults: Option<&SchedulerRequestDefaults>,
) -> Option<SkillTreeRequestPlan> {
    if let Some(plan) = scheduler_defaults.and_then(|defaults| defaults.skill_tree_plan.clone()) {
        return Some(plan);
    }

    let skill_tree = config.composition.as_ref()?.skill_tree.as_ref()?;
    if matches!(skill_tree.enabled, Some(false)) {
        return None;
    }

    let root = skill_tree.root.as_ref()?;
    let root = to_orchestrator_skill_tree(root);
    let markdown_repo = resolve_skill_markdown_repo(&config.skill_paths);

    match SkillTreeRequestPlan::from_tree_with_separator(
        &root,
        &markdown_repo,
        skill_tree.separator.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(%error, "failed to build request skill tree plan");
            None
        }
    }
}

fn resolve_requested_agent_name(
    config: &Config,
    requested_agent: Option<&str>,
    scheduler_defaults: Option<&SchedulerRequestDefaults>,
) -> String {
    if let Some(agent) = requested_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return agent.to_string();
    }

    if let Some(agent) = scheduler_defaults.and_then(|defaults| defaults.root_agent_name.clone()) {
        return agent;
    }

    if let Some(agent) = config
        .default_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return agent.to_string();
    }

    "build".to_string()
}
pub(crate) async fn run_non_interactive(
    message: Vec<String>,
    command: Option<String>,
    continue_last: bool,
    session: Option<String>,
    fork: bool,
    share: bool,
    model: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
    files: Vec<PathBuf>,
    format: RunOutputFormat,
    title: Option<String>,
    attach: Option<String>,
    dir: Option<PathBuf>,
    _port: Option<u16>,
    variant: Option<String>,
    _thinking: bool,
) -> anyhow::Result<()> {
    if let Some(dir) = dir {
        std::env::set_current_dir(&dir).map_err(|e| {
            anyhow::anyhow!("Failed to change directory to {}: {}", dir.display(), e)
        })?;
    }

    if fork && !continue_last && session.is_none() {
        anyhow::bail!("--fork requires --continue or --session");
    }

    let mut input = collect_run_input(message)?;
    append_cli_file_attachments(&mut input, &files)?;

    if let Some(base_url) = attach {
        return run_non_interactive_attach(
            base_url,
            input,
            command,
            continue_last,
            session,
            fork,
            share,
            model,
            requested_scheduler_profile,
            variant,
            format,
            title,
        )
        .await;
    }

    if continue_last || session.is_some() || fork || share {
        println!(
            "Note: session/share flags are currently applied when using `run --attach <server>`."
        );
    }

    if let Some(command_name) = command {
        let cwd = std::env::current_dir()?;
        let mut registry = CommandRegistry::new();
        let _ = registry.load_from_directory(&cwd);
        let args = if input.trim().is_empty() {
            Vec::new()
        } else {
            input
                .split_whitespace()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
        };
        let rendered =
            registry.execute(&command_name, CommandContext::new(cwd).with_arguments(args))?;
        input = rendered;
    }

    if input.trim().is_empty() {
        let (provider, model_id) = parse_model_and_provider(model);
        return run_chat_session(
            model_id,
            provider,
            requested_agent,
            requested_scheduler_profile,
            None,
            false,
        )
        .await;
    }

    let (provider, model_id) = parse_model_and_provider(model);
    run_chat_session(
        model_id,
        provider,
        requested_agent,
        requested_scheduler_profile,
        Some(input.clone()),
        true,
    )
    .await?;

    match format {
        RunOutputFormat::Default => {
            println!("{}", input);
        }
        RunOutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "message": input,
                    "format": "json",
                    "title": title,
                })
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct CliRunSelection {
    model: Option<String>,
    provider: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
}

struct CliExecutionRuntime {
    executor: AgentExecutor,
    resolved_agent_name: String,
    resolved_scheduler_profile_name: Option<String>,
    resolved_model_label: String,
    observed_topology: Arc<Mutex<CliObservedExecutionTopology>>,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    scheduler_stage_snapshots: Arc<Mutex<HashMap<String, String>>>,
    terminal_surface: Option<Arc<CliTerminalSurface>>,
    prompt_chrome: Option<Arc<CliPromptChrome>>,
    prompt_session: Option<Arc<PromptSession>>,
    prompt_session_slot: Arc<std::sync::Mutex<Option<Arc<PromptSession>>>>,
    queued_inputs: Arc<AsyncMutex<VecDeque<String>>>,
    busy_flag: Arc<AtomicBool>,
    exit_requested: Arc<AtomicBool>,
    active_abort: Arc<AsyncMutex<Option<CliActiveAbortHandle>>>,
    recovery_base_prompt: Option<String>,
    local_scheduler_state: Option<Arc<ServerState>>,
    local_scheduler_session_id: Option<String>,
    /// Shared spinner guard — updated each message cycle so that question/permission
    /// callbacks can pause the active spinner without holding a stale reference.
    spinner_guard: Arc<std::sync::Mutex<SpinnerGuard>>,
    /// HTTP client for communicating with the server (Phase 3 unification).
    api_client: Option<Arc<CliApiClient>>,
    /// Server-side session ID (created via HTTP POST /session).
    server_session_id: Option<String>,
    /// Tracks the last rendered message ID from the server for incremental rendering.
    last_rendered_message_id: Arc<std::sync::Mutex<Option<String>>>,
}

#[derive(Debug, Clone, Default)]
struct CliObservedExecutionTopology {
    active: bool,
    root_id: Option<String>,
    scheduler_id: Option<String>,
    active_stage_id: Option<String>,
    stage_order: Vec<String>,
    nodes: HashMap<String, CliObservedExecutionNode>,
}

#[derive(Debug, Clone)]
struct CliObservedExecutionNode {
    kind: String,
    label: String,
    status: String,
    waiting_on: Option<String>,
    recent_event: Option<String>,
    children: Vec<String>,
}

#[derive(Clone)]
enum CliActiveAbortHandle {
    Agent(CancellationToken),
    Scheduler {
        state: Arc<ServerState>,
        session_id: String,
    },
    /// Server-side execution — abort via HTTP POST.
    Server {
        api_client: Arc<CliApiClient>,
        session_id: String,
    },
}

#[derive(Debug)]
struct CliPromptChrome {
    mode_label: Mutex<String>,
    model_label: Mutex<String>,
    directory_label: String,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    observed_topology: Arc<Mutex<CliObservedExecutionTopology>>,
    style: CliStyle,
}

impl CliPromptChrome {
    fn new(runtime: &CliExecutionRuntime, style: &CliStyle, current_dir: &Path) -> Self {
        Self {
            mode_label: Mutex::new(cli_mode_label(runtime)),
            model_label: Mutex::new(format!("Model {}", runtime.resolved_model_label)),
            directory_label: display_path_for_cli(current_dir),
            frontend_projection: runtime.frontend_projection.clone(),
            observed_topology: runtime.observed_topology.clone(),
            style: style.clone(),
        }
    }

    fn update_from_runtime(&self, runtime: &CliExecutionRuntime) {
        if let Ok(mut mode) = self.mode_label.lock() {
            *mode = cli_mode_label(runtime);
        }
        if let Ok(mut model) = self.model_label.lock() {
            *model = format!("Model {}", runtime.resolved_model_label);
        }
    }

    fn frame(&self) -> PromptFrame {
        let mode = self
            .mode_label
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "Agent build".to_string());
        let model = self
            .model_label
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "Model auto".to_string());
        let footer = self
            .frontend_projection
            .lock()
            .map(|projection| projection.footer_text())
            .unwrap_or_else(|_| " Ready  •  Alt+Enter/Ctrl+J newline  •  /help  •  Ctrl+D exit ".to_string());
        let screen_lines = match (
            self.frontend_projection.lock(),
            self.observed_topology.lock(),
        ) {
            (Ok(projection), Ok(topology)) => cli_render_retained_layout(
                &mode,
                &model,
                &self.directory_label,
                &projection,
                &topology,
                &self.style,
            ),
            _ => Vec::new(),
        };
        PromptFrame::boxed_with_footer(&mode, &model, &footer, &self.style)
            .with_screen_lines(screen_lines)
    }
}

struct CliTerminalSurface {
    style: CliStyle,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    prompt_session: Mutex<Option<Arc<PromptSession>>>,
}

impl CliTerminalSurface {
    fn new(style: CliStyle, frontend_projection: Arc<Mutex<CliFrontendProjection>>) -> Self {
        Self {
            style,
            frontend_projection,
            prompt_session: Mutex::new(None),
        }
    }

    fn set_prompt_session(&self, prompt_session: Arc<PromptSession>) {
        if let Ok(mut slot) = self.prompt_session.lock() {
            *slot = Some(prompt_session);
        }
    }

    fn print_block(&self, block: OutputBlock) -> anyhow::Result<()> {
        self.append_rendered(&render_cli_block_rich(&block, &self.style))?;
        Ok(())
    }

    fn print_text(&self, text: &str) -> io::Result<()> {
        self.append_rendered(text)
    }

    fn print_panel(&self, title: &str, footer: Option<&str>, lines: &[String]) -> io::Result<()> {
        let panel = CliPanelFrame::boxed(title, footer, &self.style);
        self.append_rendered(&panel.render_lines(lines))
    }

    fn clear_transcript(&self) -> io::Result<()> {
        if let Ok(mut projection) = self.frontend_projection.lock() {
            projection.transcript.clear();
        }
        self.refresh_prompt()
    }

    fn append_rendered(&self, rendered: &str) -> io::Result<()> {
        if let Ok(mut projection) = self.frontend_projection.lock() {
            projection.transcript.append_rendered(rendered);
            // Auto-scroll to bottom when new content arrives
            projection.scroll_offset = 0;
        }
        self.refresh_prompt()
    }

    fn refresh_prompt(&self) -> io::Result<()> {
        if let Some(prompt) = self
            .prompt_session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().cloned())
        {
            prompt.refresh()?;
        }
        Ok(())
    }
}

enum CliDispatchInput {
    Line(String),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CliFrontendPhase {
    #[default]
    Idle,
    Busy,
    Waiting,
    Cancelling,
    Failed,
}

const CLI_TRANSCRIPT_MAX_LINES: usize = 1200;

#[derive(Debug, Clone, Default)]
struct CliRetainedTranscript {
    committed_lines: Vec<String>,
    open_line: String,
}

impl CliRetainedTranscript {
    fn append_rendered(&mut self, rendered: &str) {
        let normalized = strip_ansi(rendered).replace('\r', "");
        for chunk in normalized.split_inclusive('\n') {
            if let Some(content) = chunk.strip_suffix('\n') {
                self.open_line.push_str(content);
                self.committed_lines.push(std::mem::take(&mut self.open_line));
                self.trim_to_budget();
            } else {
                self.open_line.push_str(chunk);
            }
        }
    }

    fn clear(&mut self) {
        self.committed_lines.clear();
        self.open_line.clear();
    }

    fn viewport_lines(&self, width: usize, max_rows: usize, scroll_offset: usize) -> Vec<String> {
        let mut rows = Vec::new();
        for line in &self.committed_lines {
            extend_wrapped_lines(&mut rows, line, width);
        }
        if !self.open_line.is_empty() || rows.is_empty() {
            extend_wrapped_lines(&mut rows, &self.open_line, width);
        }
        if rows.is_empty() {
            rows.push("No messages yet. Send a prompt to start.".to_string());
        }
        if rows.len() <= max_rows {
            return rows;
        }
        // Without scroll: show the last max_rows lines (tail).
        // With scroll_offset > 0: slide the window up by scroll_offset rows.
        let tail_start = rows.len().saturating_sub(max_rows);
        let start = tail_start.saturating_sub(scroll_offset);
        let end = (start + max_rows).min(rows.len());
        rows[start..end].to_vec()
    }

    /// Total wrapped row count (for calculating max scroll offset).
    fn total_rows(&self, width: usize) -> usize {
        let mut count = 0usize;
        for line in &self.committed_lines {
            count += wrap_display_text(line, width.max(1)).len();
        }
        if !self.open_line.is_empty() {
            count += wrap_display_text(&self.open_line, width.max(1)).len();
        }
        count.max(1)
    }

    fn trim_to_budget(&mut self) {
        if self.committed_lines.len() > CLI_TRANSCRIPT_MAX_LINES {
            let overflow = self.committed_lines.len() - CLI_TRANSCRIPT_MAX_LINES;
            self.committed_lines.drain(0..overflow);
        }
    }
}

/// Cumulative token usage and cost for the current session.
#[derive(Debug, Clone, Default)]
struct CliSessionTokenStats {
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_cost: f64,
}

impl CliSessionTokenStats {
    /// Accumulate token counts from a single assistant message.
    fn accumulate(&mut self, tokens: &MessageTokensInfo, cost: f64) {
        self.input_tokens += tokens.input;
        self.output_tokens += tokens.output;
        self.reasoning_tokens += tokens.reasoning;
        self.cache_read_tokens += tokens.cache_read;
        self.cache_write_tokens += tokens.cache_write;
        self.total_tokens += tokens.input
            + tokens.output
            + tokens.reasoning
            + tokens.cache_read
            + tokens.cache_write;
        self.total_cost += cost;
    }
}

/// MCP server status snapshot for sidebar display.
#[derive(Debug, Clone)]
struct CliMcpServerStatus {
    name: String,
    status: String,
    tools: usize,
    error: Option<String>,
}

impl From<McpStatusInfo> for CliMcpServerStatus {
    fn from(info: McpStatusInfo) -> Self {
        Self {
            name: info.name,
            status: info.status,
            tools: info.tools,
            error: info.error,
        }
    }
}

#[derive(Debug, Clone)]
struct CliFrontendProjection {
    phase: CliFrontendPhase,
    active_label: Option<String>,
    queue_len: usize,
    active_stage: Option<SchedulerStageBlock>,
    transcript: CliRetainedTranscript,
    sidebar_collapsed: bool,
    active_collapsed: bool,
    session_title: Option<String>,
    /// Scroll offset for Messages panel: 0 = bottom (latest), N = scrolled up N rows.
    scroll_offset: usize,
    /// Cumulative token usage for the current session.
    token_stats: CliSessionTokenStats,
    /// MCP server statuses fetched from the server.
    mcp_servers: Vec<CliMcpServerStatus>,
    /// LSP server names fetched from the server.
    lsp_servers: Vec<String>,
}

impl Default for CliFrontendProjection {
    fn default() -> Self {
        Self {
            phase: CliFrontendPhase::default(),
            active_label: None,
            queue_len: 0,
            active_stage: None,
            transcript: CliRetainedTranscript::default(),
            sidebar_collapsed: true,
            active_collapsed: true,
            session_title: None,
            scroll_offset: 0,
            token_stats: CliSessionTokenStats::default(),
            mcp_servers: Vec::new(),
            lsp_servers: Vec::new(),
        }
    }
}

impl CliFrontendProjection {
    fn footer_text(&self) -> String {
        let state = match self.phase {
            CliFrontendPhase::Idle => "Ready".to_string(),
            CliFrontendPhase::Busy => "Busy".to_string(),
            CliFrontendPhase::Waiting => "Waiting".to_string(),
            CliFrontendPhase::Cancelling => "Cancelling".to_string(),
            CliFrontendPhase::Failed => "Error".to_string(),
        };
        let mut parts = vec![format!(" {} ", state)];
        if let Some(active) = self.active_label.as_deref().filter(|value| !value.is_empty()) {
            parts.push(active.to_string());
        }
        if self.queue_len > 0 {
            parts.push(format!("queue {}", self.queue_len));
        }
        parts.push("Alt+Enter/Ctrl+J newline".to_string());
        parts.push("/help".to_string());
        if !matches!(self.phase, CliFrontendPhase::Idle) {
            parts.push("/abort".to_string());
        }
        parts.push("Ctrl+D exit".to_string());
        format!(" {} ", parts.join("  •  "))
    }
}

impl CliObservedExecutionTopology {
    fn reset_for_run(&mut self, agent_name: &str, scheduler_profile: Option<&str>) {
        self.active = true;
        self.root_id = Some("prompt".to_string());
        self.scheduler_id = scheduler_profile.map(|_| "scheduler".to_string());
        self.active_stage_id = None;
        self.stage_order.clear();
        self.nodes.clear();
        self.nodes.insert(
            "prompt".to_string(),
            CliObservedExecutionNode {
                kind: "prompt".to_string(),
                label: format!("Prompt run ({})", agent_name),
                status: "running".to_string(),
                waiting_on: Some("model".to_string()),
                recent_event: Some("Prompt run started".to_string()),
                children: Vec::new(),
            },
        );
        if let Some(profile) = scheduler_profile {
            self.nodes.insert(
                "scheduler".to_string(),
                CliObservedExecutionNode {
                    kind: "scheduler".to_string(),
                    label: format!("Scheduler run ({})", profile),
                    status: "running".to_string(),
                    waiting_on: Some("model".to_string()),
                    recent_event: Some("Scheduler orchestration started".to_string()),
                    children: Vec::new(),
                },
            );
            self.attach_child("prompt", "scheduler");
        }
    }

    fn observe_block(&mut self, block: &OutputBlock) {
        match block {
            OutputBlock::SchedulerStage(stage) => self.observe_scheduler_stage(stage),
            OutputBlock::Tool(tool) => self.observe_tool(tool),
            _ => {}
        }
    }

    fn observe_scheduler_stage(&mut self, stage: &rocode_command::output_blocks::SchedulerStageBlock) {
        let stage_id = format!(
            "stage:{}:{}",
            stage.stage_index.unwrap_or((self.stage_order.len() + 1) as u64),
            stage.stage
        );
        let parent_id = self
            .scheduler_id
            .clone()
            .unwrap_or_else(|| self.root_id.clone().unwrap_or_else(|| "prompt".to_string()));
        let status = stage.status.clone().unwrap_or_else(|| "running".to_string());
        let node = self.nodes.entry(stage_id.clone()).or_insert(CliObservedExecutionNode {
            kind: "stage".to_string(),
            label: stage.title.clone(),
            status: status.clone(),
            waiting_on: stage.waiting_on.clone(),
            recent_event: stage.last_event.clone(),
            children: Vec::new(),
        });
        node.label = stage.title.clone();
        node.status = status.clone();
        node.waiting_on = stage.waiting_on.clone();
        node.recent_event = stage.last_event.clone();
        self.attach_child(&parent_id, &stage_id);
        if !self.stage_order.iter().any(|id| id == &stage_id) {
            self.stage_order.push(stage_id.clone());
        }
        if matches!(status.as_str(), "running" | "waiting" | "cancelling" | "retry") {
            self.active_stage_id = Some(stage_id.clone());
        }
        if let Some(scheduler_id) = self.scheduler_id.clone() {
            if let Some(scheduler) = self.nodes.get_mut(&scheduler_id) {
                scheduler.waiting_on = stage.waiting_on.clone();
                scheduler.recent_event = stage.last_event.clone();
                scheduler.status = if status == "waiting" {
                    "waiting".to_string()
                } else {
                    "running".to_string()
                };
            }
        }
    }

    fn observe_tool(&mut self, tool: &rocode_command::output_blocks::ToolBlock) {
        let parent_id = self
            .active_stage_id
            .clone()
            .or_else(|| self.scheduler_id.clone())
            .or_else(|| self.root_id.clone())
            .unwrap_or_else(|| "prompt".to_string());
        let tool_id = format!("tool:{}:{}", parent_id, tool.name);
        let status = match tool.phase {
            rocode_command::output_blocks::ToolPhase::Start
            | rocode_command::output_blocks::ToolPhase::Running => "running",
            rocode_command::output_blocks::ToolPhase::Done => "done",
            rocode_command::output_blocks::ToolPhase::Error => "error",
        }
        .to_string();
        let node = self.nodes.entry(tool_id.clone()).or_insert(CliObservedExecutionNode {
            kind: "tool".to_string(),
            label: tool.name.clone(),
            status: status.clone(),
            waiting_on: Some("tool".to_string()),
            recent_event: tool.detail.clone(),
            children: Vec::new(),
        });
        node.status = status.clone();
        node.waiting_on = if matches!(tool.phase, rocode_command::output_blocks::ToolPhase::Done) {
            None
        } else {
            Some("tool".to_string())
        };
        node.recent_event = tool.detail.clone();
        self.attach_child(&parent_id, &tool_id);
    }

    fn start_question(&mut self, count: usize) {
        let parent_id = self
            .active_stage_id
            .clone()
            .or_else(|| self.scheduler_id.clone())
            .or_else(|| self.root_id.clone())
            .unwrap_or_else(|| "prompt".to_string());
        let question_id = format!("question:{}:{}", parent_id, count);
        self.nodes.insert(
            question_id.clone(),
            CliObservedExecutionNode {
                kind: "question".to_string(),
                label: format!("Question ({})", count),
                status: "waiting".to_string(),
                waiting_on: Some("user".to_string()),
                recent_event: Some("Waiting for user answer".to_string()),
                children: Vec::new(),
            },
        );
        self.attach_child(&parent_id, &question_id);
    }

    fn finish_question(&mut self, outcome: &str) {
        for node in self.nodes.values_mut().filter(|node| node.kind == "question") {
            if node.status == "waiting" {
                node.status = outcome.to_string();
                node.waiting_on = None;
                node.recent_event = Some(format!("Question {}", outcome));
            }
        }
    }

    fn finish_run(&mut self, outcome: Option<String>) {
        self.active = false;
        if let Some(root_id) = self.root_id.clone() {
            if let Some(root) = self.nodes.get_mut(&root_id) {
                root.status = outcome
                    .clone()
                    .unwrap_or_else(|| "completed".to_string())
                    .to_lowercase();
                root.waiting_on = None;
                root.recent_event = outcome;
            }
        }
    }

    fn attach_child(&mut self, parent_id: &str, child_id: &str) {
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            if !parent.children.iter().any(|id| id == child_id) {
                parent.children.push(child_id.to_string());
            }
        }
    }
}

fn cli_print_execution_topology(
    observed_topology: &Arc<Mutex<CliObservedExecutionTopology>>,
    style: &CliStyle,
) {
    println!();
    println!(
        "  {} {}",
        style.bold_cyan(style.bullet()),
        style.bold("Execution Topology")
    );
    let Ok(topology) = observed_topology.lock() else {
        println!("    unavailable");
        println!();
        return;
    };
    if topology.nodes.is_empty() {
        println!("    no observed execution topology");
        println!();
        return;
    }
    if topology.active {
        println!("    active");
    } else {
        println!("    idle · last observed topology");
    }
    if let Some(root_id) = topology.root_id.as_deref() {
        cli_print_execution_node(&topology, root_id, "    ", true);
    }
    println!();
}

fn cli_print_execution_node(
    topology: &CliObservedExecutionTopology,
    node_id: &str,
    prefix: &str,
    is_last: bool,
) {
    let Some(node) = topology.nodes.get(node_id) else {
        return;
    };
    let branch = if prefix.trim().is_empty() {
        ""
    } else if is_last {
        "└─ "
    } else {
        "├─ "
    };
    let mut line = format!("{}{}{} · {}", prefix, branch, node.label, node.status);
    if let Some(waiting_on) = node.waiting_on.as_deref() {
        line.push_str(&format!(" · waiting {}", waiting_on));
    }
    if let Some(recent_event) = node.recent_event.as_deref() {
        line.push_str(&format!(" · {}", recent_event));
    }
    println!("{}", line);
    let child_prefix = if prefix.trim().is_empty() {
        "      ".to_string()
    } else if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };
    for (index, child_id) in node.children.iter().enumerate() {
        cli_print_execution_node(
            topology,
            child_id,
            &child_prefix,
            index + 1 == node.children.len(),
        );
    }
}

#[derive(Debug, Clone, Default)]
struct CliSchedulerResolution {
    defaults: Option<SchedulerRequestDefaults>,
    profile_model: Option<(String, String)>,
}

fn resolve_scheduler_profile_config(
    config: &Config,
    requested_scheduler_profile: Option<&str>,
) -> Option<(String, SchedulerProfileConfig)> {
    let requested = requested_scheduler_profile
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let scheduler_path = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(path) = scheduler_path {
        let scheduler_config = match SchedulerConfig::load_from_file(path) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(path = %path, %error, "failed to load scheduler config");
                return requested.and_then(|name| {
                    SchedulerPresetKind::from_str(name).ok().map(|_| {
                        (
                            name.to_string(),
                            SchedulerProfileConfig {
                                orchestrator: Some(name.to_string()),
                                ..Default::default()
                            },
                        )
                    })
                });
            }
        };

        if let Some(name) = requested {
            if let Ok(profile) = scheduler_config.profile(name) {
                return Some((name.to_string(), profile.clone()));
            }
            return SchedulerPresetKind::from_str(name).ok().map(|_| {
                (
                    name.to_string(),
                    SchedulerProfileConfig {
                        orchestrator: Some(name.to_string()),
                        ..Default::default()
                    },
                )
            });
        }

        if let Some(name) = scheduler_config.default_profile_key() {
            if let Ok(profile) = scheduler_config.profile(name) {
                return Some((name.to_string(), profile.clone()));
            }
        }
        return None;
    }

    requested.and_then(|name| {
        SchedulerPresetKind::from_str(name).ok().map(|_| {
            (
                name.to_string(),
                SchedulerProfileConfig {
                    orchestrator: Some(name.to_string()),
                    ..Default::default()
                },
            )
        })
    })
}

fn resolve_scheduler_runtime(
    config: &Config,
    requested_scheduler_profile: Option<&str>,
) -> CliSchedulerResolution {
    let Some((profile_name, profile)) =
        resolve_scheduler_profile_config(config, requested_scheduler_profile)
    else {
        return CliSchedulerResolution::default();
    };

    let defaults = scheduler_plan_from_profile(Some(profile_name.clone()), &profile)
        .ok()
        .map(|plan| scheduler_request_defaults_from_plan(&plan));
    let profile_model = profile
        .model
        .as_ref()
        .map(|model| (model.provider_id.clone(), model.model_id.clone()));

    CliSchedulerResolution {
        defaults,
        profile_model,
    }
}

fn apply_system_prompt_to_conversation(
    conversation: Option<Conversation>,
    system_prompt: &str,
) -> Conversation {
    let mut conversation = conversation.unwrap_or_else(Conversation::new);
    if let Some(first) = conversation.messages.first_mut() {
        if matches!(first.role, MessageRole::System) {
            first.content = system_prompt.to_string();
            return conversation;
        }
    }
    conversation
        .messages
        .insert(0, AgentMessage::system(system_prompt.to_string()));
    conversation
}

fn compose_executor_system_prompt(agent_info: &AgentInfo, current_dir: &Path) -> String {
    let (model_api_id, provider_id) = match &agent_info.model {
        Some(m) => (m.model_id.clone(), m.provider_id.clone()),
        None => (
            "claude-sonnet-4-20250514".to_string(),
            "anthropic".to_string(),
        ),
    };
    let mut sections = Vec::new();
    if let Some(agent_prompt) = agent_info.resolved_system_prompt() {
        if !agent_prompt.trim().is_empty() {
            sections.push(agent_prompt);
        }
    }
    sections.push(SystemPrompt::for_model(&model_api_id).to_string());
    let env_ctx = EnvironmentContext::from_current(
        &model_api_id,
        &provider_id,
        current_dir.to_string_lossy().as_ref(),
    );
    sections.push(SystemPrompt::environment(&env_ctx));
    sections.join("\n\n")
}

fn build_local_cli_server_state(
    config: &Config,
    provider_registry: &ProviderRegistry,
    tool_registry: Arc<rocode_tool::registry::ToolRegistry>,
) -> Arc<ServerState> {
    let mut state = ServerState::new();

    let mut providers = rocode_provider::ProviderRegistry::new();
    for provider in provider_registry.list() {
        providers.register_arc(provider);
    }

    state.providers = tokio::sync::RwLock::new(providers);
    state.config_store = Arc::new(rocode_config::ConfigStore::new(config.clone()));
    state.tool_registry = tool_registry;
    state.prompt_runner = Arc::new(
        rocode_session::SessionPrompt::new(Arc::new(tokio::sync::RwLock::new(
            rocode_session::SessionStateManager::new(),
        )))
        .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(config)),
    );
    state.category_registry = Arc::new(rocode_config::CategoryRegistry::with_builtins());

    Arc::new(state)
}

async fn build_cli_execution_runtime(
    config: &Config,
    current_dir: &Path,
    provider_registry: Arc<rocode_provider::ProviderRegistry>,
    tool_registry: Arc<rocode_tool::registry::ToolRegistry>,
    agent_registry: Arc<AgentRegistry>,
    selection: &CliRunSelection,
    prior_conversation: Option<Conversation>,
    prior_subsessions: Option<HashMap<String, PersistedSubsessionState>>,
) -> anyhow::Result<CliExecutionRuntime> {
    let observed_topology = Arc::new(Mutex::new(CliObservedExecutionTopology::default()));
    let frontend_projection = Arc::new(Mutex::new(CliFrontendProjection::default()));
    let scheduler_stage_snapshots = Arc::new(Mutex::new(HashMap::new()));
    let scheduler_resolution =
        resolve_scheduler_runtime(config, selection.requested_scheduler_profile.as_deref());
    let scheduler_defaults = scheduler_resolution.defaults.clone();
    let scheduler_profile_name = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.profile_name.clone());
    let scheduler_root_agent = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.root_agent_name.clone());
    let request_skill_tree_plan =
        resolve_request_skill_tree_plan(config, scheduler_defaults.as_ref());
    let agent_name = resolve_requested_agent_name(
        config,
        selection.requested_agent.as_deref(),
        scheduler_defaults.as_ref(),
    );

    let mut agent_info = agent_registry
        .get(&agent_name)
        .cloned()
        .unwrap_or_else(AgentInfo::build);

    if let Some(ref model_id) = selection.model {
        let provider_id = selection.provider.clone().unwrap_or_else(|| {
            if model_id.starts_with("claude") {
                "anthropic".to_string()
            } else {
                "openai".to_string()
            }
        });
        agent_info = agent_info.with_model(model_id.clone(), provider_id);
    } else if let Some((provider_id, model_id)) = scheduler_resolution.profile_model.clone() {
        agent_info = agent_info.with_model(model_id, provider_id);
    }

    let resolved_model_label = agent_info
        .model
        .as_ref()
        .map(|m| format!("{}/{}", m.provider_id, m.model_id))
        .unwrap_or_else(|| "auto".to_string());

    let local_scheduler_state = scheduler_profile_name.as_ref().map(|_| {
        build_local_cli_server_state(config, provider_registry.as_ref(), tool_registry.clone())
    });
    let local_scheduler_session_id = if let Some(state) = local_scheduler_state.as_ref() {
        let mut sessions = state.sessions.lock().await;
        Some(
            sessions
                .create("rocode-cli", current_dir.to_string_lossy())
                .id,
        )
    } else {
        None
    };

    // Shared spinner guard slot — closures capture this; process_message_with_mode
    // swaps in the real spinner's guard each cycle.
    let spinner_guard: Arc<std::sync::Mutex<SpinnerGuard>> =
        Arc::new(std::sync::Mutex::new(SpinnerGuard::noop()));
    let prompt_session_slot: Arc<std::sync::Mutex<Option<Arc<PromptSession>>>> =
        Arc::new(std::sync::Mutex::new(None));

    let mut executor = AgentExecutor::new(
        agent_info.clone(),
        provider_registry,
        tool_registry,
        agent_registry.clone(),
    )
    .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(config))
    .with_ask_question({
        let observed_topology = observed_topology.clone();
        let frontend_projection = frontend_projection.clone();
        let spinner_guard = spinner_guard.clone();
        let prompt_session_slot = prompt_session_slot.clone();
        move |questions| {
            let observed_topology = observed_topology.clone();
            let frontend_projection = frontend_projection.clone();
            let guard = spinner_guard.lock().map(|g| g.clone()).unwrap_or_else(|_| SpinnerGuard::noop());
            let prompt_session_slot = prompt_session_slot.clone();
            async move {
                cli_ask_question(
                    questions,
                    observed_topology,
                    frontend_projection,
                    prompt_session_slot,
                    guard,
                )
                .await
            }
        }
    })
    .with_ask_permission(build_cli_permission_callback(spinner_guard.clone()));

    if let Some(states) = prior_subsessions {
        executor = executor.with_persisted_subsessions(states);
    }

    let full_prompt = compose_executor_system_prompt(&agent_info, current_dir);
    let conversation = apply_system_prompt_to_conversation(prior_conversation, &full_prompt);
    *executor.conversation_mut() = conversation;

    if let Some(plan) = request_skill_tree_plan {
        executor = executor.with_request_skill_tree_plan(plan);
    }

    tracing::info!(
        requested_agent = ?selection.requested_agent,
        requested_scheduler_profile = ?selection.requested_scheduler_profile,
        resolved_agent = %agent_name,
        scheduler_profile = ?scheduler_profile_name,
        scheduler_root_agent = ?scheduler_root_agent,
        resolved_model = %resolved_model_label,
        "resolved cli runtime execution configuration"
    );

    Ok(CliExecutionRuntime {
        executor,
        resolved_agent_name: agent_name,
        resolved_scheduler_profile_name: scheduler_profile_name,
        resolved_model_label,
        observed_topology,
        frontend_projection,
        scheduler_stage_snapshots,
        terminal_surface: None,
        prompt_chrome: None,
        prompt_session: None,
        prompt_session_slot,
        queued_inputs: Arc::new(AsyncMutex::new(VecDeque::new())),
        busy_flag: Arc::new(AtomicBool::new(false)),
        exit_requested: Arc::new(AtomicBool::new(false)),
        active_abort: Arc::new(AsyncMutex::new(None)),
        recovery_base_prompt: None,
        local_scheduler_state,
        local_scheduler_session_id,
        spinner_guard,
        api_client: None,
        server_session_id: None,
        last_rendered_message_id: Arc::new(std::sync::Mutex::new(None)),
    })
}

fn cli_available_presets(config: &Config) -> Vec<String> {
    let mut names = BTreeSet::new();
    for preset in SchedulerPresetKind::public_presets() {
        names.insert(preset.as_str().to_string());
    }

    if let Some(path) = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(scheduler_config) = SchedulerConfig::load_from_file(path) {
            for name in scheduler_config.profiles.keys() {
                names.insert(name.clone());
            }
        }
    }

    names.into_iter().collect()
}

fn cli_list_presets(config: &Config, active_profile: Option<&str>) {
    let style = CliStyle::detect();
    let lines = cli_available_presets(config)
        .into_iter()
        .map(|preset| {
            let active = if active_profile == Some(preset.as_str()) {
                " ← active".to_string()
            } else {
                String::new()
            };
            format!("{preset}{active}")
        })
        .collect::<Vec<_>>();
    let rendered = render_cli_list("Available Presets", None, &lines, &style);
    print!("{}", rendered);
    let _ = io::stdout().flush();
}

fn cli_has_preset(config: &Config, name: &str) -> bool {
    cli_available_presets(config)
        .iter()
        .any(|preset| preset.eq_ignore_ascii_case(name))
}

fn cli_switch_message(runtime: Option<&CliExecutionRuntime>, kind: &str, value: &str) {
    let style = CliStyle::detect();
    let _ = print_block(
        runtime,
        OutputBlock::Status(StatusBlock::title(format!("Switched {} to {}.", kind, value))),
        &style,
    );
}

#[allow(dead_code)]
fn print_cli_panel(
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    style: &CliStyle,
) -> io::Result<()> {
    let panel = CliPanelFrame::boxed(title, footer, style);
    print!("{}", panel.render_lines(lines));
    io::stdout().flush()
}

#[allow(dead_code)]
fn print_cli_panel_on_surface(
    runtime: Option<&CliExecutionRuntime>,
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    style: &CliStyle,
) -> io::Result<()> {
    if let Some(surface) = runtime.and_then(|runtime| runtime.terminal_surface.as_ref()) {
        surface.print_panel(title, footer, lines)
    } else {
        print_cli_panel(title, footer, lines, style)
    }
}

/// Render a list as plain text (title + indented items) — no box frame.
/// Designed for use inside the Messages transcript where an outer box already exists.
fn render_cli_list(title: &str, footer: Option<&str>, lines: &[String], style: &CliStyle) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n  {} {}\n",
        style.bold_cyan(style.bullet()),
        style.bold(title),
    ));
    if lines.is_empty() {
        out.push_str(&format!("    {}\n", style.dim("(none)")));
    } else {
        for line in lines {
            out.push_str(&format!("    {}\n", line));
        }
    }
    if let Some(footer) = footer {
        out.push_str(&format!("    {}\n", style.dim(footer)));
    }
    out.push('\n');
    out
}

fn print_cli_list_on_surface(
    runtime: Option<&CliExecutionRuntime>,
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    style: &CliStyle,
) -> io::Result<()> {
    let rendered = render_cli_list(title, footer, lines, style);
    if let Some(surface) = runtime.and_then(|runtime| runtime.terminal_surface.as_ref()) {
        surface.print_text(&rendered)
    } else {
        print!("{}", rendered);
        io::stdout().flush()
    }
}

fn cli_mode_label(runtime: &CliExecutionRuntime) -> String {
    match runtime.resolved_scheduler_profile_name.as_deref() {
        Some(profile) => format!("Preset {}", profile),
        None => format!("Agent {}", runtime.resolved_agent_name),
    }
}

fn cli_refresh_prompt(runtime: &CliExecutionRuntime) {
    if let Some(prompt_session) = runtime.prompt_session.as_ref() {
        let _ = prompt_session.refresh();
    }
}

fn cli_is_terminal_stage_status(status: Option<&str>) -> bool {
    matches!(status, Some("done" | "blocked" | "cancelled"))
}

fn cli_active_stage_context_lines(
    stage: Option<&SchedulerStageBlock>,
    style: &CliStyle,
) -> Vec<String> {
    let Some(stage) = stage else {
        return Vec::new();
    };

    let max_width = usize::from(style.width).saturating_sub(8).clamp(24, 96);
    let header = if let (Some(index), Some(total)) = (stage.stage_index, stage.stage_total) {
        format!("Stage: {} [{}/{}]", stage.title, index, total)
    } else {
        format!("Stage: {}", stage.title)
    };

    let mut summary = Vec::new();
    if let Some(step) = stage.step {
        summary.push(format!("step {step}"));
    }
    if let Some(status) = stage.status.as_deref().filter(|value| !value.is_empty()) {
        summary.push(status.to_string());
    }
    if let Some(waiting_on) = stage.waiting_on.as_deref().filter(|value| !value.is_empty()) {
        summary.push(format!("waiting on {waiting_on}"));
    }
    summary.push(format!(
        "tokens {}/{}",
        stage
            .prompt_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".to_string()),
        stage
            .completion_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".to_string())
    ));

    let mut lines = vec![
        truncate_display(&header, max_width),
        truncate_display(&format!("Status: {}", summary.join(" · ")), max_width),
    ];
    if let Some(focus) = stage.focus.as_deref().filter(|value| !value.is_empty()) {
        lines.push(truncate_display(&format!("Focus: {focus}"), max_width));
    }
    if let Some(last_event) = stage.last_event.as_deref().filter(|value| !value.is_empty()) {
        lines.push(truncate_display(&format!("Last: {last_event}"), max_width));
    }
    lines
}

fn cli_attach_interactive_handles(
    runtime: &mut CliExecutionRuntime,
    terminal_surface: Arc<CliTerminalSurface>,
    prompt_chrome: Arc<CliPromptChrome>,
    prompt_session: Arc<PromptSession>,
    queued_inputs: Arc<AsyncMutex<VecDeque<String>>>,
    busy_flag: Arc<AtomicBool>,
    exit_requested: Arc<AtomicBool>,
    active_abort: Arc<AsyncMutex<Option<CliActiveAbortHandle>>>,
) {
    runtime.terminal_surface = Some(terminal_surface);
    runtime.prompt_chrome = Some(prompt_chrome.clone());
    runtime.prompt_session = Some(prompt_session.clone());
    if let Ok(mut slot) = runtime.prompt_session_slot.lock() {
        *slot = Some(prompt_session.clone());
    }
    runtime.queued_inputs = queued_inputs;
    runtime.busy_flag = busy_flag;
    runtime.exit_requested = exit_requested;
    runtime.active_abort = active_abort;
    prompt_chrome.update_from_runtime(runtime);
    cli_refresh_prompt(runtime);
}

async fn cli_trigger_abort(handle: CliActiveAbortHandle) -> bool {
    match handle {
        CliActiveAbortHandle::Agent(token) => {
            token.cancel();
            true
        }
        CliActiveAbortHandle::Scheduler { state, session_id } => abort_local_session_execution(
            state,
            &session_id,
            true,
        )
        .await
        .get("aborted")
        .and_then(|value| value.as_bool())
        .unwrap_or(false),
        CliActiveAbortHandle::Server { api_client, session_id } => {
            match api_client.abort_session(&session_id).await {
                Ok(result) => result
                    .get("aborted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                Err(e) => {
                    tracing::error!("Failed to abort server session: {}", e);
                    false
                }
            }
        }
    }
}

fn cli_frontend_set_phase(
    frontend_projection: &Arc<Mutex<CliFrontendProjection>>,
    phase: CliFrontendPhase,
    active_label: Option<String>,
) {
    if let Ok(mut projection) = frontend_projection.lock() {
        projection.phase = phase;
        if active_label.is_some() {
            projection.active_label = active_label;
        }
    }
}

fn cli_frontend_clear(runtime: &CliExecutionRuntime) {
    if let Ok(mut projection) = runtime.frontend_projection.lock() {
        projection.phase = CliFrontendPhase::Idle;
        projection.active_label = None;
        projection.active_stage = None;
    }
}

fn cli_frontend_observe_block(
    frontend_projection: &Arc<Mutex<CliFrontendProjection>>,
    block: &OutputBlock,
) {
    let Ok(mut projection) = frontend_projection.lock() else {
        return;
    };
    match block {
        OutputBlock::SchedulerStage(stage) => {
            projection.phase = match stage.status.as_deref() {
                Some("waiting") | Some("blocked") => CliFrontendPhase::Waiting,
                Some("cancelling") => CliFrontendPhase::Cancelling,
                Some("cancelled") | Some("done") => projection.phase,
                _ => CliFrontendPhase::Busy,
            };
            projection.active_label = Some(cli_stage_activity_label(stage));
        }
        OutputBlock::Tool(tool) => {
            projection.phase = CliFrontendPhase::Busy;
            projection.active_label = Some(format!("tool {}", tool.name));
        }
        OutputBlock::SessionEvent(event) if event.event == "question" => {
            projection.phase = CliFrontendPhase::Waiting;
            projection.active_label = Some("question".to_string());
        }
        OutputBlock::Message(message)
            if message.role == OutputMessageRole::Assistant
                && matches!(message.phase, MessagePhase::Start | MessagePhase::Delta) =>
        {
            projection.phase = CliFrontendPhase::Busy;
            projection.active_label = Some("assistant response".to_string());
        }
        _ => {}
    }
}

fn cli_stage_activity_label(stage: &SchedulerStageBlock) -> String {
    let mut parts = Vec::new();
    if let (Some(index), Some(total)) = (stage.stage_index, stage.stage_total) {
        parts.push(format!("stage {index}/{total}"));
    } else {
        parts.push("stage".to_string());
    }
    parts.push(stage.stage.clone());
    if let Some(step) = stage.step {
        parts.push(format!("step {step}"));
    }
    parts.join(" · ")
}

fn cli_scheduler_stage_snapshot_key(stage: &SchedulerStageBlock) -> String {
    let decision_title = stage
        .decision
        .as_ref()
        .map(|decision| decision.title.clone())
        .unwrap_or_default();
    format!(
        "{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}",
        stage.stage_index.unwrap_or_default(),
        stage.stage,
        stage.status,
        stage.step,
        stage.waiting_on,
        stage.last_event,
        stage.prompt_tokens,
        stage.completion_tokens,
        decision_title,
        stage.activity.as_deref().unwrap_or_default()
    )
}

fn cli_should_emit_scheduler_stage_block(
    snapshots: &Arc<Mutex<HashMap<String, String>>>,
    stage: &SchedulerStageBlock,
) -> bool {
    let stage_id = format!(
        "{}:{}",
        stage.stage_index.unwrap_or_default(),
        stage.stage.as_str()
    );
    let snapshot = cli_scheduler_stage_snapshot_key(stage);
    let Ok(mut cache) = snapshots.lock() else {
        return true;
    };
    match cache.get(&stage_id) {
        Some(existing) if existing == &snapshot => false,
        _ => {
            cache.insert(stage_id, snapshot);
            true
        }
    }
}

fn display_path_for_cli(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            let suffix = stripped.display().to_string();
            return if suffix.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", suffix)
            };
        }
    }
    path.display().to_string()
}

fn extend_wrapped_lines(out: &mut Vec<String>, text: &str, width: usize) {
    if text.is_empty() {
        out.push(String::new());
        return;
    }
    let wrapped = wrap_display_text(text, width.max(1));
    if wrapped.is_empty() {
        out.push(String::new());
    } else {
        out.extend(wrapped);
    }
}

fn cli_fit_lines(lines: &[String], width: usize, rows: usize, tail: bool) -> Vec<String> {
    let mut wrapped = Vec::new();
    for line in lines {
        extend_wrapped_lines(&mut wrapped, line, width);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    if wrapped.len() > rows {
        if tail {
            wrapped.split_off(wrapped.len().saturating_sub(rows))
        } else {
            wrapped.truncate(rows);
            wrapped
        }
    } else {
        wrapped.resize(rows, String::new());
        wrapped
    }
}

fn cli_box_line(text: &str, inner_width: usize, style: &CliStyle) -> String {
    let content = pad_right_display(text, inner_width, ' ');
    if style.color {
        format!("{} {} {}", style.cyan("│"), content, style.cyan("│"))
    } else {
        format!("│ {} │", content)
    }
}

fn cli_render_box(
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    outer_width: usize,
    style: &CliStyle,
) -> Vec<String> {
    let inner_width = outer_width.saturating_sub(4).max(1);
    let chrome_width = inner_width + 2;
    let header_content = pad_right_display(
        &truncate_display(&format!(" {} ", title.trim()), chrome_width),
        chrome_width,
        '─',
    );
    let header = if style.color {
        format!(
            "{}{}{}",
            style.cyan("╭"),
            style.bold_cyan(&header_content),
            style.cyan("╮")
        )
    } else {
        format!("╭{}╮", header_content)
    };

    let footer_text = footer.unwrap_or("");
    let footer_content = if footer_text.is_empty() {
        "─".repeat(chrome_width)
    } else {
        pad_right_display(
            &truncate_display(&format!(" {} ", footer_text.trim()), chrome_width),
            chrome_width,
            '─',
        )
    };
    let footer = if style.color {
        format!("{}{}{}", style.cyan("╰"), style.dim(&footer_content), style.cyan("╯"))
    } else {
        format!("╰{}╯", footer_content)
    };

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(header);
    rendered.extend(lines.iter().map(|line| cli_box_line(line, inner_width, style)));
    rendered.push(footer);
    rendered
}

fn cli_join_columns(
    left: &[String],
    left_width: usize,
    right: &[String],
    right_width: usize,
    gap: usize,
) -> Vec<String> {
    let blank_left = " ".repeat(left_width);
    let blank_right = " ".repeat(right_width);
    let height = left.len().max(right.len());
    let mut rows = Vec::with_capacity(height);
    for index in 0..height {
        let left_line = left.get(index).map(String::as_str).unwrap_or(&blank_left);
        let right_line = right.get(index).map(String::as_str).unwrap_or(&blank_right);
        rows.push(format!("{}{}{}", left_line, " ".repeat(gap), right_line));
    }
    rows
}

fn cli_terminal_rows() -> usize {
    crossterm::terminal::size()
        .map(|(_, rows)| usize::from(rows))
        .unwrap_or(28)
}

fn cli_sidebar_lines(
    projection: &CliFrontendProjection,
    topology: &CliObservedExecutionTopology,
) -> Vec<String> {
    let phase = match projection.phase {
        CliFrontendPhase::Idle => "idle",
        CliFrontendPhase::Busy => "busy",
        CliFrontendPhase::Waiting => "waiting",
        CliFrontendPhase::Cancelling => "cancelling",
        CliFrontendPhase::Failed => "error",
    };
    let mut lines = vec![
        format!("Phase: {}", phase),
        format!(
            "Queue: {}",
            if projection.queue_len == 0 {
                "empty".to_string()
            } else {
                projection.queue_len.to_string()
            }
        ),
    ];
    if let Some(active) = projection
        .active_label
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Activity: {active}"));
    }
    if topology.active {
        lines.push("Execution: active".to_string());
    } else {
        lines.push("Execution: idle".to_string());
    }
    if let Some(active_stage_id) = topology.active_stage_id.as_deref() {
        if let Some(node) = topology.nodes.get(active_stage_id) {
            lines.push(format!("Node: {}", node.label));
            lines.push(format!("Status: {}", node.status));
            if let Some(waiting_on) = node.waiting_on.as_deref() {
                lines.push(format!("Waiting: {waiting_on}"));
            }
            if let Some(recent_event) = node.recent_event.as_deref() {
                lines.push(format!("Last: {recent_event}"));
            }
        }
    }

    // ── Context (token usage + cost) ────────────────────────────
    let ts = &projection.token_stats;
    if ts.total_tokens > 0 {
        lines.push(String::new());
        lines.push("─ Context ─".to_string());
        lines.push(format!("Tokens: {}", format_token_count(ts.total_tokens)));
        lines.push(format!("Cost:   ${:.4}", ts.total_cost));
    }

    // ── MCP servers ─────────────────────────────────────────────
    if !projection.mcp_servers.is_empty() {
        let connected = projection.mcp_servers.iter()
            .filter(|s| s.status == "connected")
            .count();
        let errored = projection.mcp_servers.iter()
            .filter(|s| s.status == "failed" || s.status == "error")
            .count();
        lines.push(String::new());
        lines.push(format!("─ MCP ({} active, {} err) ─", connected, errored));
        for server in &projection.mcp_servers {
            let indicator = match server.status.as_str() {
                "connected" => "●",
                "failed" | "error" => "✗",
                "needs_auth" | "needs auth" => "?",
                _ => "○",
            };
            lines.push(format!("{} {} [{}]", indicator, server.name, server.status));
            if let Some(ref err) = server.error {
                lines.push(format!("  ↳ {}", err));
            }
        }
    }

    // ── LSP servers ─────────────────────────────────────────────
    if !projection.lsp_servers.is_empty() {
        lines.push(String::new());
        lines.push(format!("─ LSP ({}) ─", projection.lsp_servers.len()));
        for server in &projection.lsp_servers {
            lines.push(format!("● {}", server));
        }
    }

    lines.push(String::new());
    lines.push("/help · /model · /preset".to_string());
    lines.push("/abort · /sidebar · /active".to_string());
    lines.push("/status · /compact · /new".to_string());
    lines
}

/// Format a token count for display (e.g., 1234 → "1,234", 1234567 → "1.2M").
fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn cli_active_stage_panel_lines(stage: Option<&SchedulerStageBlock>, style: &CliStyle) -> Vec<String> {
    let Some(stage) = stage else {
        return vec![
            "No active stage. Running work will appear here in-place.".to_string(),
            "Transcript stays on the left; live execution stays here.".to_string(),
            String::new(),
            "Queued prompts remain editable in the input box below.".to_string(),
            "Use /abort to stop the active execution boundary.".to_string(),
        ];
    };

    let mut lines = cli_active_stage_context_lines(Some(stage), style);
    if let Some(activity) = stage.activity.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("Activity: {}", activity.replace('\n', " · ")));
    }
    let mut available = Vec::new();
    if let Some(count) = stage.available_skill_count {
        available.push(format!("skills {}", count));
    }
    if let Some(count) = stage.available_agent_count {
        available.push(format!("agents {}", count));
    }
    if let Some(count) = stage.available_category_count {
        available.push(format!("categories {}", count));
    }
    if !available.is_empty() {
        lines.push(format!("Available: {}", available.join(" · ")));
    }
    if !stage.active_skills.is_empty() {
        lines.push(format!("Active skills: {}", stage.active_skills.join(", ")));
    }
    lines
}

fn cli_messages_footer(
    transcript: &CliRetainedTranscript,
    width: usize,
    max_rows: usize,
    scroll_offset: usize,
) -> String {
    let total = transcript.total_rows(width);
    if total <= max_rows {
        return "retained transcript".to_string();
    }
    if scroll_offset == 0 {
        format!("↑ /up to scroll · {} lines total", total)
    } else {
        let max_offset = total.saturating_sub(max_rows);
        let clamped = scroll_offset.min(max_offset);
        let position = max_offset.saturating_sub(clamped);
        format!(
            "line {}/{} · /up /down /bottom",
            position + 1,
            total,
        )
    }
}

fn cli_render_retained_layout(
    mode: &str,
    model: &str,
    directory: &str,
    projection: &CliFrontendProjection,
    topology: &CliObservedExecutionTopology,
    style: &CliStyle,
) -> Vec<String> {
    let total_width = usize::from(style.width.saturating_sub(1)).clamp(60, 160);
    let terminal_rows = cli_terminal_rows().max(20);
    let gap = 1usize;

    // Session header — compact single-line with session title
    let session_title = projection
        .session_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(untitled)");
    let header_lines = vec![
        format!(
            "> {} · {} · {} · {}",
            truncate_display(session_title, 32),
            mode,
            model,
            truncate_display(directory, 24),
        ),
    ];
    let header_box = cli_render_box("ROCode", None, &header_lines, total_width, style);

    // Calculate body rows budget — give back rows from collapsed panels
    let active_rows_budget = if projection.active_collapsed { 0 } else { 5usize };
    let sidebar_overhead = if projection.sidebar_collapsed { 3 } else { 0 }; // collapsed sidebar = fewer columns, more transcript width
    let body_rows = terminal_rows
        .saturating_sub(14 + active_rows_budget)
        .max(6)
        + sidebar_overhead;

    let mut screen = Vec::new();
    screen.extend(header_box);

    if projection.sidebar_collapsed {
        // Full-width Messages only, no sidebar column
        let messages_inner = total_width.saturating_sub(4).max(1);
        let transcript_lines = projection.transcript.viewport_lines(messages_inner, body_rows, projection.scroll_offset);
        let messages_footer = cli_messages_footer(&projection.transcript, messages_inner, body_rows, projection.scroll_offset);
        let messages_box =
            cli_render_box("Messages", Some(&messages_footer), &transcript_lines, total_width, style);
        screen.extend(messages_box);
    } else {
        let right_width = (if total_width >= 128 { 38 } else { 32 })
            .min(total_width.saturating_sub(29 + gap))
            .max(24);
        let left_width = total_width.saturating_sub(right_width + gap);
        let left_inner = left_width.saturating_sub(4).max(1);
        let right_inner = right_width.saturating_sub(4).max(1);
        let transcript_lines = projection.transcript.viewport_lines(left_inner, body_rows, projection.scroll_offset);
        let messages_footer = cli_messages_footer(&projection.transcript, left_inner, body_rows, projection.scroll_offset);
        let sidebar_lines = cli_fit_lines(&cli_sidebar_lines(projection, topology), right_inner, body_rows, false);
        let messages_box =
            cli_render_box("Messages", Some(&messages_footer), &transcript_lines, left_width, style);
        let sidebar_box = cli_render_box("Sidebar", None, &sidebar_lines, right_width, style);
        let body = cli_join_columns(&messages_box, left_width, &sidebar_box, right_width, gap);
        screen.extend(body);
    }

    if projection.active_collapsed {
        // Single collapsed bar
        let collapsed_label = if let Some(stage) = projection.active_stage.as_ref() {
            format!(
                "▸ {} (collapsed — /active to expand)",
                truncate_display(&stage.title, total_width.saturating_sub(48).max(12)),
            )
        } else {
            "▸ No active stage (/active to expand)".to_string()
        };
        let active_box = cli_render_box("Active", None, &[collapsed_label], total_width, style);
        screen.extend(active_box);
    } else {
        let active_rows = 5usize;
        let active_lines = cli_fit_lines(
            &cli_active_stage_panel_lines(projection.active_stage.as_ref(), style),
            total_width.saturating_sub(4).max(1),
            active_rows,
            false,
        );
        let active_box = cli_render_box("Active", None, &active_lines, total_width, style);
        screen.extend(active_box);
    }

    screen
}

async fn run_chat_session(
    model: Option<String>,
    provider: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
    initial_prompt: Option<String>,
    single_shot: bool,
) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let config = load_config(&current_dir)?;
    let provider_registry = Arc::new(setup_providers(&config).await?);

    if provider_registry.list().is_empty() {
        eprintln!("Error: No API keys configured.");
        println!("Set one of the following environment variables:");
        eprintln!("  - ANTHROPIC_API_KEY");
        eprintln!("  - OPENAI_API_KEY");
        eprintln!("  - OPENROUTER_API_KEY");
        eprintln!("  - GOOGLE_API_KEY");
        eprintln!("  - MISTRAL_API_KEY");
        eprintln!("  - GROQ_API_KEY");
        eprintln!("  - XAI_API_KEY");
        eprintln!("  - DEEPSEEK_API_KEY");
        eprintln!("  - COHERE_API_KEY");
        eprintln!("  - TOGETHER_API_KEY");
        eprintln!("  - PERPLEXITY_API_KEY");
        eprintln!("  - CEREBRAS_API_KEY");
        eprintln!("  - DEEPINFRA_API_KEY");
        eprintln!("  - VERCEL_API_KEY");
        eprintln!("  - GITLAB_TOKEN");
        eprintln!("  - GITHUB_COPILOT_TOKEN");
        eprintln!("  - GOOGLE_VERTEX_API_KEY + GOOGLE_VERTEX_PROJECT_ID + GOOGLE_VERTEX_LOCATION");
        std::process::exit(1);
    }

    let tool_registry = Arc::new(create_default_registry().await);
    let agent_registry_arc = Arc::new(AgentRegistry::from_config(&config));
    let mut selection = CliRunSelection {
        model,
        provider,
        requested_agent,
        requested_scheduler_profile,
    };

    let mut runtime = build_cli_execution_runtime(
        &config,
        &current_dir,
        provider_registry.clone(),
        tool_registry.clone(),
        agent_registry_arc.clone(),
        &selection,
        None,
        None,
    )
    .await?;
    let repl_style = CliStyle::detect();

    // ── Server connection & session creation (unification Phase 3) ────
    let server_url = discover_or_start_server(None).await?;
    let api_client = Arc::new(CliApiClient::new(server_url.clone()));
    let session_info = api_client
        .create_session(None, selection.requested_scheduler_profile.clone())
        .await?;
    let server_session_id = session_info.id.clone();
    runtime.api_client = Some(api_client.clone());
    runtime.server_session_id = Some(server_session_id.clone());

    tracing::info!(
        server_url = %server_url,
        session_id = %server_session_id,
        "CLI connected to server and created session"
    );

    if single_shot {
        if let Some(prompt_text) = initial_prompt {
            process_message(&mut runtime, &prompt_text).await?;
        }
        return Ok(());
    }

    let shared_frontend_projection = runtime.frontend_projection.clone();
    let queued_inputs = runtime.queued_inputs.clone();
    let busy_flag = runtime.busy_flag.clone();
    let exit_requested = runtime.exit_requested.clone();
    let active_abort = runtime.active_abort.clone();
    let terminal_surface = Arc::new(CliTerminalSurface::new(
        repl_style.clone(),
        runtime.frontend_projection.clone(),
    ));
    let prompt_chrome = Arc::new(CliPromptChrome::new(&runtime, &repl_style, &current_dir));
    let (prompt_event_tx, mut prompt_event_rx) = mpsc::unbounded_channel();
    let prompt_session = Arc::new(PromptSession::spawn(
        Arc::new({
            let prompt_chrome = prompt_chrome.clone();
            move || prompt_chrome.frame()
        }),
        prompt_event_tx,
    )?);
    terminal_surface.set_prompt_session(prompt_session.clone());
    cli_attach_interactive_handles(
        &mut runtime,
        terminal_surface.clone(),
        prompt_chrome.clone(),
        prompt_session.clone(),
        queued_inputs.clone(),
        busy_flag.clone(),
        exit_requested.clone(),
        active_abort.clone(),
    );

    let (dispatch_tx, mut dispatch_rx) = mpsc::unbounded_channel::<CliDispatchInput>();
    tokio::spawn({
        let queued_inputs = queued_inputs.clone();
        let busy_flag = busy_flag.clone();
        let exit_requested = exit_requested.clone();
        let active_abort = active_abort.clone();
        let frontend_projection = shared_frontend_projection.clone();
        let prompt_session = prompt_session.clone();
        let terminal_surface = terminal_surface.clone();
        async move {
            while let Some(event) = prompt_event_rx.recv().await {
                match event {
                    PromptSessionEvent::Line(line) => {
                        let trimmed = line.trim().to_string();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if busy_flag.load(Ordering::SeqCst) {
                            if matches!(
                                parse_interactive_command(&trimmed),
                                Some(InteractiveCommand::Abort)
                            ) {
                                let handle = { active_abort.lock().await.clone() };
                                let aborted = match handle {
                                    Some(handle) => cli_trigger_abort(handle).await,
                                    None => false,
                                };
                                let _ = terminal_surface.print_block(OutputBlock::Status(
                                    if aborted {
                                        StatusBlock::warning("Abort requested for active run.")
                                    } else {
                                        StatusBlock::warning("No active run to abort.")
                                    },
                                ));
                                continue;
                            }
                            let queue_len = {
                                let mut queue = queued_inputs.lock().await;
                                queue.push_back(trimmed.clone());
                                queue.len()
                            };
                            if let Ok(mut projection) = frontend_projection.lock() {
                                projection.queue_len = queue_len;
                            }
                            let _ = prompt_session.refresh();
                            let _ = terminal_surface.print_block(OutputBlock::QueueItem(
                                QueueItemBlock {
                                    position: queue_len,
                                    text: truncate_text(&trimmed, 72),
                                },
                            ));
                        } else if dispatch_tx.send(CliDispatchInput::Line(trimmed)).is_err() {
                            break;
                        }
                    }
                    PromptSessionEvent::Eof => {
                        if busy_flag.load(Ordering::SeqCst) {
                            exit_requested.store(true, Ordering::SeqCst);
                            let _ = terminal_surface.print_block(OutputBlock::Status(
                                StatusBlock::muted("Exit requested after current run."),
                            ));
                        } else {
                            let _ = dispatch_tx.send(CliDispatchInput::Eof);
                            break;
                        }
                    }
                    PromptSessionEvent::Interrupt => {}
                }
            }
        }
    });

    // ── SSE event subscription (unification Phase 3) ─────────────────
    let (sse_tx, mut sse_rx) = mpsc::unbounded_channel::<CliServerEvent>();
    let sse_cancel = CancellationToken::new();
    let _sse_handle = event_stream::spawn_sse_subscriber(
        server_url.clone(),
        server_session_id.clone(),
        sse_tx,
        sse_cancel.clone(),
    );

    // ── Initial sidebar data fetch ──────────────────────────────────────
    cli_refresh_server_info(
        &api_client,
        &runtime.frontend_projection,
        Some(&server_session_id),
    )
    .await;

    if let Some(prompt_text) = initial_prompt {
        runtime.busy_flag.store(true, Ordering::SeqCst);
        process_message(&mut runtime, &prompt_text).await?;
        runtime.busy_flag.store(false, Ordering::SeqCst);
    }

    loop {
        let queued = {
            let mut queue = runtime.queued_inputs.lock().await;
            let next = queue.pop_front();
            let remaining = queue.len();
            if let Ok(mut projection) = runtime.frontend_projection.lock() {
                projection.queue_len = remaining;
            }
            next
        };
        let trimmed = match queued {
            Some(line) => line,
            None => {
                // Wait for either user input or SSE events.
                loop {
                    tokio::select! {
                        dispatch = dispatch_rx.recv() => {
                            match dispatch {
                                Some(CliDispatchInput::Line(line)) => break line,
                                Some(CliDispatchInput::Eof) | None => {
                                    sse_cancel.cancel();
                                    return Ok(());
                                }
                            }
                        }
                        sse_event = sse_rx.recv() => {
                            if let Some(event) = sse_event {
                                match event {
                                    CliServerEvent::QuestionCreated {
                                        request_id,
                                        session_id: _,
                                        questions_json,
                                    } => {
                                        // Handle question interactively and POST answer via HTTP.
                                        handle_question_from_sse(
                                            &runtime,
                                            &api_client,
                                            &request_id,
                                            &questions_json,
                                        ).await;
                                    }
                                    CliServerEvent::SessionUpdated {
                                        session_id,
                                        source,
                                    } => {
                                        handle_session_updated_from_sse(
                                            &runtime,
                                            &api_client,
                                            &session_id,
                                            source.as_deref(),
                                            &repl_style,
                                        ).await;
                                    }
                                    other => {
                                        handle_sse_event(&runtime, other, &repl_style);
                                    }
                                }
                            }
                            // Continue waiting for user input after handling SSE event.
                        }
                    }
                }
            }
        };

        if trimmed.is_empty() {
            continue;
        }

        if let Some(cmd) = parse_interactive_command(&trimmed) {
            match cmd {
                InteractiveCommand::Exit => break,
                InteractiveCommand::ShowHelp => {
                    show_help();
                }
                InteractiveCommand::Abort => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "No active run to abort. Use /abort while a response is running.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ShowRecovery => {
                    cli_print_recovery_actions(&runtime);
                }
                InteractiveCommand::ExecuteRecovery(selector) => {
                    let Some(action) = cli_select_recovery_action(&runtime, &selector) else {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(format!(
                                "Unknown recovery action: {}",
                                selector
                            ))),
                            &repl_style,
                        );
                        cli_print_recovery_actions(&runtime);
                        continue;
                    };
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::title(format!("↺ {}", action.label))),
                        &repl_style,
                    );
                    process_message_with_mode(&mut runtime, &action.prompt, false).await?;
                }
                InteractiveCommand::ClearScreen => {
                    if let Some(surface) = runtime.terminal_surface.as_ref() {
                        let _ = surface.clear_transcript();
                    } else {
                        print!("\x1B[2J\x1B[1;1H");
                        io::stdout().flush()?;
                    }
                }
                InteractiveCommand::NewSession => {
                    match api_client
                        .create_session(None, runtime.resolved_scheduler_profile_name.clone())
                        .await
                    {
                        Ok(new_session) => {
                            let new_sid = new_session.id.clone();
                            runtime.server_session_id = Some(new_sid.clone());

                            // Reset token stats and last rendered message ID for the new session.
                            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                                proj.token_stats = CliSessionTokenStats::default();
                            }
                            if let Ok(mut guard) = runtime.last_rendered_message_id.lock() {
                                *guard = None;
                            }

                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "New session created: {}",
                                    &new_sid[..new_sid.len().min(8)]
                                ))),
                                &repl_style,
                            );

                            // Refresh sidebar data for the new session.
                            cli_refresh_server_info(
                                &api_client,
                                &runtime.frontend_projection,
                                Some(&new_sid),
                            )
                            .await;
                        }
                        Err(e) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to create new session: {}",
                                    e
                                ))),
                                &repl_style,
                            );
                        }
                    }
                }
                InteractiveCommand::ShowStatus => {
                    let style = CliStyle::detect();

                    // Refresh server info before showing status.
                    cli_refresh_server_info(
                        &api_client,
                        &runtime.frontend_projection,
                        runtime.server_session_id.as_deref(),
                    )
                    .await;

                    let (phase, active_label, queue_len, token_stats, mcp_servers, lsp_servers) = runtime
                        .frontend_projection
                        .lock()
                        .map(|projection| {
                            (
                                match projection.phase {
                                    CliFrontendPhase::Idle => "idle",
                                    CliFrontendPhase::Busy => "busy",
                                    CliFrontendPhase::Waiting => "waiting",
                                    CliFrontendPhase::Cancelling => "cancelling",
                                    CliFrontendPhase::Failed => "failed",
                                }
                                .to_string(),
                                projection.active_label.clone(),
                                projection.queue_len,
                                projection.token_stats.clone(),
                                projection.mcp_servers.clone(),
                                projection.lsp_servers.clone(),
                            )
                        })
                        .unwrap_or_else(|_| ("unknown".to_string(), None, 0, CliSessionTokenStats::default(), Vec::new(), Vec::new()));
                    let mut lines = vec![
                        format!("Agent: {}", runtime.resolved_agent_name),
                        format!("Model: {}", runtime.resolved_model_label),
                        format!("Directory: {}", current_dir.display()),
                        format!("Runtime: {}", phase),
                    ];
                    if let Some(ref profile) = runtime.resolved_scheduler_profile_name {
                        lines.push(format!("Scheduler: {}", profile));
                    }
                    if let Some(active_label) =
                        active_label.filter(|value| !value.trim().is_empty())
                    {
                        lines.push(format!("Active: {}", active_label));
                    }
                    lines.push(format!("Queue: {}", queue_len));

                    // ── Context (tokens + cost) ─────────────────────
                    if token_stats.total_tokens > 0 {
                        lines.push(String::new());
                        lines.push(format!("Tokens: {} total", format_token_count(token_stats.total_tokens)));
                        lines.push(format!("  Input:     {}", format_token_count(token_stats.input_tokens)));
                        lines.push(format!("  Output:    {}", format_token_count(token_stats.output_tokens)));
                        if token_stats.reasoning_tokens > 0 {
                            lines.push(format!("  Reasoning: {}", format_token_count(token_stats.reasoning_tokens)));
                        }
                        if token_stats.cache_read_tokens > 0 {
                            lines.push(format!("  Cache R:   {}", format_token_count(token_stats.cache_read_tokens)));
                        }
                        if token_stats.cache_write_tokens > 0 {
                            lines.push(format!("  Cache W:   {}", format_token_count(token_stats.cache_write_tokens)));
                        }
                        lines.push(format!("Cost: ${:.4}", token_stats.total_cost));
                    }

                    // ── MCP servers ──────────────────────────────────
                    if !mcp_servers.is_empty() {
                        lines.push(String::new());
                        lines.push("MCP Servers:".to_string());
                        for server in &mcp_servers {
                            let detail = if server.tools > 0 {
                                format!(" ({} tools)", server.tools)
                            } else {
                                String::new()
                            };
                            lines.push(format!("  {} [{}]{}", server.name, server.status, detail));
                            if let Some(ref err) = server.error {
                                lines.push(format!("    ↳ {}", err));
                            }
                        }
                    }

                    // ── LSP servers ──────────────────────────────────
                    if !lsp_servers.is_empty() {
                        lines.push(String::new());
                        lines.push("LSP Servers:".to_string());
                        for server in &lsp_servers {
                            lines.push(format!("  {}", server));
                        }
                    }

                    if let Some(ref sid) = runtime.server_session_id {
                        lines.push(String::new());
                        lines.push(format!("Server: {}", api_client.base_url()));
                        lines.push(format!("Session: {}", sid));
                    }

                    let _ = print_cli_list_on_surface(Some(&runtime), "Session Status", None, &lines, &style);
                    cli_print_execution_topology(&runtime.observed_topology, &style);
                }
                InteractiveCommand::ListModels => {
                    let style = CliStyle::detect();
                    let mut lines = Vec::new();
                    for p in provider_registry.list() {
                        for m in p.models() {
                            lines.push(format!("{}:{}", p.id(), m.id));
                        }
                    }
                    let _ = print_cli_list_on_surface(Some(&runtime), "Available Models", None, &lines, &style);
                }
                InteractiveCommand::SelectModel(model_id) => {
                    let (provider, model) = parse_model_and_provider(Some(model_id.clone()));
                    if model.is_none() {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(format!(
                                "Invalid model selector: {}",
                                model_id
                            ))),
                            &repl_style,
                        );
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.provider = provider;
                    selection.model = model;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    runtime.frontend_projection = shared_frontend_projection.clone();
                    cli_attach_interactive_handles(
                        &mut runtime,
                        terminal_surface.clone(),
                        prompt_chrome.clone(),
                        prompt_session.clone(),
                        queued_inputs.clone(),
                        busy_flag.clone(),
                        exit_requested.clone(),
                        active_abort.clone(),
                    );
                    cli_switch_message(Some(&runtime), "model", &runtime.resolved_model_label);
                }
                InteractiveCommand::ListProviders => {
                    let style = CliStyle::detect();
                    let mut lines = Vec::new();
                    for p in provider_registry.list() {
                        let model_count = p.models().len();
                        lines.push(format!(
                            "{} ({} model{})",
                            p.id(),
                            model_count,
                            if model_count != 1 { "s" } else { "" }
                        ));
                    }
                    let _ = print_cli_list_on_surface(Some(&runtime), "Configured Providers", None, &lines, &style);
                }
                InteractiveCommand::ListThemes => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "Theme switching is not yet supported in CLI mode.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ListPresets => {
                    cli_list_presets(&config, runtime.resolved_scheduler_profile_name.as_deref());
                }
                InteractiveCommand::SelectPreset(name) => {
                    if !cli_has_preset(&config, &name) {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(format!(
                                "Unknown preset: {}",
                                name
                            ))),
                            &repl_style,
                        );
                        cli_list_presets(
                            &config,
                            runtime.resolved_scheduler_profile_name.as_deref(),
                        );
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.requested_scheduler_profile = Some(name.clone());
                    selection.requested_agent = None;
                    selection.model = None;
                    selection.provider = None;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    runtime.frontend_projection = shared_frontend_projection.clone();
                    cli_attach_interactive_handles(
                        &mut runtime,
                        terminal_surface.clone(),
                        prompt_chrome.clone(),
                        prompt_session.clone(),
                        queued_inputs.clone(),
                        busy_flag.clone(),
                        exit_requested.clone(),
                        active_abort.clone(),
                    );
                    cli_switch_message(
                        Some(&runtime),
                        "preset",
                        runtime
                            .resolved_scheduler_profile_name
                            .as_deref()
                            .unwrap_or(name.as_str()),
                    );
                }
                InteractiveCommand::ListSessions => {
                    cli_list_sessions().await;
                }
                InteractiveCommand::ListAgents => {
                    let style = CliStyle::detect();
                    let mut lines = Vec::new();
                    for info in agent_registry_arc.list() {
                        let active = if info.name == runtime.resolved_agent_name {
                            " ← active".to_string()
                        } else {
                            String::new()
                        };
                        let model_info = info
                            .model
                            .as_ref()
                            .map(|m| format!(" ({}/{})", m.provider_id, m.model_id))
                            .unwrap_or_default();
                        lines.push(format!("{}{}{}", info.name, model_info, active));
                    }
                    let _ = print_cli_list_on_surface(Some(&runtime), "Available Agents", None, &lines, &style);
                }
                InteractiveCommand::SelectAgent(name) => {
                    if agent_registry_arc.get(&name).is_none() {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(format!(
                                "Unknown agent: {}",
                                name
                            ))),
                            &repl_style,
                        );
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.requested_agent = Some(name.clone());
                    selection.requested_scheduler_profile = None;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    runtime.frontend_projection = shared_frontend_projection.clone();
                    cli_attach_interactive_handles(
                        &mut runtime,
                        terminal_surface.clone(),
                        prompt_chrome.clone(),
                        prompt_session.clone(),
                        queued_inputs.clone(),
                        busy_flag.clone(),
                        exit_requested.clone(),
                        active_abort.clone(),
                    );
                    cli_switch_message(Some(&runtime), "agent", &runtime.resolved_agent_name);
                }
                InteractiveCommand::Compact => {
                    let Some(ref sid) = runtime.server_session_id else {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(
                                "No server session to compact.",
                            )),
                            &repl_style,
                        );
                        continue;
                    };
                    let sid = sid.clone();
                    match api_client.compact_session(&sid).await {
                        Ok(_result) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::title(
                                    "Session compacted successfully.",
                                )),
                                &repl_style,
                            );
                            // Reset token stats and re-fetch from compacted session.
                            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                                proj.token_stats = CliSessionTokenStats::default();
                            }
                            if let Ok(mut guard) = runtime.last_rendered_message_id.lock() {
                                *guard = None;
                            }
                            cli_refresh_server_info(
                                &api_client,
                                &runtime.frontend_projection,
                                Some(&sid),
                            )
                            .await;
                        }
                        Err(e) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to compact session: {}",
                                    e
                                ))),
                                &repl_style,
                            );
                        }
                    }
                }
                InteractiveCommand::Copy => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "/copy is not yet supported in CLI mode.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ListTasks => {
                    cli_list_tasks();
                }
                InteractiveCommand::ShowTask(id) => {
                    cli_show_task(&id);
                }
                InteractiveCommand::KillTask(id) => {
                    cli_kill_task(&id);
                }
                InteractiveCommand::ToggleSidebar => {
                    let state_msg = if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        projection.sidebar_collapsed = !projection.sidebar_collapsed;
                        Some(if projection.sidebar_collapsed { "collapsed" } else { "expanded" })
                    } else {
                        None
                    };
                    // print_block AFTER lock is released — it internally locks projection too
                    if let Some(state) = state_msg {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::success(format!("Sidebar {state}."))),
                            &repl_style,
                        );
                    }
                    cli_refresh_prompt(&runtime);
                }
                InteractiveCommand::ToggleActive => {
                    let state_msg = if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        projection.active_collapsed = !projection.active_collapsed;
                        Some(if projection.active_collapsed { "collapsed" } else { "expanded" })
                    } else {
                        None
                    };
                    // print_block AFTER lock is released — it internally locks projection too
                    if let Some(state) = state_msg {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::success(format!("Active panel {state}."))),
                            &repl_style,
                        );
                    }
                    cli_refresh_prompt(&runtime);
                }
                InteractiveCommand::ScrollUp => {
                    if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        let page_size = 10usize;
                        projection.scroll_offset = projection.scroll_offset.saturating_add(page_size);
                    }
                    cli_refresh_prompt(&runtime);
                }
                InteractiveCommand::ScrollDown => {
                    if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        let page_size = 10usize;
                        projection.scroll_offset = projection.scroll_offset.saturating_sub(page_size);
                    }
                    cli_refresh_prompt(&runtime);
                }
                InteractiveCommand::ScrollBottom => {
                    if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        projection.scroll_offset = 0;
                    }
                    cli_refresh_prompt(&runtime);
                }
                InteractiveCommand::Unknown(name) => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown command: /{}. Type /help for available commands.",
                            name
                        ))),
                        &repl_style,
                    );
                }
            }
            continue;
        }

        runtime.busy_flag.store(true, Ordering::SeqCst);

        // Scheduler mode with server connection: send prompt via HTTP only,
        // execution and rendering handled via SSE events (session.updated,
        // question.created, etc.). No local execution needed.
        let scheduler_via_server = runtime.resolved_scheduler_profile_name.is_some()
            && runtime.api_client.is_some()
            && runtime.server_session_id.is_some();

        if scheduler_via_server {
            let api = runtime.api_client.as_ref().unwrap();
            let sid = runtime.server_session_id.as_ref().unwrap();

            // Reset last rendered message ID so we can track incremental rendering.
            // The user message will be the first new message from the server.
            // (We render it locally below, so start tracking after it.)

            // Print user message locally.
            let style = CliStyle::detect();
            if let Ok(mut topology) = runtime.observed_topology.lock() {
                topology.reset_for_run(
                    &runtime.resolved_agent_name,
                    runtime.resolved_scheduler_profile_name.as_deref(),
                );
            }
            cli_frontend_set_phase(
                &runtime.frontend_projection,
                CliFrontendPhase::Busy,
                Some(
                    runtime
                        .resolved_scheduler_profile_name
                        .as_deref()
                        .map(|profile| format!("preset {}", profile))
                        .unwrap_or_else(|| "assistant response".to_string()),
                ),
            );
            print_block(
                Some(&runtime),
                OutputBlock::Message(MessageBlock::full(
                    OutputMessageRole::User,
                    trimmed.clone(),
                )),
                &style,
            )?;

            // Set up abort handle for server-side execution.
            {
                let mut active_abort = runtime.active_abort.lock().await;
                *active_abort = Some(CliActiveAbortHandle::Server {
                    api_client: api.clone(),
                    session_id: sid.clone(),
                });
            }

            // Store recovery base prompt.
            runtime.recovery_base_prompt = Some(trimmed.clone());

            // Send prompt to server via HTTP — execution happens server-side.
            if let Err(e) = api.send_prompt(
                sid,
                trimmed.clone(),
                Some(runtime.resolved_agent_name.clone()),
                runtime.resolved_scheduler_profile_name.clone(),
                (runtime.resolved_model_label != "auto").then(|| runtime.resolved_model_label.clone()),
                None,
            ).await {
                let _ = print_block(
                    Some(&runtime),
                    OutputBlock::Status(StatusBlock::error(format!("Failed to send prompt: {}", e))),
                    &repl_style,
                );
                runtime.busy_flag.store(false, Ordering::SeqCst);
                let mut active_abort = runtime.active_abort.lock().await;
                *active_abort = None;
                continue;
            }

            // The REPL loop will continue — SSE events will drive rendering:
            // - session.updated → handle_session_updated_from_sse (message rendering)
            // - question.created → handle_question_from_sse (interactive Q&A)
            // - session.idle → marks run complete
            // Wait for the server-side execution to complete by draining SSE events.
            loop {
                match sse_rx.recv().await {
                    Some(CliServerEvent::QuestionCreated {
                        request_id,
                        session_id: _,
                        questions_json,
                    }) => {
                        handle_question_from_sse(
                            &runtime,
                            &api_client,
                            &request_id,
                            &questions_json,
                        ).await;
                    }
                    Some(CliServerEvent::SessionUpdated {
                        session_id,
                        source,
                    }) => {
                        handle_session_updated_from_sse(
                            &runtime,
                            &api_client,
                            &session_id,
                            source.as_deref(),
                            &repl_style,
                        ).await;
                    }
                    Some(CliServerEvent::SessionIdle { .. }) => {
                        // Server-side execution is complete.
                        cli_frontend_set_phase(
                            &runtime.frontend_projection,
                            CliFrontendPhase::Idle,
                            None,
                        );
                        cli_refresh_prompt(&runtime);
                        // Do a final message sync to catch any remaining messages.
                        if let Some(sid) = runtime.server_session_id.as_deref() {
                            handle_session_updated_from_sse(
                                &runtime,
                                &api_client,
                                sid,
                                Some("prompt.done"),
                                &repl_style,
                            ).await;
                        }
                        if let Ok(mut topology) = runtime.observed_topology.lock() {
                            topology.finish_run(Some("Completed".to_string()));
                        }
                        cli_frontend_clear(&runtime);
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::success("Done.")),
                            &repl_style,
                        );
                        break;
                    }
                    Some(other) => {
                        handle_sse_event(&runtime, other, &repl_style);
                    }
                    None => {
                        // SSE channel closed.
                        break;
                    }
                }
            }

            {
                let mut active_abort = runtime.active_abort.lock().await;
                *active_abort = None;
            }
        } else {
            // Non-scheduler (agent) mode: local execution only.
            process_message(&mut runtime, &trimmed).await?;
        }

        // Drain any SSE events that arrived during processing.
        while let Ok(event) = sse_rx.try_recv() {
            match event {
                CliServerEvent::QuestionCreated {
                    request_id,
                    session_id: _,
                    questions_json,
                } => {
                    handle_question_from_sse(
                        &runtime,
                        &api_client,
                        &request_id,
                        &questions_json,
                    ).await;
                }
                CliServerEvent::SessionUpdated {
                    session_id,
                    source,
                } => {
                    handle_session_updated_from_sse(
                        &runtime,
                        &api_client,
                        &session_id,
                        source.as_deref(),
                        &repl_style,
                    ).await;
                }
                other => {
                    handle_sse_event(&runtime, other, &repl_style);
                }
            }
        }

        runtime.busy_flag.store(false, Ordering::SeqCst);
        if runtime.exit_requested.load(Ordering::SeqCst)
            && runtime.queued_inputs.lock().await.is_empty()
        {
            break;
        }
    }

    sse_cancel.cancel();
    Ok(())
}

async fn process_message(runtime: &mut CliExecutionRuntime, input: &str) -> anyhow::Result<()> {
    process_message_with_mode(runtime, input, true).await
}

/// Handle an incoming SSE event from the server — update topology,
/// frontend projection, and render output blocks.
fn handle_sse_event(
    runtime: &CliExecutionRuntime,
    event: CliServerEvent,
    style: &CliStyle,
) {
    // Helper to check if the event belongs to our session.
    let is_my_session = |event_session_id: &str| -> bool {
        runtime
            .server_session_id
            .as_deref()
            .map_or(true, |my_sid| event_session_id.is_empty() || event_session_id == my_sid)
    };

    match event {
        CliServerEvent::SessionUpdated { session_id, source } => {
            if !is_my_session(&session_id) {
                return;
            }
            tracing::debug!(session_id, ?source, "session updated");
        }
        CliServerEvent::SessionBusy { session_id } => {
            if !is_my_session(&session_id) {
                return;
            }
            cli_frontend_set_phase(
                &runtime.frontend_projection,
                CliFrontendPhase::Busy,
                Some("server processing".to_string()),
            );
            cli_refresh_prompt(runtime);
        }
        CliServerEvent::SessionIdle { session_id } => {
            if !is_my_session(&session_id) {
                return;
            }
            cli_frontend_set_phase(
                &runtime.frontend_projection,
                CliFrontendPhase::Idle,
                None,
            );
            cli_refresh_prompt(runtime);
        }
        CliServerEvent::SessionRetrying { session_id } => {
            if !is_my_session(&session_id) {
                return;
            }
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning("Retrying…")),
                style,
            );
        }
        CliServerEvent::QuestionCreated { request_id, session_id, .. } => {
            // Handled inline in the REPL loop (needs async). Should not reach here.
            tracing::warn!(request_id, session_id, "question.created reached sync handler — skipping");
        }
        CliServerEvent::QuestionReplied { request_id } => {
            tracing::debug!(request_id, "question replied");
        }
        CliServerEvent::QuestionRejected { request_id } => {
            tracing::debug!(request_id, "question rejected");
        }
        CliServerEvent::ToolCallStarted { session_id, tool_call_id, tool_name } => {
            if !is_my_session(&session_id) {
                return;
            }
            if let Ok(mut topology) = runtime.observed_topology.lock() {
                topology.active = true;
            }
            tracing::debug!(tool_call_id, tool_name, "tool call started");
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title(format!("⚙ {}", tool_name))),
                style,
            );
        }
        CliServerEvent::OutputBlock { id, payload } => {
            // OutputBlock events from SSE carry a web-specific JSON format.
            // TUI also doesn't process these — rendering comes from session.updated.
            tracing::trace!(?id, "SSE output_block received");
            let _ = payload; // Future: parse and render inline.
        }
        CliServerEvent::Error { error, message_id, done } => {
            tracing::error!(error, ?message_id, ?done, "server error");
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(error)),
                style,
            );
        }
        CliServerEvent::Usage { prompt_tokens, completion_tokens, message_id } => {
            tracing::debug!(prompt_tokens, completion_tokens, ?message_id, "token usage");
            if prompt_tokens > 0 || completion_tokens > 0 {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::success(format!(
                        "tokens: prompt={} completion={}",
                        prompt_tokens, completion_tokens
                    ))),
                    style,
                );
            }
        }
        CliServerEvent::Unknown { event, data } => {
            tracing::trace!("Ignoring unknown SSE event: {} ({})", event, data);
        }
    }
}

/// Handle a `question.created` SSE event: parse the question definitions,
/// present them interactively via the CLI select widgets, and POST the
/// answers back to the server via the HTTP API.
async fn handle_question_from_sse(
    runtime: &CliExecutionRuntime,
    api_client: &Arc<CliApiClient>,
    request_id: &str,
    questions_json: &serde_json::Value,
) {
    // 1. Parse Vec<QuestionDef> from the SSE payload.
    let questions: Vec<rocode_tool::QuestionDef> = match serde_json::from_value(questions_json.clone()) {
        Ok(qs) => qs,
        Err(e) => {
            tracing::warn!("Failed to parse questions from SSE: {}", e);
            // Reject the question so the server doesn't hang waiting.
            let _ = api_client.reject_question(request_id).await;
            return;
        }
    };

    if questions.is_empty() {
        tracing::debug!("Empty question list from SSE — rejecting");
        let _ = api_client.reject_question(request_id).await;
        return;
    }

    // 2. Present questions interactively using the existing CLI question handler.
    let guard = runtime.spinner_guard.lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| SpinnerGuard::noop());
    let result = cli_ask_question(
        questions,
        runtime.observed_topology.clone(),
        runtime.frontend_projection.clone(),
        runtime.prompt_session_slot.clone(),
        guard,
    )
    .await;

    match result {
        Ok(answers) => {
            // 3. POST answers back to the server.
            if let Err(e) = api_client.reply_question(request_id, answers).await {
                tracing::error!("Failed to reply question `{}`: {}", request_id, e);
            }
        }
        Err(_) => {
            // User cancelled or error — reject the question.
            if let Err(e) = api_client.reject_question(request_id).await {
                tracing::error!("Failed to reject question `{}`: {}", request_id, e);
            }
        }
    }
}

/// Refresh MCP/LSP status and session token stats from the server.
///
/// Called periodically while idle and after SSE events to keep the sidebar
/// and `/status` output up to date.
async fn cli_refresh_server_info(
    api_client: &CliApiClient,
    projection: &Arc<Mutex<CliFrontendProjection>>,
    server_session_id: Option<&str>,
) {
    // ── MCP servers ─────────────────────────────────────────────
    match api_client.get_mcp_status().await {
        Ok(servers) => {
            let statuses: Vec<CliMcpServerStatus> = servers.into_iter().map(Into::into).collect();
            if let Ok(mut proj) = projection.lock() {
                proj.mcp_servers = statuses;
            }
        }
        Err(e) => {
            tracing::debug!("Failed to refresh MCP status: {}", e);
        }
    }

    // ── LSP servers ─────────────────────────────────────────────
    match api_client.get_lsp_servers().await {
        Ok(servers) => {
            if let Ok(mut proj) = projection.lock() {
                proj.lsp_servers = servers;
            }
        }
        Err(e) => {
            tracing::debug!("Failed to refresh LSP status: {}", e);
        }
    }

    // ── Session token stats ─────────────────────────────────────
    if let Some(sid) = server_session_id {
        match api_client.get_messages(sid).await {
            Ok(messages) => {
                let mut stats = CliSessionTokenStats::default();
                for msg in &messages {
                    if msg.role == "assistant" {
                        stats.accumulate(&msg.tokens, msg.cost);
                    }
                }
                if let Ok(mut proj) = projection.lock() {
                    proj.token_stats = stats;
                }
            }
            Err(e) => {
                tracing::debug!("Failed to refresh token stats: {}", e);
            }
        }
    }
}

/// Handle a `session.updated` SSE event: fetch new messages from the server
/// and render them incrementally in the CLI.
async fn handle_session_updated_from_sse(
    runtime: &CliExecutionRuntime,
    api_client: &Arc<CliApiClient>,
    session_id: &str,
    _source: Option<&str>,
    style: &CliStyle,
) {
    let server_sid = match runtime.server_session_id.as_deref() {
        Some(sid) if sid == session_id => sid,
        _ => return, // Not our session.
    };

    // Get the last rendered message ID for incremental fetching.
    let after = runtime
        .last_rendered_message_id
        .lock()
        .ok()
        .and_then(|guard| guard.clone());

    // Fetch new messages from the server.
    let messages = match api_client
        .get_messages_after(server_sid, after.as_deref(), None)
        .await
    {
        Ok(msgs) => msgs,
        Err(e) => {
            tracing::warn!("Failed to fetch messages after session.updated: {}", e);
            return;
        }
    };

    if messages.is_empty() {
        return;
    }

    // Render each new message as OutputBlocks.
    for msg in &messages {
        let role = match msg.role.as_str() {
            "user" => OutputMessageRole::User,
            "assistant" => OutputMessageRole::Assistant,
            _ => OutputMessageRole::Assistant,
        };

        // Collect all text parts into a single string.
        let mut text = String::new();
        let mut has_scheduler_stage = false;

        for part in &msg.parts {
            match part.part_type.as_str() {
                "text" => {
                    if let Some(t) = part.text.as_deref() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                "tool_call" => {
                    if let Some(tc) = &part.tool_call {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::title(format!(
                                "⚙ {}",
                                tc.name
                            ))),
                            style,
                        );
                    }
                }
                "tool_result" => {
                    // Tool results are usually shown via tool call display.
                }
                _ => {}
            }

            // Check for scheduler stage metadata.
            if msg.metadata.as_ref().map_or(false, |m| {
                m.contains_key("scheduler_stage_name")
            }) {
                has_scheduler_stage = true;
            }
        }

        // Render text content if present.
        if !text.is_empty() {
            if has_scheduler_stage {
                // Render as scheduler stage block.
                let stage_name = msg
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("scheduler_stage_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("stage")
                    .to_string();
                let stage_index = msg
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("scheduler_stage_index"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let stage_total = msg
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("scheduler_stage_total"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);

                let _ = print_block(
                    Some(runtime),
                    OutputBlock::SchedulerStage(SchedulerStageBlock {
                        profile: runtime.resolved_scheduler_profile_name.clone(),
                        stage: stage_name.clone(),
                        title: stage_name,
                        text,
                        stage_index: Some(stage_index as u64),
                        stage_total: stage_total.map(|v| v as u64),
                        status: Some("done".to_string()),
                        step: None,
                        focus: None,
                        last_event: None,
                        waiting_on: None,
                        activity: None,
                        available_skill_count: None,
                        available_agent_count: None,
                        available_category_count: None,
                        active_skills: vec![],
                        active_agents: vec![],
                        active_categories: vec![],
                        prompt_tokens: None,
                        completion_tokens: None,
                        reasoning_tokens: None,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        decision: None,
                    }),
                    style,
                );
            } else if role == OutputMessageRole::User {
                // Skip user messages — we already rendered them locally.
            } else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Message(MessageBlock::full(role, text)),
                    style,
                );
            }
        }

        // Check for error.
        if let Some(error) = &msg.error {
            if !error.is_empty() {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::error(error.clone())),
                    style,
                );
            }
        }
    }

    // Accumulate token stats from new assistant messages.
    if let Ok(mut proj) = runtime.frontend_projection.lock() {
        for msg in &messages {
            if msg.role == "assistant" {
                proj.token_stats.accumulate(&msg.tokens, msg.cost);
            }
        }
    }

    // Update the last rendered message ID.
    if let Some(last_msg) = messages.last() {
        if let Ok(mut guard) = runtime.last_rendered_message_id.lock() {
            *guard = Some(last_msg.id.clone());
        }
    }
}

async fn process_scheduler_message(
    runtime: &mut CliExecutionRuntime,
    input: &str,
    style: &CliStyle,
) -> anyhow::Result<()> {
    let state = runtime
        .local_scheduler_state
        .clone()
        .ok_or_else(|| anyhow::anyhow!("local scheduler state is not initialized"))?;
    let profile = runtime
        .resolved_scheduler_profile_name
        .clone()
        .ok_or_else(|| anyhow::anyhow!("scheduler profile is not initialized"))?;
    let session_id = runtime.local_scheduler_session_id.clone();
    let model = (runtime.resolved_model_label != "auto").then(|| runtime.resolved_model_label.clone());
    if let Some(session_id) = session_id.clone() {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = Some(CliActiveAbortHandle::Scheduler {
            state: state.clone(),
            session_id,
        });
    }

    if let Ok(mut shared) = runtime.spinner_guard.lock() {
        *shared = SpinnerGuard::noop();
    }
    {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = None;
    }

    let observed_topology = runtime.observed_topology.clone();
    let frontend_projection = runtime.frontend_projection.clone();
    let stage_snapshots = runtime.scheduler_stage_snapshots.clone();
    let surface = runtime.terminal_surface.clone();
    let prompt_session = runtime.prompt_session.clone();
    let output_hook: Arc<dyn Fn(OutputBlock) + Send + Sync> = Arc::new({
        let style = style.clone();
        let surface = surface.clone();
        let prompt_session = prompt_session.clone();
        move |block| {
            if let Ok(mut topology) = observed_topology.lock() {
                topology.observe_block(&block);
            }
            cli_frontend_observe_block(&frontend_projection, &block);
            match &block {
                OutputBlock::SchedulerStage(stage)
                    if !cli_should_emit_scheduler_stage_block(&stage_snapshots, stage) => {}
                OutputBlock::SchedulerStage(stage) if !cli_is_terminal_stage_status(stage.status.as_deref()) => {
                    if let Ok(mut projection) = frontend_projection.lock() {
                        projection.active_stage = Some(stage.clone());
                        projection.active_collapsed = false; // auto-expand when stage is active
                    }
                    if let Some(prompt_session) = prompt_session.as_ref() {
                        let _ = prompt_session.refresh();
                    }
                }
                OutputBlock::SchedulerStage(_stage) => {
                    if let Ok(mut projection) = frontend_projection.lock() {
                        projection.active_stage = None;
                        projection.active_collapsed = true; // auto-collapse when stage ends
                    }
                    if let Some(prompt_session) = prompt_session.as_ref() {
                        let _ = prompt_session.refresh();
                    }
                    let _ = print_block_on_surface(surface.as_deref(), block, &style);
                }
                _ => {
                    let _ = print_block_on_surface(surface.as_deref(), block, &style);
                }
            }
        }
    });

    let result = run_local_scheduler_prompt(
        state,
        LocalSchedulerPromptRequest {
            session_id,
            directory: std::env::current_dir()?.display().to_string(),
            prompt_text: input.to_string(),
            display_prompt_text: input.to_string(),
            scheduler_profile: profile,
            model,
            variant: None,
        },
        Some(output_hook),
    )
    .await;

    if let Ok(mut shared) = runtime.spinner_guard.lock() {
        *shared = SpinnerGuard::noop();
    }

    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            {
                let mut active_abort = runtime.active_abort.lock().await;
                *active_abort = None;
            }
            cli_frontend_set_phase(
                &runtime.frontend_projection,
                CliFrontendPhase::Failed,
                Some("scheduler failed".to_string()),
            );
            return Err(error);
        }
    };
    runtime.local_scheduler_session_id = Some(outcome.session_id.clone());

    // Update session title in frontend projection
    if let Some(state) = runtime.local_scheduler_state.as_ref() {
        let sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get(&outcome.session_id) {
            if let Ok(mut projection) = runtime.frontend_projection.lock() {
                projection.session_title = Some(session.title.clone());
            }
        }
    }

    if outcome.cancelled {
        cli_frontend_clear(runtime);
        print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::warning("Aborted current response.")),
            style,
        )?;
    } else {
        cli_frontend_clear(runtime);
        if outcome.prompt_tokens > 0 || outcome.completion_tokens > 0 {
            print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::success(format!(
                    "Done. tokens: prompt={} completion={}",
                    outcome.prompt_tokens, outcome.completion_tokens
                ))),
                style,
            )?;
        } else {
            print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::success("Done.")),
                style,
            )?;
        }
    }

    if let Ok(mut topology) = runtime.observed_topology.lock() {
        topology.finish_run(Some(if outcome.cancelled {
            "cancelled".to_string()
        } else {
            "completed".to_string()
        }));
    }
    cli_frontend_clear(runtime);
    Ok(())
}

async fn process_message_with_mode(
    runtime: &mut CliExecutionRuntime,
    input: &str,
    update_recovery_base: bool,
) -> anyhow::Result<()> {
    if update_recovery_base {
        runtime.recovery_base_prompt = Some(input.to_string());
    }
    let style = CliStyle::detect();
    if let Ok(mut topology) = runtime.observed_topology.lock() {
        topology.reset_for_run(
            &runtime.resolved_agent_name,
            runtime.resolved_scheduler_profile_name.as_deref(),
        );
    }
    if let Ok(mut snapshots) = runtime.scheduler_stage_snapshots.lock() {
        snapshots.clear();
    }
    cli_frontend_set_phase(
        &runtime.frontend_projection,
        CliFrontendPhase::Busy,
        Some(
            runtime
                .resolved_scheduler_profile_name
                .as_deref()
                .map(|profile| format!("preset {}", profile))
                .unwrap_or_else(|| "assistant response".to_string()),
        ),
    );

    print_block(
        Some(runtime),
        OutputBlock::Message(MessageBlock::full(
            OutputMessageRole::User,
            input.to_string(),
        )),
        &style,
    )?;

    if runtime.resolved_scheduler_profile_name.is_some() {
        return process_scheduler_message(runtime, input, &style).await;
    }

    if let Ok(mut shared) = runtime.spinner_guard.lock() {
        *shared = SpinnerGuard::noop();
    }
    let cancel_token = CancellationToken::new();
    let observed_topology = runtime.observed_topology.clone();
    let frontend_projection = runtime.frontend_projection.clone();
    let surface = runtime.terminal_surface.clone();
    {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = Some(CliActiveAbortHandle::Agent(cancel_token.clone()));
    }

    let mut md_streamer = MarkdownStreamer::new(&style).with_continuation_prefix("  ");
    let run_future = Box::pin(stream_prompt_to_blocks_with_cancel(
        &mut runtime.executor,
        input,
        cancel_token.clone(),
        |block| {
            if let Ok(mut topology) = observed_topology.lock() {
                topology.observe_block(&block);
            }
            cli_frontend_observe_block(&frontend_projection, &block);
            // Intercept message deltas for markdown rendering
            match &block {
                OutputBlock::Message(msg) if msg.phase == MessagePhase::Start =>
                    print_block_on_surface(surface.as_deref(), block, &style),
                OutputBlock::Message(msg) if msg.phase == MessagePhase::Delta => {
                    let rendered = md_streamer.push(&msg.text);
                    if !rendered.is_empty() {
                        if let Some(surface) = surface.as_deref() {
                            surface.print_text(&rendered)?;
                        } else {
                            print!("{}", rendered);
                            io::stdout().flush()?;
                        }
                    }
                    Ok(())
                }
                OutputBlock::Message(msg) if msg.phase == MessagePhase::End => {
                    let remaining = md_streamer.finish();
                    if !remaining.is_empty() {
                        if let Some(surface) = surface.as_deref() {
                            surface.print_text(&remaining)?;
                        } else {
                            print!("{}", remaining);
                            io::stdout().flush()?;
                        }
                    }
                    print_block_on_surface(surface.as_deref(), block, &style)
                }
                OutputBlock::Tool(_tool) => print_block_on_surface(surface.as_deref(), block, &style),
                _ => print_block_on_surface(surface.as_deref(), block, &style),
            }
        },
    ));
    let stats = run_future.await;

    {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = None;
    }

    let (prompt_tokens, completion_tokens, stream_failed, cancelled) = match stats {
        Ok(stats) => (stats.prompt_tokens, stats.completion_tokens, false, false),
        Err(error) => {
            let cancelled = error
                .downcast_ref::<AgentExecutorError>()
                .is_some_and(|agent_error| matches!(agent_error, AgentExecutorError::Cancelled));
            if cancelled {
                cli_frontend_clear(runtime);
                print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("Aborted current response.")),
                    &style,
                )?;
            } else {
                cli_frontend_set_phase(
                    &runtime.frontend_projection,
                    CliFrontendPhase::Failed,
                    Some("run failed".to_string()),
                );
                print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::error(error.to_string())),
                    &style,
                )?;
            }
            (0, 0, !cancelled, cancelled)
        }
    };

    if !stream_failed && !cancelled {
        cli_frontend_clear(runtime);
        if prompt_tokens > 0 || completion_tokens > 0 {
            print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::success(format!(
                    "Done. tokens: prompt={} completion={}",
                    prompt_tokens, completion_tokens
                ))),
                &style,
            )?;
        } else {
            print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::success("Done.")),
                &style,
            )?;
        }
    }
    if let Ok(mut topology) = runtime.observed_topology.lock() {
        topology.finish_run(if cancelled {
            Some("Cancelled".to_string())
        } else if stream_failed {
            Some("Failed".to_string())
        } else {
            Some("Completed".to_string())
        });
    }
    cli_frontend_clear(runtime);
    Ok(())
}

#[derive(Debug, Clone)]
struct CliRecoveryAction {
    key: &'static str,
    label: String,
    description: String,
    prompt: String,
}

fn cli_recovery_actions(runtime: &CliExecutionRuntime) -> Vec<CliRecoveryAction> {
    let Some(base_prompt) = runtime.recovery_base_prompt.as_deref() else {
        return Vec::new();
    };

    let mut actions = vec![
        CliRecoveryAction {
            key: "retry",
            label: "Retry last run".to_string(),
            description: "Re-run the last request with the same mode and constraints.".to_string(),
            prompt: format!(
                "Recovery protocol: retry the previous request with the same mode and constraints.\nPreserve any valid prior work, but re-run the task end-to-end where needed.\n\nOriginal request:\n{}",
                base_prompt
            ),
        },
        CliRecoveryAction {
            key: "resume",
            label: "Resume from latest boundary".to_string(),
            description: "Continue from the latest incomplete boundary without restarting discovery.".to_string(),
            prompt: format!(
                "Recovery protocol: resume from the latest incomplete boundary.\nDo not restart discovery from scratch. Preserve prior verified work, artifacts, decisions, and constraints.\n\nOriginal request:\n{}",
                base_prompt
            ),
        },
    ];

    if let Some((stage_label, stage_summary)) = cli_latest_recovery_stage(runtime) {
        actions.push(CliRecoveryAction {
            key: "restart-stage",
            label: format!("Restart stage · {}", stage_label),
            description: "Re-enter this stage as a fresh boundary and recompute downstream work.".to_string(),
            prompt: format!(
                "Recovery protocol: restart scheduler stage `{}`.\nRe-enter this stage as a fresh boundary. Preserve global constraints and prior validated upstream context, but allow this stage and all downstream work to be recomputed from here.\n\nPrevious stage outcome:\n{}\n\nOriginal request:\n{}",
                stage_label, stage_summary, base_prompt
            ),
        });
        actions.push(CliRecoveryAction {
            key: "partial-replay",
            label: format!("Partial replay · {}", stage_label),
            description: "Replay only from this stage boundary and preserve valid prior work.".to_string(),
            prompt: format!(
                "Recovery protocol: partial replay from scheduler stage `{}`.\nRestart from this stage boundary only. Preserve all prior valid work and replay only the downstream work required after this stage.\n\nPrevious stage outcome:\n{}\n\nOriginal request:\n{}",
                stage_label, stage_summary, base_prompt
            ),
        });
    }

    actions
}

fn cli_latest_recovery_stage(runtime: &CliExecutionRuntime) -> Option<(String, String)> {
    let topology = runtime.observed_topology.lock().ok()?;
    let stage_id = topology.stage_order.last()?;
    let stage = topology.nodes.get(stage_id)?;
    let summary = stage
        .recent_event
        .clone()
        .or_else(|| stage.waiting_on.clone())
        .unwrap_or_else(|| stage.status.clone());
    Some((stage.label.clone(), summary))
}

fn cli_print_recovery_actions(runtime: &CliExecutionRuntime) {
    let style = CliStyle::detect();
    let actions = cli_recovery_actions(runtime);
    if actions.is_empty() {
        let lines = vec![
            "No recovery actions available".to_string(),
            "Send a prompt first, then use /recover".to_string(),
        ];
        let rendered = render_cli_list("Recovery Actions", None, &lines, &style);
        print!("{}", rendered);
        let _ = io::stdout().flush();
        return;
    }
    let mut lines = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        lines.push(format!("{}. {}  [{}]", index + 1, action.label, action.key));
        lines.push(format!("   {}", action.description));
    }
    let rendered = render_cli_list(
        "Recovery Actions",
        Some("Use /recover <number|key> to execute"),
        &lines,
        &style,
    );
    print!("{}", rendered);
    let _ = io::stdout().flush();
}

fn cli_select_recovery_action(
    runtime: &CliExecutionRuntime,
    selector: &str,
) -> Option<CliRecoveryAction> {
    let actions = cli_recovery_actions(runtime);
    let normalized = selector.trim().to_ascii_lowercase().replace('_', "-");
    if let Ok(index) = normalized.parse::<usize>() {
        return actions.get(index.saturating_sub(1)).cloned();
    }
    actions
        .into_iter()
        .find(|action| action.key == normalized)
}

fn print_block(
    runtime: Option<&CliExecutionRuntime>,
    block: OutputBlock,
    style: &CliStyle,
) -> anyhow::Result<()> {
    print_block_on_surface(
        runtime.and_then(|runtime| runtime.terminal_surface.as_deref()),
        block,
        style,
    )
}

fn print_block_on_surface(
    surface: Option<&CliTerminalSurface>,
    block: OutputBlock,
    style: &CliStyle,
) -> anyhow::Result<()> {
    if let Some(surface) = surface {
        surface.print_block(block)?;
    } else {
        print!("{}", render_cli_block_rich(&block, style));
        io::stdout().flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cli_render_retained_layout, cli_should_emit_scheduler_stage_block, CliFrontendPhase,
        CliFrontendProjection, CliObservedExecutionTopology, CliRetainedTranscript,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use rocode_command::cli_style::CliStyle;
    use rocode_command::output_blocks::SchedulerStageBlock;

    fn stage_with_status(status: &str) -> SchedulerStageBlock {
        SchedulerStageBlock {
            profile: Some("prometheus".to_string()),
            stage: "route".to_string(),
            title: "Prometheus · Route".to_string(),
            text: String::new(),
            stage_index: Some(1),
            stage_total: Some(5),
            step: None,
            status: Some(status.to_string()),
            focus: None,
            last_event: None,
            waiting_on: None,
            activity: None,
            available_skill_count: None,
            available_agent_count: None,
            available_category_count: None,
            active_skills: Vec::new(),
            active_agents: Vec::new(),
            active_categories: Vec::new(),
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            decision: None,
        }
    }

    #[test]
    fn cli_prints_scheduler_stage_snapshots_only_on_change() {
        let snapshots = Arc::new(Mutex::new(HashMap::new()));
        let running = stage_with_status("running");
        let done = stage_with_status("done");

        assert!(cli_should_emit_scheduler_stage_block(&snapshots, &running));
        assert!(!cli_should_emit_scheduler_stage_block(&snapshots, &running));
        assert!(cli_should_emit_scheduler_stage_block(&snapshots, &done));
    }

    #[test]
    fn retained_transcript_merges_partial_lines() {
        let mut transcript = CliRetainedTranscript::default();
        transcript.append_rendered("● hello");
        transcript.append_rendered(" world\n");
        transcript.append_rendered("next line\n");

        assert_eq!(transcript.committed_lines, vec!["● hello world", "next line"]);
        assert!(transcript.open_line.is_empty());
    }

    #[test]
    fn retained_layout_emits_session_messages_sidebar_and_active_boxes() {
        let style = CliStyle::plain();
        let mut projection = CliFrontendProjection {
            phase: CliFrontendPhase::Busy,
            active_label: Some("assistant response".to_string()),
            queue_len: 2,
            active_stage: Some(stage_with_status("running")),
            transcript: CliRetainedTranscript::default(),
            sidebar_collapsed: false,
            active_collapsed: false,
            session_title: Some("Test Session".to_string()),
            scroll_offset: 0,
        };
        projection
            .transcript
            .append_rendered("● user prompt\n\n● assistant reply\n");
        let mut topology = CliObservedExecutionTopology::default();
        topology.active = true;

        let lines = cli_render_retained_layout(
            "Preset prometheus",
            "Model auto",
            "~/tests/rust/rocode",
            &projection,
            &topology,
            &style,
        );
        let joined = lines.join("\n");

        assert!(joined.contains("ROCode"));
        assert!(joined.contains("Messages"));
        assert!(joined.contains("Sidebar"));
        assert!(joined.contains("Active"));
        assert!(joined.contains("assistant reply"));
        assert!(joined.contains("Test Session"));
    }

    #[test]
    fn retained_layout_collapses_sidebar() {
        let style = CliStyle::plain();
        let projection = CliFrontendProjection {
            phase: CliFrontendPhase::Idle,
            sidebar_collapsed: true,
            active_collapsed: false,
            session_title: Some("Collapsed Test".to_string()),
            ..Default::default()
        };
        let topology = CliObservedExecutionTopology::default();

        let lines = cli_render_retained_layout(
            "Agent build",
            "Model auto",
            "~/workspace",
            &projection,
            &topology,
            &style,
        );
        let joined = lines.join("\n");

        assert!(joined.contains("ROCode"));
        assert!(joined.contains("Messages"));
        // Sidebar box should NOT appear when collapsed
        assert!(!joined.contains("╭ Sidebar"));
        assert!(joined.contains("Active"));
    }

    #[test]
    fn retained_layout_collapses_active() {
        let style = CliStyle::plain();
        let projection = CliFrontendProjection {
            phase: CliFrontendPhase::Idle,
            sidebar_collapsed: false,
            active_collapsed: true,
            session_title: None,
            ..Default::default()
        };
        let topology = CliObservedExecutionTopology::default();

        let lines = cli_render_retained_layout(
            "Agent build",
            "Model auto",
            "~/workspace",
            &projection,
            &topology,
            &style,
        );
        let joined = lines.join("\n");

        assert!(joined.contains("Sidebar"));
        assert!(joined.contains("Active"));
        assert!(joined.contains("/active to expand"));
    }
}

// ── CLI interactive question handler ─────────────────────────────────

async fn cli_ask_question(
    questions: Vec<rocode_tool::QuestionDef>,
    observed_topology: Arc<Mutex<CliObservedExecutionTopology>>,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    prompt_session_slot: Arc<std::sync::Mutex<Option<Arc<PromptSession>>>>,
    spinner_guard: SpinnerGuard,
) -> Result<Vec<Vec<String>>, rocode_tool::ToolError> {
    // Pause spinner so it doesn't trample the interactive prompt.
    spinner_guard.pause();
    let style = CliStyle::detect();
    let prompt_session = prompt_session_slot
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned());
    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.suspend();
    }
    if let Ok(mut topology) = observed_topology.lock() {
        topology.start_question(questions.len());
    }
    let mut all_answers = Vec::with_capacity(questions.len());

    for q in &questions {
        cli_frontend_set_phase(
            &frontend_projection,
            CliFrontendPhase::Waiting,
            Some(q.header.clone().unwrap_or_else(|| "question".to_string())),
        );
        let options: Vec<SelectOption> = q
            .options
            .iter()
            .map(|opt| SelectOption {
                label: opt.label.clone(),
                description: opt.description.clone(),
            })
            .collect();

        let result = if options.is_empty() {
            // No options — free text input
            prompt_free_text(&q.question, q.header.as_deref(), &style)
        } else if q.multiple {
            interactive_multi_select(&q.question, q.header.as_deref(), &options, &style)
        } else {
            interactive_select(&q.question, q.header.as_deref(), &options, &style)
        };

        match result {
            Ok(SelectResult::Selected(choices)) => {
                all_answers.push(choices);
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Busy,
                    Some("assistant response".to_string()),
                );
            }
            Ok(SelectResult::Other(text)) => {
                all_answers.push(vec![text]);
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Busy,
                    Some("assistant response".to_string()),
                );
            }
            Ok(SelectResult::Cancelled) => {
                if let Ok(mut topology) = observed_topology.lock() {
                    topology.finish_question("cancelled");
                }
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Failed,
                    Some("question cancelled".to_string()),
                );
                if let Some(prompt_session) = prompt_session.as_ref() {
                    let _ = prompt_session.resume();
                }
                spinner_guard.resume();
                return Err(rocode_tool::ToolError::ExecutionError(
                    "User cancelled the question".to_string(),
                ));
            }
            Err(e) => {
                if let Ok(mut topology) = observed_topology.lock() {
                    topology.finish_question("failed");
                }
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Failed,
                    Some("question failed".to_string()),
                );
                if let Some(prompt_session) = prompt_session.as_ref() {
                    let _ = prompt_session.resume();
                }
                spinner_guard.resume();
                return Err(rocode_tool::ToolError::ExecutionError(format!(
                    "Interactive prompt error: {}",
                    e
                )));
            }
        }
    }

    if let Ok(mut topology) = observed_topology.lock() {
        topology.finish_question("answered");
    }
    cli_frontend_set_phase(
        &frontend_projection,
        CliFrontendPhase::Busy,
        Some("assistant response".to_string()),
    );
    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.resume();
    }
    spinner_guard.resume();
    Ok(all_answers)
}

fn prompt_free_text(
    question: &str,
    header: Option<&str>,
    style: &CliStyle,
) -> io::Result<SelectResult> {
    println!();
    if let Some(h) = header {
        println!("  {} {}", style.bold_cyan(style.bullet()), style.bold(h));
    }
    println!("  {}", question);
    print!("  {} ", style.bold_cyan("›"));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_string();

    if answer.is_empty() {
        Ok(SelectResult::Cancelled)
    } else {
        Ok(SelectResult::Other(answer))
    }
}

// ── CLI agent task handlers ──────────────────────────────────────────

fn cli_list_tasks() {
    let tasks = global_task_registry().list();
    if tasks.is_empty() {
        println!("No agent tasks.");
        return;
    }
    let now = chrono::Utc::now().timestamp();
    for task in &tasks {
        let (icon, status_str) = match &task.status {
            AgentTaskStatus::Pending => ("◯", "pending".to_string()),
            AgentTaskStatus::Running { step } => {
                let steps = task
                    .max_steps
                    .map(|m| format!("{}/{}", step, m))
                    .unwrap_or(format!("{}/?", step));
                ("◐", format!("running  {}", steps))
            }
            AgentTaskStatus::Completed { steps } => ("●", format!("done     {}", steps)),
            AgentTaskStatus::Cancelled => ("✗", "cancelled".to_string()),
            AgentTaskStatus::Failed { .. } => ("✗", "failed".to_string()),
        };
        let elapsed = now - task.started_at;
        let elapsed_str = if elapsed < 60 {
            format!("{}s ago", elapsed)
        } else {
            format!("{}m ago", elapsed / 60)
        };
        println!(
            "  {}  {}  {:<20} {:<16} {}",
            icon, task.id, task.agent_name, status_str, elapsed_str
        );
    }
    let running = tasks
        .iter()
        .filter(|t| matches!(t.status, AgentTaskStatus::Running { .. }))
        .count();
    let done = tasks.iter().filter(|t| t.status.is_terminal()).count();
    println!("{} running, {} finished", running, done);
}

fn cli_show_task(id: &str) {
    match global_task_registry().get(id) {
        Some(task) => {
            let (status_label, step_info) = match &task.status {
                AgentTaskStatus::Pending => ("pending".to_string(), String::new()),
                AgentTaskStatus::Running { step } => {
                    let steps = task
                        .max_steps
                        .map(|m| format!(" (step {}/{})", step, m))
                        .unwrap_or(format!(" (step {}/?)", step));
                    ("running".to_string(), steps)
                }
                AgentTaskStatus::Completed { steps } => {
                    ("completed".to_string(), format!(" ({} steps)", steps))
                }
                AgentTaskStatus::Cancelled => ("cancelled".to_string(), String::new()),
                AgentTaskStatus::Failed { error } => (format!("failed: {}", error), String::new()),
            };
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - task.started_at;
            let elapsed_str = if elapsed < 60 {
                format!("{}s ago", elapsed)
            } else {
                format!("{}m ago", elapsed / 60)
            };
            println!("Task {} — {}", task.id, task.agent_name);
            println!("Status: {}{}", status_label, step_info);
            println!("Started: {}", elapsed_str);
            println!("Prompt: {}", task.prompt);
            if !task.output_tail.is_empty() {
                println!("Recent output:");
                for line in &task.output_tail {
                    println!("  {}", line);
                }
            }
        }
        None => {
            println!("Task \"{}\" not found", id);
        }
    }
}

fn cli_kill_task(id: &str) {
    match rocode_orchestrator::global_lifecycle().cancel_task(id) {
        Ok(()) => println!("✓ Task {} cancelled", id),
        Err(err) => eprintln!("{}", err),
    }
}

// ── CLI session listing ─────────────────────────────────────────────

async fn cli_list_sessions() {
    let style = CliStyle::detect();

    let db = match rocode_storage::Database::new().await {
        Ok(db) => db,
        Err(e) => {
            println!(
                "  {} Failed to open session database: {}",
                style.bold_red("✗"),
                e
            );
            return;
        }
    };

    let session_repo = rocode_storage::SessionRepository::new(db.pool().clone());

    let sessions = match session_repo.list(None, 20).await {
        Ok(sessions) => sessions,
        Err(e) => {
            println!("  {} Failed to list sessions: {}", style.bold_red("✗"), e);
            return;
        }
    };

    if sessions.is_empty() {
        println!(
            "\n  {} {}\n",
            style.dim("○"),
            style.dim("No sessions found.")
        );
        return;
    }

    println!(
        "\n  {} {}\n",
        style.bold_cyan(style.bullet()),
        style.bold("Recent Sessions")
    );

    for session in &sessions {
        let title = if session.title.is_empty() {
            "(untitled)"
        } else {
            &session.title
        };
        let id_short = if session.id.len() > 8 {
            &session.id[..8]
        } else {
            &session.id
        };
        let time_str = format_session_time(session.time.updated);

        println!(
            "    {} {} {}",
            style.dim(id_short),
            title,
            style.dim(&time_str),
        );
    }
    println!();
    println!(
        "  {} Use {} to continue a previous session at startup.",
        style.dim("tip:"),
        style.bold("--continue"),
    );
    println!();
}

fn format_session_time(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let elapsed = now - timestamp;
    if elapsed < 0 {
        return "just now".to_string();
    }
    if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
    }
}
