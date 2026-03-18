use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rocode_agent::{AgentInfo, AgentRegistry};
use rocode_command::cli_panel::CliPanelFrame;
#[cfg(test)]
use rocode_command::cli_panel::{
    display_width, pad_right_display, truncate_display, wrap_display_text,
};
use rocode_command::cli_permission::{prompt_permission, PermissionDecision, PermissionMemory};
use rocode_command::cli_prompt::{
    PromptCompletion, PromptFrame, PromptSession, PromptSessionEvent,
};
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
use rocode_command::{CommandRegistry, ResolvedUiCommand, UiActionId};
use rocode_config::loader::load_config;
use rocode_config::Config;
use rocode_core::agent_task_registry::{global_task_registry, AgentTaskStatus};
#[cfg(test)]
use rocode_core::contracts::mcp::McpConnectionStatusWire;
use rocode_core::contracts::output_blocks::ToolPhaseWire;
use rocode_core::contracts::permission::{keys as permission_keys, PermissionReplyWire};
use rocode_core::contracts::scheduler::keys as scheduler_keys;
use rocode_core::contracts::scheduler::{SchedulerStageStatus, SchedulerStageWaitingOn};
use rocode_core::contracts::session::keys as session_keys;
use rocode_core::contracts::tools::{QuestionInteractionStatus, ToolCallStatusWire};
use rocode_core::contracts::wire::keys as wire_keys;
use rocode_orchestrator::{
    scheduler_plan_from_profile, scheduler_request_defaults_from_plan, SchedulerConfig,
    SchedulerPresetKind, SchedulerProfileConfig, SchedulerRequestDefaults,
};
use rocode_provider::ProviderRegistry;
use rocode_util::util::color::strip_ansi;
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::sync::CancellationToken;

use crate::api_client::{CliApiClient, McpStatusInfo, MessageTokensInfo, SessionInfo};
use crate::cli::RunOutputFormat;
use crate::event_stream::{self, CliServerEvent};
use crate::providers::{render_help, setup_providers};
use crate::remote::{parse_output_block, run_non_interactive_attach, RemoteAttachOptions};
use crate::server_lifecycle::discover_or_start_server;
use crate::util::{
    append_cli_file_attachments, collect_run_input, parse_model_and_provider, truncate_text,
};
use rocode_command::branding::logo_lines;
use rocode_tui::branding::{APP_SHORT_NAME, APP_TAGLINE, APP_VERSION_DATE};
use rocode_tui::ui::Clipboard;

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

fn cli_resolve_show_thinking(
    explicit_flag: bool,
    _config: Option<&Config>,
    fallback: bool,
) -> bool {
    if explicit_flag {
        return true;
    }

    fallback
}
pub(crate) async fn run_non_interactive(options: RunNonInteractiveOptions) -> anyhow::Result<()> {
    let RunNonInteractiveOptions {
        message,
        command,
        continue_last,
        session,
        fork,
        share,
        model,
        requested_agent,
        requested_scheduler_profile,
        files,
        format,
        title,
        attach,
        dir,
        port: _port,
        variant,
        thinking,
    } = options;

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
    if input.trim().is_empty() {
        let (provider, model_id) = parse_model_and_provider(model);
        return run_chat_session(
            model_id,
            provider,
            requested_agent,
            requested_scheduler_profile,
            thinking,
        )
        .await;
    }

    let base_url = if let Some(base_url) = attach {
        base_url
    } else {
        discover_or_start_server(None).await?
    };
    let api_client = CliApiClient::new(base_url.clone());
    let remote_config = api_client.get_config().await.ok();
    let show_thinking = cli_resolve_show_thinking(thinking, remote_config.as_ref(), true);

    run_non_interactive_attach(RemoteAttachOptions {
        base_url,
        input,
        command,
        continue_last,
        session,
        fork,
        share,
        model,
        agent: requested_agent,
        scheduler_profile: requested_scheduler_profile,
        variant,
        format,
        title,
        show_thinking,
    })
    .await
}

pub(crate) struct RunNonInteractiveOptions {
    pub message: Vec<String>,
    pub command: Option<String>,
    pub continue_last: bool,
    pub session: Option<String>,
    pub fork: bool,
    pub share: bool,
    pub model: Option<String>,
    pub requested_agent: Option<String>,
    pub requested_scheduler_profile: Option<String>,
    pub files: Vec<PathBuf>,
    pub format: RunOutputFormat,
    pub title: Option<String>,
    pub attach: Option<String>,
    pub dir: Option<PathBuf>,
    pub port: Option<u16>,
    pub variant: Option<String>,
    pub thinking: bool,
}

#[derive(Debug, Clone, Default)]
struct CliRunSelection {
    model: Option<String>,
    provider: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
    show_thinking: bool,
}

struct CliExecutionRuntime {
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
    /// Shared spinner guard — updated each message cycle so that question/permission
    /// callbacks can pause the active spinner without holding a stale reference.
    spinner_guard: Arc<std::sync::Mutex<SpinnerGuard>>,
    /// HTTP client for communicating with the server (Phase 3 unification).
    api_client: Option<Arc<CliApiClient>>,
    /// Server-side session ID (created via HTTP POST /session).
    server_session_id: Option<String>,
    /// Root session plus any explicitly attached child sessions for the active execution tree.
    related_session_ids: Arc<Mutex<BTreeSet<String>>>,
    /// Canonical retained transcript for the root session even when the operator
    /// temporarily focuses a child session view.
    root_session_transcript: Arc<Mutex<CliRetainedTranscript>>,
    /// Background transcripts for non-root child sessions. These are populated
    /// from the unified event surface but not rendered into the main transcript
    /// until the operator explicitly focuses one.
    child_session_transcripts: Arc<Mutex<HashMap<String, CliRetainedTranscript>>>,
    /// Local CLI-only focus target. `None` means the root session remains visible.
    focused_session_id: Arc<Mutex<Option<String>>>,
    permission_memory: Arc<AsyncMutex<PermissionMemory>>,
    show_thinking: Arc<AtomicBool>,
}

struct CliRuntimeBuildInput<'a> {
    config: &'a Config,
    agent_registry: Arc<AgentRegistry>,
    selection: &'a CliRunSelection,
}

#[derive(Clone)]
struct CliInteractiveHandles {
    terminal_surface: Arc<CliTerminalSurface>,
    prompt_chrome: Arc<CliPromptChrome>,
    prompt_session: Arc<PromptSession>,
    queued_inputs: Arc<AsyncMutex<VecDeque<String>>>,
    busy_flag: Arc<AtomicBool>,
    exit_requested: Arc<AtomicBool>,
    active_abort: Arc<AsyncMutex<Option<CliActiveAbortHandle>>>,
}

enum CliUiActionOutcome {
    Continue,
    Break,
}

fn cli_resolve_registry_ui_action(
    registry: &CommandRegistry,
    input: &str,
) -> Option<ResolvedUiCommand> {
    registry.resolve_ui_slash_input(input)
}

async fn cli_prompt_action_text(
    runtime: &CliExecutionRuntime,
    header: Option<&str>,
    question: &str,
) -> anyhow::Result<Option<String>> {
    let prompt_session = runtime
        .prompt_session_slot
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned());
    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.suspend();
    }

    {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(
            stdout,
            crossterm::cursor::Show,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
        );
        let _ = stdout.flush();
    }

    let header = header.map(str::to_string);
    let question = question.to_string();
    let style = CliStyle::detect();
    let result =
        tokio::task::spawn_blocking(move || prompt_free_text(&question, header.as_deref(), &style))
            .await
            .map_err(|error| anyhow::anyhow!("prompt task failed: {}", error))?;

    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.resume();
    }

    match result {
        Ok(SelectResult::Other(text)) => Ok(Some(text)),
        Ok(SelectResult::Cancelled) | Ok(SelectResult::Selected(_)) => Ok(None),
        Err(error) => Err(anyhow::anyhow!("prompt failed: {}", error)),
    }
}

async fn cli_execute_ui_action(
    action_id: UiActionId,
    argument: Option<&str>,
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    provider_registry: &ProviderRegistry,
    agent_registry: &AgentRegistry,
    current_dir: &Path,
    repl_style: &CliStyle,
) -> anyhow::Result<CliUiActionOutcome> {
    match action_id {
        UiActionId::AbortExecution => {
            let handle = { runtime.active_abort.lock().await.clone() };
            let Some(handle) = handle else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active run to abort. Use /abort while a response is running.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            if cli_trigger_abort(handle).await {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("Cancellation requested.")),
                    repl_style,
                );
            } else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::error("Failed to request cancellation.")),
                    repl_style,
                );
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::Exit => Ok(CliUiActionOutcome::Break),
        UiActionId::ShowHelp => {
            let style = CliStyle::detect();
            let rendered = render_help(&style);
            if let Some(surface) = runtime.terminal_surface.as_ref() {
                let _ = surface.print_text(&rendered);
            } else {
                print!("{}", rendered);
                let _ = io::stdout().flush();
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenRecoveryList => {
            cli_print_recovery_actions(runtime);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::NewSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_new_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::RenameSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to rename.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            let Some(next_title) = cli_prompt_action_text(
                runtime,
                Some("rename session"),
                "Enter a new title for the current session:",
            )
            .await?
            else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("Session rename cancelled.")),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client
                .update_session_title(&session_id, next_title.trim())
                .await
            {
                Ok(updated) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Session renamed: {}",
                            updated.title
                        ))),
                        repl_style,
                    );
                    cli_refresh_server_info(
                        api_client,
                        &runtime.frontend_projection,
                        Some(&session_id),
                    )
                    .await;
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to rename session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ShareSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("No active server session to share.")),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.share_session(&session_id).await {
                Ok(shared) => {
                    let label = if shared.url.trim().is_empty() {
                        "Session shared.".to_string()
                    } else {
                        format!("Share link: {}", shared.url)
                    };
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(label)),
                        repl_style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to share session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::UnshareSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to unshare.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.unshare_session(&session_id).await {
                Ok(_) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title("Session unshared.")),
                        repl_style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to unshare session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ForkSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_fork_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::CompactSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_compact_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ShowStatus => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();

            cli_refresh_server_info(
                api_client,
                &runtime.frontend_projection,
                runtime.server_session_id.as_deref(),
            )
            .await;

            let (phase, active_label, queue_len, token_stats, mcp_servers, lsp_servers) = runtime
                .frontend_projection
                .lock()
                .map(|projection| {
                    (
                        projection.phase.as_str().to_string(),
                        projection.active_label.clone(),
                        projection.queue_len,
                        projection.token_stats.clone(),
                        projection.mcp_servers.clone(),
                        projection.lsp_servers.clone(),
                    )
                })
                .unwrap_or_else(|_| {
                    (
                        "unknown".to_string(),
                        None,
                        0,
                        CliSessionTokenStats::default(),
                        Vec::new(),
                        Vec::new(),
                    )
                });
            let mut lines = vec![
                format!("Agent: {}", runtime.resolved_agent_name),
                format!("Model: {}", runtime.resolved_model_label),
                format!("Directory: {}", current_dir.display()),
                format!("Runtime: {}", phase),
            ];
            if let Some(ref profile) = runtime.resolved_scheduler_profile_name {
                lines.push(format!("Scheduler: {}", profile));
            }
            if let Some(active_label) = active_label.filter(|value| !value.trim().is_empty()) {
                lines.push(format!("Active: {}", active_label));
            }
            lines.push(format!("Queue: {}", queue_len));

            if token_stats.total_tokens > 0 {
                lines.push(String::new());
                lines.push(format!(
                    "Tokens: {} total",
                    format_token_count(token_stats.total_tokens)
                ));
                lines.push(format!(
                    "  Input:     {}",
                    format_token_count(token_stats.input_tokens)
                ));
                lines.push(format!(
                    "  Output:    {}",
                    format_token_count(token_stats.output_tokens)
                ));
                if token_stats.reasoning_tokens > 0 {
                    lines.push(format!(
                        "  Reasoning: {}",
                        format_token_count(token_stats.reasoning_tokens)
                    ));
                }
                if token_stats.cache_read_tokens > 0 {
                    lines.push(format!(
                        "  Cache R:   {}",
                        format_token_count(token_stats.cache_read_tokens)
                    ));
                }
                if token_stats.cache_write_tokens > 0 {
                    lines.push(format!(
                        "  Cache W:   {}",
                        format_token_count(token_stats.cache_write_tokens)
                    ));
                }
                lines.push(format!("Cost: ${:.4}", token_stats.total_cost));
            }

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

            let _ =
                print_cli_list_on_surface(Some(runtime), "Session Status", None, &lines, &style);
            cli_print_execution_topology(&runtime.observed_topology, Some(runtime), &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenModelList => {
            if let Some(model_ref) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let mut exists = false;
                for provider in provider_registry.list() {
                    for model in provider.models() {
                        if format!("{}/{}", provider.id(), model.id) == model_ref {
                            exists = true;
                            break;
                        }
                    }
                    if exists {
                        break;
                    }
                }
                if exists {
                    runtime.resolved_model_label = model_ref.to_string();
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Model set to {}",
                            model_ref
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown model: {}",
                            model_ref
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            let mut lines = Vec::new();
            for p in provider_registry.list() {
                for m in p.models() {
                    lines.push(format!("{}:{}", p.id(), m.id));
                }
            }
            let _ =
                print_cli_list_on_surface(Some(runtime), "Available Models", None, &lines, &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenModeList => {
            if let Some(mode_ref) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                match api_client.list_execution_modes().await {
                    Ok(modes) => {
                        let normalized = mode_ref.to_ascii_lowercase();
                        let found = modes.into_iter().find(|mode| {
                            let key = format!("{}:{}", mode.kind, mode.id).to_ascii_lowercase();
                            key == normalized
                                || mode.id.to_ascii_lowercase() == normalized
                                || mode.name.to_ascii_lowercase() == normalized
                                || format!("{}:{}", mode.kind, mode.name).to_ascii_lowercase()
                                    == normalized
                        });
                        if let Some(mode) = found {
                            runtime.resolved_scheduler_profile_name = match mode.kind.as_str() {
                                "preset" | "profile" => Some(mode.id.clone()),
                                _ => None,
                            };
                            runtime.resolved_agent_name = if mode.kind == "agent" {
                                mode.id.clone()
                            } else {
                                runtime.resolved_agent_name.clone()
                            };
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Mode set to {}:{}",
                                    mode.kind, mode.id
                                ))),
                                repl_style,
                            );
                        } else {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::warning(format!(
                                    "Unknown mode: {}",
                                    mode_ref
                                ))),
                                repl_style,
                            );
                        }
                    }
                    Err(error) => {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::error(format!(
                                "Failed to load modes: {}",
                                error
                            ))),
                            repl_style,
                        );
                    }
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            match api_client.list_execution_modes().await {
                Ok(modes) => {
                    let lines = modes
                        .into_iter()
                        .filter(|mode| !mode.hidden.unwrap_or(false))
                        .map(|mode| {
                            let detail = mode
                                .description
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| mode.kind.clone());
                            format!("{} [{}] — {}", mode.id, mode.kind, detail)
                        })
                        .collect::<Vec<_>>();
                    let _ = print_cli_list_on_surface(
                        Some(runtime),
                        "Available Modes",
                        None,
                        &lines,
                        &style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to load modes: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ConnectProvider => {
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
            let _ = print_cli_list_on_surface(
                Some(runtime),
                "Configured Providers",
                None,
                &lines,
                &style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenThemeList => {
            if argument.is_some() {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "Theme switching is not yet supported in CLI mode.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            }
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning(
                    "Theme switching is not yet supported in CLI mode.",
                )),
                repl_style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenAgentList => {
            if let Some(agent_name) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let agents = agent_registry.list();
                let found = agents.iter().find(|info| info.name == agent_name);
                if let Some(info) = found {
                    runtime.resolved_agent_name = info.name.clone();
                    runtime.resolved_scheduler_profile_name = None;
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Agent set to {}",
                            info.name
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown agent: {}",
                            agent_name
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            let mut lines = Vec::new();
            for info in agent_registry.list() {
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
            let _ =
                print_cli_list_on_surface(Some(runtime), "Available Agents", None, &lines, &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenPresetList => {
            if let Some(preset_name) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let presets = cli_available_presets(&load_config(current_dir)?);
                if presets.iter().any(|preset| preset == preset_name) {
                    runtime.resolved_scheduler_profile_name = Some(preset_name.to_string());
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Preset set to {}",
                            preset_name
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown preset: {}",
                            preset_name
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_list_presets(
                &load_config(current_dir)?,
                runtime.resolved_scheduler_profile_name.as_deref(),
                Some(runtime),
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenSessionList => {
            if let Some(target) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                match target {
                    "list" => {}
                    "new" => {
                        cli_execute_new_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    "fork" => {
                        cli_execute_fork_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    "compact" => {
                        cli_execute_compact_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    _ => match api_client.list_sessions(Some(target), Some(20)).await {
                        Ok(sessions) => {
                            if let Some(session) = sessions.into_iter().find(|session| {
                                session.id == target
                                    || session.id.starts_with(target)
                                    || session
                                        .title
                                        .to_ascii_lowercase()
                                        .contains(&target.to_ascii_lowercase())
                            }) {
                                cli_set_root_server_session(runtime, session.id.clone());
                                let _ = print_block(
                                    Some(runtime),
                                    OutputBlock::Status(StatusBlock::title(format!(
                                        "Session switched: {}",
                                        session.id
                                    ))),
                                    repl_style,
                                );
                                cli_refresh_server_info(
                                    api_client,
                                    &runtime.frontend_projection,
                                    Some(&session.id),
                                )
                                .await;
                                return Ok(CliUiActionOutcome::Continue);
                            }
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::warning(format!(
                                    "Session not found: {}",
                                    target
                                ))),
                                repl_style,
                            );
                            return Ok(CliUiActionOutcome::Continue);
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to load sessions: {}",
                                    error
                                ))),
                                repl_style,
                            );
                            return Ok(CliUiActionOutcome::Continue);
                        }
                    },
                }
            }
            cli_list_sessions(Some(runtime)).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::NavigateParentSession => {
            let Some(current_session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to navigate from.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.get_session(&current_session_id).await {
                Ok(session) => {
                    let Some(parent_id) = session.parent_id else {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::warning(
                                "Current session has no parent.",
                            )),
                            repl_style,
                        );
                        return Ok(CliUiActionOutcome::Continue);
                    };

                    match api_client.get_session(&parent_id).await {
                        Ok(parent) => {
                            cli_set_root_server_session(runtime, parent.id.clone());
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Switched to parent session: {}",
                                    &parent.id[..parent.id.len().min(8)]
                                ))),
                                repl_style,
                            );
                            cli_refresh_server_info(
                                api_client,
                                &runtime.frontend_projection,
                                Some(&parent.id),
                            )
                            .await;
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to load parent session: {}",
                                    error
                                ))),
                                repl_style,
                            );
                        }
                    }
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to load current session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ListTasks => {
            cli_list_tasks(Some(runtime));
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::CopySession => {
            match cli_copy_target_transcript(runtime).filter(|text| !text.trim().is_empty()) {
                Some(text) => match Clipboard::write_text(&text) {
                    Ok(()) => {
                        let label = if cli_focused_session_id(runtime).is_some() {
                            "Focused session transcript copied to clipboard."
                        } else {
                            "Session transcript copied to clipboard."
                        };
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::title(label)),
                            repl_style,
                        );
                    }
                    Err(error) => {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::error(format!(
                                "Failed to copy transcript to clipboard: {}",
                                error
                            ))),
                            repl_style,
                        );
                    }
                },
                None => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "No transcript available for the current session view.",
                        )),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ToggleSidebar => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning(
                    "CLI mode no longer keeps a persistent sidebar; use terminal scrollback and /status.",
                )),
                repl_style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        _ => Ok(CliUiActionOutcome::Continue),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CliRecentSessionInfo {
    title: Option<String>,
    model_label: Option<String>,
    preset_label: Option<String>,
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

const CLI_NODE_KIND_PROMPT: &str = "prompt";
const CLI_NODE_KIND_SCHEDULER: &str = "scheduler";
const CLI_NODE_KIND_STAGE: &str = "stage";
const CLI_NODE_KIND_TOOL: &str = "tool";
const CLI_NODE_KIND_QUESTION: &str = "question";

const CLI_NODE_ID_PROMPT: &str = "prompt";
const CLI_NODE_ID_SCHEDULER: &str = "scheduler";

#[derive(Clone)]
enum CliActiveAbortHandle {
    /// Server-side execution — abort via HTTP POST.
    Server {
        api_client: Arc<CliApiClient>,
        session_id: String,
    },
}

const CLI_PROMPT_SUGGESTION_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliPromptValueKind {
    Model,
    Agent,
    Preset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CliPromptCommandSpec {
    name: &'static str,
    takes_value: Option<CliPromptValueKind>,
    description: &'static str,
}

const CLI_PROMPT_COMMANDS: &[CliPromptCommandSpec] = &[
    CliPromptCommandSpec {
        name: "help",
        takes_value: None,
        description: "show help",
    },
    CliPromptCommandSpec {
        name: "abort",
        takes_value: None,
        description: "cancel active run",
    },
    CliPromptCommandSpec {
        name: "clear",
        takes_value: None,
        description: "clear screen",
    },
    CliPromptCommandSpec {
        name: "recover",
        takes_value: None,
        description: "list recovery actions",
    },
    CliPromptCommandSpec {
        name: "model",
        takes_value: Some(CliPromptValueKind::Model),
        description: "switch model",
    },
    CliPromptCommandSpec {
        name: "models",
        takes_value: None,
        description: "list models",
    },
    CliPromptCommandSpec {
        name: "agent",
        takes_value: Some(CliPromptValueKind::Agent),
        description: "switch agent",
    },
    CliPromptCommandSpec {
        name: "agents",
        takes_value: None,
        description: "list agents",
    },
    CliPromptCommandSpec {
        name: "preset",
        takes_value: Some(CliPromptValueKind::Preset),
        description: "switch preset",
    },
    CliPromptCommandSpec {
        name: "presets",
        takes_value: None,
        description: "list presets",
    },
    CliPromptCommandSpec {
        name: "providers",
        takes_value: None,
        description: "list providers",
    },
    CliPromptCommandSpec {
        name: "sessions",
        takes_value: None,
        description: "list sessions",
    },
    CliPromptCommandSpec {
        name: "parent",
        takes_value: None,
        description: "return to parent session",
    },
    CliPromptCommandSpec {
        name: "child",
        takes_value: None,
        description: "list or focus child sessions",
    },
    CliPromptCommandSpec {
        name: "tasks",
        takes_value: None,
        description: "list agent tasks",
    },
    CliPromptCommandSpec {
        name: "compact",
        takes_value: None,
        description: "compact conversation history",
    },
    CliPromptCommandSpec {
        name: "copy",
        takes_value: None,
        description: "copy last reply",
    },
];

#[derive(Debug, Clone, Default)]
struct CliPromptCatalog {
    models: Vec<String>,
    agents: Vec<String>,
    presets: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct CliPromptSelectionState {
    model: String,
    agent: String,
    preset: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CliPromptAssistView {
    screen_lines: Vec<String>,
    completion: Option<PromptCompletion>,
}

#[derive(Debug)]
struct CliPromptChrome {
    mode_label: Mutex<String>,
    model_label: Mutex<String>,
    selection: Mutex<CliPromptSelectionState>,
    catalog: CliPromptCatalog,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    style: CliStyle,
}

impl CliPromptChrome {
    fn new(
        runtime: &CliExecutionRuntime,
        style: &CliStyle,
        _current_dir: &Path,
        config: &Config,
        provider_registry: &ProviderRegistry,
        agent_registry: &AgentRegistry,
    ) -> Self {
        Self {
            mode_label: Mutex::new(cli_mode_label(runtime)),
            model_label: Mutex::new(format!("Model {}", runtime.resolved_model_label)),
            selection: Mutex::new(CliPromptSelectionState {
                model: runtime.resolved_model_label.clone(),
                agent: runtime.resolved_agent_name.clone(),
                preset: runtime.resolved_scheduler_profile_name.clone(),
            }),
            catalog: CliPromptCatalog {
                models: cli_prompt_models(provider_registry),
                agents: cli_prompt_agents(agent_registry),
                presets: cli_available_presets(config),
            },
            frontend_projection: runtime.frontend_projection.clone(),
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
        if let Ok(mut selection) = self.selection.lock() {
            selection.model = runtime.resolved_model_label.clone();
            selection.agent = runtime.resolved_agent_name.clone();
            selection.preset = runtime.resolved_scheduler_profile_name.clone();
        }
    }

    fn assist(&self, line: &str, cursor_pos: usize) -> CliPromptAssistView {
        let selection = self
            .selection
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();
        cli_prompt_assist_view(&self.catalog, &selection, line, cursor_pos)
    }

    fn frame(&self, line: &str, cursor_pos: usize) -> PromptFrame {
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
            .unwrap_or_else(|_| {
                " Ready  •  Alt+Enter/Ctrl+J newline  •  /help  •  Ctrl+D exit ".to_string()
            });
        let assist = self.assist(line, cursor_pos);
        let mut screen_lines = cli_prompt_screen_lines();
        screen_lines.extend(assist.screen_lines);
        PromptFrame::boxed_with_footer(&mode, &model, &footer, &self.style)
            .with_screen_lines(screen_lines)
    }
}

fn cli_prompt_models(provider_registry: &ProviderRegistry) -> Vec<String> {
    let mut models = provider_registry
        .list()
        .into_iter()
        .flat_map(|provider| {
            let provider_id = provider.id().to_string();
            provider
                .models()
                .into_iter()
                .map(move |model| format!("{}/{}", provider_id, model.id))
        })
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

fn cli_prompt_agents(agent_registry: &AgentRegistry) -> Vec<String> {
    agent_registry
        .list()
        .into_iter()
        .map(|agent| agent.name.clone())
        .collect()
}

fn cli_prompt_assist_view(
    catalog: &CliPromptCatalog,
    selection: &CliPromptSelectionState,
    line: &str,
    cursor_pos: usize,
) -> CliPromptAssistView {
    let prefix = cli_prompt_prefix(line, cursor_pos);
    let trimmed = prefix.trim_start();
    if !trimmed.starts_with('/') {
        return CliPromptAssistView::default();
    }

    let body = &trimmed[1..];
    let body = body.trim_start();
    if body.is_empty() {
        return cli_prompt_command_assist("");
    }

    let Some((command_token, remainder)) = cli_prompt_split_command(body) else {
        return CliPromptAssistView::default();
    };
    let command_name = command_token.to_ascii_lowercase();

    if remainder.is_none() {
        if let Some(spec) = cli_prompt_command_spec(&command_name) {
            if spec.takes_value.is_some() && !prefix.ends_with(' ') {
                return cli_prompt_value_assist(spec, "", catalog, selection, false);
            }
        }
        return cli_prompt_command_assist(&command_name);
    }

    let Some(spec) = cli_prompt_command_spec(&command_name) else {
        return CliPromptAssistView::default();
    };
    let Some(value_kind) = spec.takes_value else {
        return CliPromptAssistView::default();
    };
    let query = remainder.unwrap_or("").trim();
    cli_prompt_value_assist(
        CliPromptCommandSpec {
            name: spec.name,
            takes_value: Some(value_kind),
            description: spec.description,
        },
        query,
        catalog,
        selection,
        true,
    )
}

fn cli_prompt_prefix(line: &str, cursor_pos: usize) -> String {
    line.chars().take(cursor_pos).collect()
}

fn cli_prompt_split_command(body: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = body.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    for (idx, ch) in trimmed.char_indices() {
        if ch.is_whitespace() {
            return Some((&trimmed[..idx], Some(trimmed[idx..].trim_start())));
        }
    }

    Some((trimmed, None))
}

fn cli_prompt_command_spec(name: &str) -> Option<CliPromptCommandSpec> {
    CLI_PROMPT_COMMANDS
        .iter()
        .copied()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

fn cli_prompt_command_assist(query: &str) -> CliPromptAssistView {
    let matches =
        cli_prompt_ranked_matches(CLI_PROMPT_COMMANDS.iter().map(|spec| spec.name), query);
    if matches.is_empty() {
        return CliPromptAssistView::default();
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "Commands ({} match{})",
        matches.len(),
        if matches.len() == 1 { "" } else { "es" }
    ));

    for name in matches.iter().take(CLI_PROMPT_SUGGESTION_LIMIT) {
        let spec = cli_prompt_command_spec(name).expect("command spec");
        lines.push(format!("  /{:<10} {}", spec.name, spec.description));
    }
    if matches.len() > CLI_PROMPT_SUGGESTION_LIMIT {
        lines.push(format!(
            "  ... {} more",
            matches.len() - CLI_PROMPT_SUGGESTION_LIMIT
        ));
    }

    let completion = matches.first().and_then(|name| {
        cli_prompt_command_spec(name).map(|spec| PromptCompletion {
            line: if spec.takes_value.is_some() {
                format!("/{} ", spec.name)
            } else {
                format!("/{}", spec.name)
            },
            cursor_pos: if spec.takes_value.is_some() {
                spec.name.len() + 2
            } else {
                spec.name.len() + 1
            },
        })
    });

    CliPromptAssistView {
        screen_lines: lines,
        completion,
    }
}

fn cli_prompt_value_assist(
    spec: CliPromptCommandSpec,
    query: &str,
    catalog: &CliPromptCatalog,
    selection: &CliPromptSelectionState,
    can_complete_value: bool,
) -> CliPromptAssistView {
    let values = match spec.takes_value {
        Some(CliPromptValueKind::Model) => &catalog.models,
        Some(CliPromptValueKind::Agent) => &catalog.agents,
        Some(CliPromptValueKind::Preset) => &catalog.presets,
        None => return CliPromptAssistView::default(),
    };
    let matches = cli_prompt_ranked_matches(values.iter().map(String::as_str), query);
    if matches.is_empty() {
        return CliPromptAssistView::default();
    }

    let active = match spec.takes_value {
        Some(CliPromptValueKind::Model) => Some(selection.model.as_str()),
        Some(CliPromptValueKind::Agent) => Some(selection.agent.as_str()),
        Some(CliPromptValueKind::Preset) => selection.preset.as_deref(),
        None => None,
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "/{} suggestions ({} match{})",
        spec.name,
        matches.len(),
        if matches.len() == 1 { "" } else { "es" }
    ));
    for value in matches.iter().take(CLI_PROMPT_SUGGESTION_LIMIT) {
        let active_suffix = if active.is_some_and(|current| current.eq_ignore_ascii_case(value)) {
            " [active]"
        } else {
            ""
        };
        lines.push(format!("  {}{}", value, active_suffix));
    }
    if matches.len() > CLI_PROMPT_SUGGESTION_LIMIT {
        lines.push(format!(
            "  ... {} more",
            matches.len() - CLI_PROMPT_SUGGESTION_LIMIT
        ));
    }
    lines.push("  Tab completes best match".to_string());

    let completion = if can_complete_value {
        matches.first().map(|value| PromptCompletion {
            line: format!("/{} {}", spec.name, value),
            cursor_pos: spec.name.len() + value.len() + 2,
        })
    } else {
        Some(PromptCompletion {
            line: format!("/{} ", spec.name),
            cursor_pos: spec.name.len() + 2,
        })
    };

    CliPromptAssistView {
        screen_lines: lines,
        completion,
    }
}

fn cli_prompt_ranked_matches<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Vec<String> {
    let normalized_query = query.trim().to_ascii_lowercase();
    let mut prefix_matches = Vec::new();
    let mut contains_matches = Vec::new();

    for candidate in candidates {
        let normalized_candidate = candidate.to_ascii_lowercase();
        if normalized_query.is_empty() || normalized_candidate.starts_with(&normalized_query) {
            prefix_matches.push(candidate.to_string());
        } else if normalized_candidate.contains(&normalized_query) {
            contains_matches.push(candidate.to_string());
        }
    }

    prefix_matches.sort();
    prefix_matches.dedup();
    contains_matches.sort();
    contains_matches.retain(|item| !prefix_matches.contains(item));
    prefix_matches.extend(contains_matches);
    prefix_matches
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

    fn replace_transcript(&self, transcript: CliRetainedTranscript) -> io::Result<()> {
        if let Ok(mut projection) = self.frontend_projection.lock() {
            projection.transcript = transcript.clone();
            projection.scroll_offset = 0;
        }

        let prompt = self
            .prompt_session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().cloned());

        if let Some(prompt_session) = prompt {
            let _ = prompt_session.suspend();
            let write_result: io::Result<()> = {
                print!("\x1B[2J\x1B[1;1H{}", transcript.rendered_text());
                io::stdout().flush()
            };
            let _ = prompt_session.resume();
            write_result?;
        } else {
            print!("\x1B[2J\x1B[1;1H{}", transcript.rendered_text());
            io::stdout().flush()?;
        }

        self.refresh_prompt()
    }

    fn append_rendered(&self, rendered: &str) -> io::Result<()> {
        if let Ok(mut projection) = self.frontend_projection.lock() {
            projection.transcript.append_rendered(rendered);
            projection.scroll_offset = 0;
        }
        let prompt = self
            .prompt_session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().cloned());

        if let Some(prompt_session) = prompt {
            let _ = prompt_session.suspend();
            let write_result: io::Result<()> = {
                print!("{}", rendered);
                io::stdout().flush()
            };
            let _ = prompt_session.resume();
            write_result
        } else {
            print!("{}", rendered);
            io::stdout().flush()
        }
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

impl CliFrontendPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Failed => "failed",
        }
    }
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
                self.committed_lines
                    .push(std::mem::take(&mut self.open_line));
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

    fn rendered_text(&self) -> String {
        let mut out = String::new();
        for line in &self.committed_lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.open_line);
        out
    }

    fn line_count(&self) -> usize {
        self.committed_lines.len() + usize::from(!self.open_line.is_empty())
    }

    fn last_line(&self) -> Option<&str> {
        if !self.open_line.is_empty() {
            Some(self.open_line.as_str())
        } else {
            self.committed_lines.last().map(String::as_str)
        }
    }

    #[cfg(test)]
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
    #[cfg(test)]
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
    view_label: Option<String>,
    queue_len: usize,
    active_stage: Option<SchedulerStageBlock>,
    transcript: CliRetainedTranscript,
    #[cfg_attr(not(test), allow(dead_code))]
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
            view_label: None,
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
        if let Some(active) = self
            .active_label
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            parts.push(active.to_string());
        }
        if let Some(view) = self.view_label.as_deref().filter(|value| !value.is_empty()) {
            parts.push(view.to_string());
        }
        if self.queue_len > 0 {
            parts.push(format!("queue {}", self.queue_len));
        }
        parts.push("Alt+Enter/Ctrl+J newline".to_string());
        parts.push("/help".to_string());
        parts.push("/child".to_string());
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
        self.root_id = Some(CLI_NODE_ID_PROMPT.to_string());
        self.scheduler_id = scheduler_profile.map(|_| CLI_NODE_ID_SCHEDULER.to_string());
        self.active_stage_id = None;
        self.stage_order.clear();
        self.nodes.clear();
        self.nodes.insert(
            CLI_NODE_ID_PROMPT.to_string(),
            CliObservedExecutionNode {
                kind: CLI_NODE_KIND_PROMPT.to_string(),
                label: format!("Prompt run ({})", agent_name),
                status: SchedulerStageStatus::Running.as_str().to_string(),
                waiting_on: Some(SchedulerStageWaitingOn::Model.as_str().to_string()),
                recent_event: Some("Prompt run started".to_string()),
                children: Vec::new(),
            },
        );
        if let Some(profile) = scheduler_profile {
            self.nodes.insert(
                CLI_NODE_ID_SCHEDULER.to_string(),
                CliObservedExecutionNode {
                    kind: CLI_NODE_KIND_SCHEDULER.to_string(),
                    label: format!("Scheduler run ({})", profile),
                    status: SchedulerStageStatus::Running.as_str().to_string(),
                    waiting_on: Some(SchedulerStageWaitingOn::Model.as_str().to_string()),
                    recent_event: Some("Scheduler orchestration started".to_string()),
                    children: Vec::new(),
                },
            );
            self.attach_child(CLI_NODE_ID_PROMPT, CLI_NODE_ID_SCHEDULER);
        }
    }

    fn observe_block(&mut self, block: &OutputBlock) {
        match block {
            OutputBlock::SchedulerStage(stage) => self.observe_scheduler_stage(stage),
            OutputBlock::Tool(tool) => self.observe_tool(tool),
            _ => {}
        }
    }

    fn observe_scheduler_stage(
        &mut self,
        stage: &rocode_command::output_blocks::SchedulerStageBlock,
    ) {
        let stage_id = stage.stage_id.clone().unwrap_or_else(|| {
            format!(
                "stage:{}:{}",
                stage
                    .stage_index
                    .unwrap_or((self.stage_order.len() + 1) as u64),
                stage.stage
            )
        });
        let parent_id = self.scheduler_id.clone().unwrap_or_else(|| {
            self.root_id
                .clone()
                .unwrap_or_else(|| CLI_NODE_ID_PROMPT.to_string())
        });
        let status = stage
            .status
            .clone()
            .unwrap_or_else(|| SchedulerStageStatus::Running.as_str().to_string());
        let node = self
            .nodes
            .entry(stage_id.clone())
            .or_insert(CliObservedExecutionNode {
                kind: CLI_NODE_KIND_STAGE.to_string(),
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
        if matches!(
            SchedulerStageStatus::parse(status.as_str()),
            Some(
                SchedulerStageStatus::Running
                    | SchedulerStageStatus::Waiting
                    | SchedulerStageStatus::Cancelling
                    | SchedulerStageStatus::Retrying
            )
        ) {
            self.active_stage_id = Some(stage_id.clone());
        }
        if let Some(scheduler_id) = self.scheduler_id.clone() {
            if let Some(scheduler) = self.nodes.get_mut(&scheduler_id) {
                scheduler.waiting_on = stage.waiting_on.clone();
                scheduler.recent_event = stage.last_event.clone();
                scheduler.status = match SchedulerStageStatus::parse(status.as_str()) {
                    Some(SchedulerStageStatus::Waiting | SchedulerStageStatus::Blocked) => {
                        SchedulerStageStatus::Waiting.as_str().to_string()
                    }
                    Some(SchedulerStageStatus::Cancelling) => {
                        SchedulerStageStatus::Cancelling.as_str().to_string()
                    }
                    Some(SchedulerStageStatus::Retrying) => {
                        SchedulerStageStatus::Retrying.as_str().to_string()
                    }
                    _ => SchedulerStageStatus::Running.as_str().to_string(),
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
            .unwrap_or_else(|| CLI_NODE_ID_PROMPT.to_string());
        let tool_id = format!("tool:{}:{}", parent_id, tool.name);
        let status = match tool.phase {
            rocode_command::output_blocks::ToolPhase::Start
            | rocode_command::output_blocks::ToolPhase::Running => ToolPhaseWire::Running.as_str(),
            rocode_command::output_blocks::ToolPhase::Done => ToolPhaseWire::Done.as_str(),
            rocode_command::output_blocks::ToolPhase::Error => ToolPhaseWire::Error.as_str(),
        }
        .to_string();
        let node = self
            .nodes
            .entry(tool_id.clone())
            .or_insert(CliObservedExecutionNode {
                kind: CLI_NODE_KIND_TOOL.to_string(),
                label: tool.name.clone(),
                status: status.clone(),
                waiting_on: Some(SchedulerStageWaitingOn::Tool.as_str().to_string()),
                recent_event: tool.detail.clone(),
                children: Vec::new(),
            });
        node.status = status.clone();
        node.waiting_on = if matches!(tool.phase, rocode_command::output_blocks::ToolPhase::Done) {
            None
        } else {
            Some(SchedulerStageWaitingOn::Tool.as_str().to_string())
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
            .unwrap_or_else(|| CLI_NODE_ID_PROMPT.to_string());
        let question_id = format!("question:{}:{}", parent_id, count);
        self.nodes.insert(
            question_id.clone(),
            CliObservedExecutionNode {
                kind: CLI_NODE_KIND_QUESTION.to_string(),
                label: format!("Question ({})", count),
                status: SchedulerStageStatus::Waiting.as_str().to_string(),
                waiting_on: Some(SchedulerStageWaitingOn::User.as_str().to_string()),
                recent_event: Some("Waiting for user answer".to_string()),
                children: Vec::new(),
            },
        );
        self.attach_child(&parent_id, &question_id);
    }

    fn finish_question(&mut self, outcome: &str) {
        for node in self
            .nodes
            .values_mut()
            .filter(|node| node.kind == CLI_NODE_KIND_QUESTION)
        {
            if node.status == SchedulerStageStatus::Waiting.as_str() {
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
                    .unwrap_or_else(|| ToolCallStatusWire::Completed.as_str().to_string())
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
    runtime: Option<&CliExecutionRuntime>,
    style: &CliStyle,
) {
    let Ok(topology) = observed_topology.lock() else {
        let _ = print_cli_list_on_surface(
            runtime,
            "Execution Topology",
            None,
            &[style.dim("unavailable")],
            style,
        );
        return;
    };
    if topology.nodes.is_empty() {
        let _ = print_cli_list_on_surface(
            runtime,
            "Execution Topology",
            None,
            &[style.dim("no observed execution topology")],
            style,
        );
        return;
    }
    let mut lines = Vec::new();
    if topology.active {
        lines.push(style.bold_green("active"));
    } else {
        lines.push(style.dim("idle · last observed topology"));
    }
    if let Some(root_id) = topology.root_id.as_deref() {
        cli_collect_execution_node(&topology, root_id, "", true, &mut lines);
    }
    let _ = print_cli_list_on_surface(runtime, "Execution Topology", None, &lines, style);
}

fn cli_collect_execution_node(
    topology: &CliObservedExecutionTopology,
    node_id: &str,
    prefix: &str,
    is_last: bool,
    lines: &mut Vec<String>,
) {
    let Some(node) = topology.nodes.get(node_id) else {
        return;
    };
    let branch = if prefix.is_empty() {
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
    lines.push(line);
    let child_prefix = if prefix.is_empty() {
        "  ".to_string()
    } else if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };
    for (index, child_id) in node.children.iter().enumerate() {
        cli_collect_execution_node(
            topology,
            child_id,
            &child_prefix,
            index + 1 == node.children.len(),
            lines,
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

async fn build_cli_execution_runtime(
    input: CliRuntimeBuildInput<'_>,
) -> anyhow::Result<CliExecutionRuntime> {
    let CliRuntimeBuildInput {
        config,
        agent_registry,
        selection,
    } = input;
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

    // Shared spinner guard slot — closures capture this; process_message_with_mode
    // swaps in the real spinner's guard each cycle.
    let spinner_guard: Arc<std::sync::Mutex<SpinnerGuard>> =
        Arc::new(std::sync::Mutex::new(SpinnerGuard::noop()));
    let prompt_session_slot: Arc<std::sync::Mutex<Option<Arc<PromptSession>>>> =
        Arc::new(std::sync::Mutex::new(None));

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
        spinner_guard,
        api_client: None,
        server_session_id: None,
        related_session_ids: Arc::new(Mutex::new(BTreeSet::new())),
        root_session_transcript: Arc::new(Mutex::new(CliRetainedTranscript::default())),
        child_session_transcripts: Arc::new(Mutex::new(HashMap::new())),
        focused_session_id: Arc::new(Mutex::new(None)),
        permission_memory: Arc::new(AsyncMutex::new(PermissionMemory::new())),
        show_thinking: Arc::new(AtomicBool::new(selection.show_thinking)),
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

fn cli_list_presets(
    config: &Config,
    active_profile: Option<&str>,
    runtime: Option<&CliExecutionRuntime>,
) {
    let style = CliStyle::detect();
    let lines = cli_available_presets(config)
        .into_iter()
        .map(|preset| {
            let active = if active_profile == Some(preset.as_str()) {
                format!(" {}", style.bold_green("← active"))
            } else {
                String::new()
            };
            format!("{preset}{active}")
        })
        .collect::<Vec<_>>();
    let _ = print_cli_list_on_surface(runtime, "Available Presets", None, &lines, &style);
}

fn cli_copy_target_transcript(runtime: &CliExecutionRuntime) -> Option<String> {
    if let Some(focused_session_id) = cli_focused_session_id(runtime) {
        return runtime
            .child_session_transcripts
            .lock()
            .ok()
            .and_then(|transcripts| {
                transcripts
                    .get(&focused_session_id)
                    .map(CliRetainedTranscript::rendered_text)
            });
    }

    runtime
        .root_session_transcript
        .lock()
        .ok()
        .map(|transcript| transcript.rendered_text())
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
/// Uses `\r\n` line endings for correct display when the terminal is in raw mode.
fn render_cli_list(
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    style: &CliStyle,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\r\n  {} {}\r\n",
        style.bold_cyan(style.bullet()),
        style.bold(title),
    ));
    if lines.is_empty() {
        out.push_str(&format!("    {}\r\n", style.dim("(none)")));
    } else {
        for line in lines {
            out.push_str(&format!("    {}\r\n", line));
        }
    }
    if let Some(footer) = footer {
        out.push_str(&format!("    {}\r\n", style.dim(footer)));
    }
    out.push_str("\r\n");
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

fn cli_prompt_screen_lines() -> Vec<String> {
    Vec::new()
}

fn cli_session_metadata_string(session: &SessionInfo, key: &str) -> Option<String> {
    session
        .metadata
        .as_ref()?
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn cli_recent_session_info_for_directory(
    sessions: &[SessionInfo],
    current_dir: &Path,
) -> Option<CliRecentSessionInfo> {
    let current_dir = current_dir.display().to_string();
    let session = sessions
        .iter()
        .filter(|session| session.directory == current_dir)
        .max_by_key(|session| session.time.updated)
        .or_else(|| sessions.iter().max_by_key(|session| session.time.updated))?;

    let model_label = cli_session_metadata_string(session, "current_model").or_else(|| {
        cli_session_metadata_string(session, session_keys::MODEL_PROVIDER)
            .zip(cli_session_metadata_string(session, session_keys::MODEL_ID))
            .map(|(provider, model)| format!("{provider}/{model}"))
    });
    let preset_label = cli_session_metadata_string(session, scheduler_keys::PROFILE)
        .or_else(|| cli_session_metadata_string(session, scheduler_keys::RESOLVED_PROFILE))
        .or_else(|| {
            cli_session_metadata_string(session, session_keys::AGENT)
                .map(|agent| format!("agent:{agent}"))
        });
    let title = (!session.title.trim().is_empty()).then(|| session.title.trim().to_string());

    Some(CliRecentSessionInfo {
        title,
        model_label,
        preset_label,
    })
}

async fn cli_load_recent_session_info(
    api_client: &CliApiClient,
    current_dir: &Path,
) -> Option<CliRecentSessionInfo> {
    let sessions = api_client.list_sessions(None, Some(20)).await.ok()?;
    cli_recent_session_info_for_directory(&sessions, current_dir)
}

fn cli_render_startup_banner(style: &CliStyle, recent: Option<&CliRecentSessionInfo>) -> String {
    let mut out = String::new();
    out.push_str("\r\n");

    for (idx, line) in logo_lines("").into_iter().enumerate() {
        let rendered = if idx == 0 {
            style.bold_rgb(&line, 94, 196, 255)
        } else {
            style.rgb(&line, 145, 167, 196)
        };
        out.push_str(&rendered);
        out.push_str("\r\n");
    }

    out.push_str(&style.dim(&format!(
        "{APP_SHORT_NAME} {APP_VERSION_DATE} · {APP_TAGLINE}"
    )));
    out.push_str("\r\n");

    if let Some(recent) = recent {
        if let Some(title) = recent.title.as_deref() {
            out.push_str(&style.bold("Last session: "));
            out.push_str(title);
            out.push_str("\r\n");
        }
        out.push_str(&style.bold("Last model: "));
        out.push_str(recent.model_label.as_deref().unwrap_or("—"));
        out.push_str("\r\n");
        out.push_str(&style.bold("Last preset: "));
        out.push_str(recent.preset_label.as_deref().unwrap_or("—"));
        out.push_str("\r\n");
    }

    out.push_str("\r\n");
    out
}

fn cli_is_terminal_stage_status(status: Option<&str>) -> bool {
    matches!(
        status.and_then(SchedulerStageStatus::parse),
        Some(
            SchedulerStageStatus::Done
                | SchedulerStageStatus::Blocked
                | SchedulerStageStatus::Cancelled
        )
    )
}

fn cli_set_root_server_session(runtime: &mut CliExecutionRuntime, session_id: String) {
    runtime.server_session_id = Some(session_id.clone());
    if let Ok(mut related) = runtime.related_session_ids.lock() {
        related.clear();
        related.insert(session_id);
    }
    if let Ok(mut root) = runtime.root_session_transcript.lock() {
        root.clear();
    }
    if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
        transcripts.clear();
    }
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = None;
    }
    cli_set_view_label(runtime, None);
}

fn cli_tracks_related_session(runtime: &CliExecutionRuntime, session_id: &str) -> bool {
    if session_id.is_empty() {
        return true;
    }
    runtime
        .related_session_ids
        .lock()
        .map(|related| related.contains(session_id))
        .unwrap_or(false)
}

fn cli_track_child_session(runtime: &CliExecutionRuntime, parent_id: &str, child_id: &str) -> bool {
    if parent_id.is_empty() || child_id.is_empty() {
        return false;
    }
    let mut inserted = false;
    if let Ok(mut related) = runtime.related_session_ids.lock() {
        if related.contains(parent_id) {
            inserted = related.insert(child_id.to_string());
        }
    }
    if inserted {
        if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
            transcripts.entry(child_id.to_string()).or_default();
        }
    }
    inserted
}

fn cli_untrack_child_session(
    runtime: &CliExecutionRuntime,
    parent_id: &str,
    child_id: &str,
) -> bool {
    if parent_id.is_empty() || child_id.is_empty() {
        return false;
    }
    runtime
        .related_session_ids
        .lock()
        .map(|mut related| related.contains(parent_id) && related.remove(child_id))
        .unwrap_or(false)
}

fn cli_cache_child_session_block(
    runtime: &CliExecutionRuntime,
    session_id: &str,
    block: &OutputBlock,
    style: &CliStyle,
) {
    let rendered = render_cli_block_rich(block, style);
    if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
        transcripts
            .entry(session_id.to_string())
            .or_default()
            .append_rendered(&rendered);
    }
}

fn cli_cache_root_session_block(
    runtime: &CliExecutionRuntime,
    block: &OutputBlock,
    style: &CliStyle,
) {
    let rendered = render_cli_block_rich(block, style);
    if let Ok(mut transcript) = runtime.root_session_transcript.lock() {
        transcript.append_rendered(&rendered);
    }
}

fn cli_capture_visible_root_transcript(runtime: &CliExecutionRuntime) {
    let snapshot = runtime
        .frontend_projection
        .lock()
        .ok()
        .map(|projection| projection.transcript.clone());
    if let Some(snapshot) = snapshot {
        if let Ok(mut root) = runtime.root_session_transcript.lock() {
            *root = snapshot;
        }
    }
}

fn cli_focused_session_id(runtime: &CliExecutionRuntime) -> Option<String> {
    runtime
        .focused_session_id
        .lock()
        .ok()
        .and_then(|focused| focused.clone())
}

fn cli_is_root_focused(runtime: &CliExecutionRuntime) -> bool {
    cli_focused_session_id(runtime).is_none()
}

fn cli_replace_visible_transcript(
    runtime: &CliExecutionRuntime,
    transcript: CliRetainedTranscript,
) -> io::Result<()> {
    if let Some(surface) = runtime.terminal_surface.as_ref() {
        surface.replace_transcript(transcript)
    } else {
        if let Ok(mut projection) = runtime.frontend_projection.lock() {
            projection.transcript = transcript;
            projection.scroll_offset = 0;
        }
        Ok(())
    }
}

fn cli_short_session_id(session_id: &str) -> &str {
    &session_id[..session_id.len().min(8)]
}

fn cli_set_view_label(runtime: &CliExecutionRuntime, label: Option<String>) {
    if let Ok(mut projection) = runtime.frontend_projection.lock() {
        projection.view_label = label;
    }
    cli_refresh_prompt(runtime);
}

fn cli_ordered_child_session_ids(runtime: &CliExecutionRuntime) -> Vec<String> {
    let root_session_id = runtime.server_session_id.as_deref();
    let attached_ids = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();

    let mut child_ids = BTreeSet::new();
    for session_id in &attached_ids {
        if root_session_id != Some(session_id.as_str()) {
            child_ids.insert(session_id.clone());
        }
    }
    for session_id in transcripts.keys() {
        child_ids.insert(session_id.clone());
    }

    child_ids.into_iter().collect()
}

fn cli_list_child_sessions(runtime: &CliExecutionRuntime) {
    let style = CliStyle::detect();
    let attached_ids = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();
    let focused = cli_focused_session_id(runtime);

    let mut lines = Vec::new();
    let child_ids = cli_ordered_child_session_ids(runtime);
    if child_ids.is_empty() {
        lines.push("No child sessions have been observed for this run yet.".to_string());
        lines.push("When scheduler agents fork, they will appear here.".to_string());
    } else {
        for session_id in child_ids {
            let transcript = transcripts.get(&session_id);
            let attached = attached_ids.contains(&session_id);
            let focus_marker = if focused.as_deref() == Some(session_id.as_str()) {
                "● focused"
            } else {
                "○ cached"
            };
            let status = if attached { "attached" } else { "detached" };
            let line_count = transcript.map(|item| item.line_count()).unwrap_or(0);
            lines.push(format!(
                "{}  {}  [{} · {} lines]",
                focus_marker, session_id, status, line_count
            ));
            if let Some(summary) = transcript
                .and_then(|item| item.last_line())
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                lines.push(format!("    {}", truncate_text(summary, 88)));
            }
        }
    }

    let footer = match focused {
        Some(child_id) => format!(
            "/child next · /child prev · /child focus <id> · /child back · now viewing {}",
            child_id
        ),
        None => "/child next · /child prev · /child focus <id> · /child back".to_string(),
    };
    let _ = print_cli_list_on_surface(
        Some(runtime),
        "Child Sessions",
        Some(&footer),
        &lines,
        &style,
    );
}

fn cli_focus_child_session(runtime: &CliExecutionRuntime, requested_id: &str) -> io::Result<bool> {
    let requested_id = requested_id.trim();
    if requested_id.is_empty() {
        return Ok(false);
    }

    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();
    let related = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let root_session_id = runtime.server_session_id.as_deref();

    let mut candidates = BTreeSet::new();
    for session_id in related {
        if root_session_id != Some(session_id.as_str()) {
            candidates.insert(session_id);
        }
    }
    for session_id in transcripts.keys() {
        candidates.insert(session_id.clone());
    }

    let target = if candidates.contains(requested_id) {
        Some(requested_id.to_string())
    } else {
        let mut prefix_matches = candidates
            .into_iter()
            .filter(|candidate| candidate.starts_with(requested_id))
            .collect::<Vec<_>>();
        if prefix_matches.len() == 1 {
            prefix_matches.pop()
        } else {
            None
        }
    };

    let Some(target_id) = target else {
        return Ok(false);
    };

    let Some(transcript) = transcripts.get(&target_id).cloned() else {
        return Ok(false);
    };

    if cli_is_root_focused(runtime) {
        cli_capture_visible_root_transcript(runtime);
    }
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = Some(target_id.clone());
    }
    cli_set_view_label(
        runtime,
        Some(format!("view child {}", cli_short_session_id(&target_id))),
    );
    cli_replace_visible_transcript(runtime, transcript)?;
    Ok(true)
}

fn cli_cycle_child_session(
    runtime: &CliExecutionRuntime,
    forward: bool,
) -> io::Result<Option<(String, usize, usize)>> {
    let child_ids = cli_ordered_child_session_ids(runtime);
    if child_ids.is_empty() {
        return Ok(None);
    }

    let focused = cli_focused_session_id(runtime);
    let next_index = match focused
        .as_deref()
        .and_then(|current| child_ids.iter().position(|id| id == current))
    {
        Some(index) if forward => (index + 1) % child_ids.len(),
        Some(index) => (index + child_ids.len() - 1) % child_ids.len(),
        None if forward => 0,
        None => child_ids.len() - 1,
    };
    let target_id = child_ids[next_index].clone();
    if !cli_focus_child_session(runtime, &target_id)? {
        return Ok(None);
    }
    Ok(Some((target_id, next_index + 1, child_ids.len())))
}

fn cli_focus_root_session(runtime: &CliExecutionRuntime) -> io::Result<bool> {
    if cli_is_root_focused(runtime) {
        return Ok(false);
    }
    let transcript = runtime
        .root_session_transcript
        .lock()
        .map(|item| item.clone())
        .unwrap_or_default();
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = None;
    }
    cli_set_view_label(runtime, None);
    cli_replace_visible_transcript(runtime, transcript)?;
    Ok(true)
}

fn cli_session_update_requires_refresh(source: Option<&str>) -> bool {
    matches!(
        source,
        Some(
            "prompt.final"
                | "stream.final"
                | "prompt.completed"
                | "session.title.set"
                | "prompt.done"
        )
    )
}

#[cfg(test)]
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
    if let Some(waiting_on) = stage
        .waiting_on
        .as_deref()
        .filter(|value| !value.is_empty())
    {
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
    if let Some(last_event) = stage
        .last_event
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(truncate_display(&format!("Last: {last_event}"), max_width));
    }
    if let Some(ref child_id) = stage.child_session_id {
        lines.push(truncate_display(&format!("Child: {child_id}"), max_width));
    }
    lines
}

fn cli_attach_interactive_handles(
    runtime: &mut CliExecutionRuntime,
    handles: CliInteractiveHandles,
) {
    runtime.terminal_surface = Some(handles.terminal_surface);
    runtime.prompt_chrome = Some(handles.prompt_chrome.clone());
    runtime.prompt_session = Some(handles.prompt_session.clone());
    if let Ok(mut slot) = runtime.prompt_session_slot.lock() {
        *slot = Some(handles.prompt_session.clone());
    }
    runtime.queued_inputs = handles.queued_inputs;
    runtime.busy_flag = handles.busy_flag;
    runtime.exit_requested = handles.exit_requested;
    runtime.active_abort = handles.active_abort;
    handles.prompt_chrome.update_from_runtime(runtime);
    cli_refresh_prompt(runtime);
}

async fn cli_trigger_abort(handle: CliActiveAbortHandle) -> bool {
    match handle {
        CliActiveAbortHandle::Server {
            api_client,
            session_id,
        } => match api_client.abort_session(&session_id).await {
            Ok(result) => result
                .get("aborted")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            Err(e) => {
                tracing::error!("Failed to abort server session: {}", e);
                false
            }
        },
    }
}

async fn cli_execute_new_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    match api_client
        .create_session(None, runtime.resolved_scheduler_profile_name.clone())
        .await
    {
        Ok(new_session) => {
            let new_sid = new_session.id.clone();
            cli_set_root_server_session(runtime, new_sid.clone());

            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                proj.token_stats = CliSessionTokenStats::default();
            }

            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title(format!(
                    "New session created: {}",
                    &new_sid[..new_sid.len().min(8)]
                ))),
                repl_style,
            );

            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&new_sid)).await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to create new session: {}",
                    error
                ))),
                repl_style,
            );
        }
    }
}

async fn cli_execute_fork_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    let Some(session_id) = runtime.server_session_id.clone() else {
        let _ = print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::warning("No active server session to fork.")),
            repl_style,
        );
        return;
    };

    match api_client.fork_session(&session_id, None).await {
        Ok(forked) => {
            cli_set_root_server_session(runtime, forked.id.clone());
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title(format!("Forked session: {}", forked.id))),
                repl_style,
            );
            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&forked.id))
                .await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to fork session: {}",
                    error
                ))),
                repl_style,
            );
        }
    }
}

async fn cli_execute_compact_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    let Some(session_id) = runtime.server_session_id.clone() else {
        let _ = print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::warning("No server session to compact.")),
            repl_style,
        );
        return;
    };

    match api_client.compact_session(&session_id).await {
        Ok(_) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title("Session compacted successfully.")),
                repl_style,
            );
            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                proj.token_stats = CliSessionTokenStats::default();
            }
            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&session_id))
                .await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to compact session: {}",
                    error
                ))),
                repl_style,
            );
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
            projection.phase = match stage
                .status
                .as_deref()
                .and_then(SchedulerStageStatus::parse)
            {
                Some(SchedulerStageStatus::Waiting | SchedulerStageStatus::Blocked) => {
                    CliFrontendPhase::Waiting
                }
                Some(SchedulerStageStatus::Cancelling) => CliFrontendPhase::Cancelling,
                Some(SchedulerStageStatus::Cancelled | SchedulerStageStatus::Done) => {
                    projection.phase
                }
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
    let stage_id = stage.stage_id.clone().unwrap_or_else(|| {
        format!(
            "{}:{}",
            stage.stage_index.unwrap_or_default(),
            stage.stage.as_str()
        )
    });
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn cli_box_line(text: &str, inner_width: usize, style: &CliStyle) -> String {
    let content = pad_right_display(text, inner_width, ' ');
    if style.color {
        format!("{} {} {}", style.cyan("│"), content, style.cyan("│"))
    } else {
        format!("│ {} │", content)
    }
}

#[cfg(test)]
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
        format!(
            "{}{}{}",
            style.cyan("╰"),
            style.dim(&footer_content),
            style.cyan("╯")
        )
    } else {
        format!("╰{}╯", footer_content)
    };

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(header);
    rendered.extend(
        lines
            .iter()
            .map(|line| cli_box_line(line, inner_width, style)),
    );
    rendered.push(footer);
    rendered
}

#[cfg(test)]
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

#[cfg(test)]
fn cli_terminal_rows() -> usize {
    crossterm::terminal::size()
        .map(|(_, rows)| usize::from(rows))
        .unwrap_or(28)
}

#[cfg(test)]
fn cli_sidebar_lines(
    projection: &CliFrontendProjection,
    topology: &CliObservedExecutionTopology,
) -> Vec<String> {
    let phase = match projection.phase {
        CliFrontendPhase::Failed => ToolCallStatusWire::Error.as_str(),
        other => other.as_str(),
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
        let connected = projection
            .mcp_servers
            .iter()
            .filter(|s| {
                McpConnectionStatusWire::parse(&s.status)
                    == Some(McpConnectionStatusWire::Connected)
            })
            .count();
        let errored = projection
            .mcp_servers
            .iter()
            .filter(|s| {
                McpConnectionStatusWire::parse(&s.status) == Some(McpConnectionStatusWire::Failed)
            })
            .count();
        lines.push(String::new());
        lines.push(format!("─ MCP ({} active, {} err) ─", connected, errored));
        for server in &projection.mcp_servers {
            let indicator = match McpConnectionStatusWire::parse(&server.status) {
                Some(McpConnectionStatusWire::Connected) => "●",
                Some(McpConnectionStatusWire::Failed) => "✗",
                Some(
                    McpConnectionStatusWire::NeedsAuth
                    | McpConnectionStatusWire::NeedsClientRegistration,
                ) => "?",
                Some(McpConnectionStatusWire::Disabled | McpConnectionStatusWire::Disconnected)
                | None => "○",
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
    lines.push("/child · /abort · /sidebar".to_string());
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

#[cfg(test)]
fn cli_active_stage_panel_lines(
    stage: Option<&SchedulerStageBlock>,
    style: &CliStyle,
) -> Vec<String> {
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
    if stage.total_agent_count > 0 {
        lines.push(format!(
            "Agents: [{}/{}]",
            stage.done_agent_count, stage.total_agent_count
        ));
    }
    if let Some(ref child_id) = stage.child_session_id {
        lines.push(format!("→ Child session: {}", child_id));
    }
    lines
}

#[cfg(test)]
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
        format!("line {}/{} · /up /down /bottom", position + 1, total,)
    }
}

#[cfg(test)]
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
    let mut header_parts = vec![
        truncate_display(session_title, 32),
        mode.to_string(),
        model.to_string(),
    ];
    if let Some(view_label) = projection
        .view_label
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        header_parts.push(view_label.to_string());
    }
    header_parts.push(truncate_display(directory, 24));
    let header_lines = vec![format!("> {}", header_parts.join(" · "))];
    let header_box = cli_render_box("ROCode", None, &header_lines, total_width, style);

    // ── Adaptive layout: compute actual content sizes, then allocate rows ──
    //
    // Fixed chrome overhead (lines consumed by box borders):
    //   header_box:   3 lines (top border + 1 content + bottom border)
    //   messages_box: 2 lines (top border + bottom border)
    //   active_box:   2 lines (top border + bottom border) when expanded, 3 when collapsed
    //   prompt:       ~8 lines (rendered separately by PromptFrame, not counted in screen_lines)
    //
    // Remaining rows after chrome are split between messages content and active content,
    // with active getting exactly what it needs (clamped) and messages getting the rest.

    let active_inner_width = total_width.saturating_sub(4).max(1);

    // Compute active panel's natural content height
    let (active_content_lines, active_chrome) = if projection.active_collapsed {
        // Collapsed: single label line + 2 chrome = 3 total
        (Vec::new(), 3usize) // content handled separately below
    } else {
        let raw_lines = cli_active_stage_panel_lines(projection.active_stage.as_ref(), style);
        // Wrap lines to actual width to get true row count
        let mut wrapped_count = 0usize;
        for line in &raw_lines {
            wrapped_count += 1.max(
                (display_width(line) + active_inner_width.saturating_sub(1))
                    / active_inner_width.max(1),
            );
        }
        let natural_rows = if raw_lines.is_empty() {
            1
        } else {
            wrapped_count
        };
        (raw_lines, 2 + natural_rows.clamp(2, 12)) // chrome(2) + content(2..12)
    };

    // Total chrome = header(3) + messages chrome(2) + active chrome + prompt overhead(~8)
    let prompt_overhead = 8usize; // header + ~6 visible rows + footer
    let total_chrome = 3 + 2 + active_chrome + prompt_overhead;
    let sidebar_overhead = if projection.sidebar_collapsed { 3 } else { 0 };

    // Messages get whatever remains after chrome and active content
    let body_rows = terminal_rows.saturating_sub(total_chrome).max(4) + sidebar_overhead;

    let mut screen = Vec::new();
    screen.extend(header_box);

    if projection.sidebar_collapsed {
        // Full-width Messages only, no sidebar column
        let messages_inner = total_width.saturating_sub(4).max(1);
        let transcript_lines = projection.transcript.viewport_lines(
            messages_inner,
            body_rows,
            projection.scroll_offset,
        );
        let messages_footer = cli_messages_footer(
            &projection.transcript,
            messages_inner,
            body_rows,
            projection.scroll_offset,
        );
        let messages_box = cli_render_box(
            "Messages",
            Some(&messages_footer),
            &transcript_lines,
            total_width,
            style,
        );
        screen.extend(messages_box);
    } else {
        let right_width = (if total_width >= 128 { 38 } else { 32 })
            .min(total_width.saturating_sub(29 + gap))
            .max(24);
        let left_width = total_width.saturating_sub(right_width + gap);
        let left_inner = left_width.saturating_sub(4).max(1);
        let right_inner = right_width.saturating_sub(4).max(1);
        let transcript_lines =
            projection
                .transcript
                .viewport_lines(left_inner, body_rows, projection.scroll_offset);
        let messages_footer = cli_messages_footer(
            &projection.transcript,
            left_inner,
            body_rows,
            projection.scroll_offset,
        );
        let sidebar_lines = cli_fit_lines(
            &cli_sidebar_lines(projection, topology),
            right_inner,
            body_rows,
            false,
        );
        let messages_box = cli_render_box(
            "Messages",
            Some(&messages_footer),
            &transcript_lines,
            left_width,
            style,
        );
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
        // Use actual content height (already clamped 2..12 during budget computation)
        let active_rows = active_chrome.saturating_sub(2); // remove chrome to get content rows
        let active_lines = cli_fit_lines(
            &active_content_lines,
            active_inner_width,
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
    thinking_requested: bool,
) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let config = load_config(&current_dir)?;
    let command_registry = CommandRegistry::new();
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

    let agent_registry_arc = Arc::new(AgentRegistry::from_config(&config));

    // ── Server connection & recent session info ──────────────────────
    // Connect to (or start) the server early so we can load the most
    // recent session's model/preset and carry them into the new session
    // when the user hasn't explicitly overridden them via CLI flags.
    let server_url = discover_or_start_server(None).await?;
    let api_client = Arc::new(CliApiClient::new(server_url.clone()));
    let server_config = api_client.get_config().await.ok();
    let recent_session_info = cli_load_recent_session_info(&api_client, &current_dir).await;

    // ── Selection: CLI flags → recent session fallback ───────────────
    let (carry_model, carry_provider) = recent_session_info
        .as_ref()
        .and_then(|info| info.model_label.clone())
        .map(|label| {
            // model_label is stored as "provider/model_id"
            let (p, m) = parse_model_and_provider(Some(label));
            (m, p)
        })
        .unwrap_or((None, None));

    let carry_preset = recent_session_info
        .as_ref()
        .and_then(|info| info.preset_label.as_deref())
        .and_then(|label| {
            // Ignore "agent:xxx" entries — only carry pure preset names.
            if label.starts_with("agent:") {
                None
            } else {
                Some(label.to_string())
            }
        });

    let selection = CliRunSelection {
        model: model.or(carry_model),
        provider: provider.or(carry_provider),
        requested_agent,
        requested_scheduler_profile: requested_scheduler_profile.or(carry_preset),
        show_thinking: cli_resolve_show_thinking(thinking_requested, server_config.as_ref(), true),
    };

    let mut runtime = build_cli_execution_runtime(CliRuntimeBuildInput {
        config: &config,
        agent_registry: agent_registry_arc.clone(),
        selection: &selection,
    })
    .await?;
    let repl_style = CliStyle::detect();

    // ── Session creation ─────────────────────────────────────────────
    let session_info = api_client
        .create_session(None, selection.requested_scheduler_profile.clone())
        .await?;
    let server_session_id = session_info.id.clone();
    runtime.api_client = Some(api_client.clone());
    cli_set_root_server_session(&mut runtime, server_session_id.clone());

    tracing::info!(
        server_url = %server_url,
        session_id = %server_session_id,
        "CLI connected to server and created session"
    );

    let shared_frontend_projection = runtime.frontend_projection.clone();
    let queued_inputs = runtime.queued_inputs.clone();
    let busy_flag = runtime.busy_flag.clone();
    let exit_requested = runtime.exit_requested.clone();
    let active_abort = runtime.active_abort.clone();
    let terminal_surface = Arc::new(CliTerminalSurface::new(
        repl_style.clone(),
        runtime.frontend_projection.clone(),
    ));
    let prompt_chrome = Arc::new(CliPromptChrome::new(
        &runtime,
        &repl_style,
        &current_dir,
        &config,
        provider_registry.as_ref(),
        agent_registry_arc.as_ref(),
    ));
    let (prompt_event_tx, mut prompt_event_rx) = mpsc::unbounded_channel();
    let prompt_session = Arc::new(PromptSession::spawn(
        Arc::new({
            let prompt_chrome = prompt_chrome.clone();
            move |line, cursor_pos| prompt_chrome.frame(line, cursor_pos)
        }),
        Some(Arc::new({
            let prompt_chrome = prompt_chrome.clone();
            move |line, cursor_pos| prompt_chrome.assist(line, cursor_pos).completion
        })),
        prompt_event_tx,
    )?);
    terminal_surface.set_prompt_session(prompt_session.clone());
    terminal_surface.print_text(&cli_render_startup_banner(
        &repl_style,
        recent_session_info.as_ref(),
    ))?;
    cli_attach_interactive_handles(
        &mut runtime,
        CliInteractiveHandles {
            terminal_surface: terminal_surface.clone(),
            prompt_chrome: prompt_chrome.clone(),
            prompt_session: prompt_session.clone(),
            queued_inputs: queued_inputs.clone(),
            busy_flag: busy_flag.clone(),
            exit_requested: exit_requested.clone(),
            active_abort: active_abort.clone(),
        },
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
                                let _ =
                                    terminal_surface.print_block(OutputBlock::Status(if aborted {
                                        StatusBlock::warning("Abort requested for active run.")
                                    } else {
                                        StatusBlock::warning("No active run to abort.")
                                    }));
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
                                    CliServerEvent::ConfigUpdated => {
                                        cli_handle_config_updated_from_sse(&runtime, &api_client).await;
                                    }
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
                                    CliServerEvent::PermissionRequested {
                                        session_id,
                                        permission_id,
                                        info_json,
                                    } => {
                                        if cli_tracks_related_session(&runtime, &session_id) {
                                            handle_permission_from_sse(
                                                &runtime,
                                                &api_client,
                                                &permission_id,
                                                &info_json,
                                            ).await;
                                        }
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

        if let Some(resolved) = cli_resolve_registry_ui_action(&command_registry, &trimmed) {
            match cli_execute_ui_action(
                resolved.action_id,
                resolved.argument.as_deref(),
                &mut runtime,
                &api_client,
                &provider_registry,
                &agent_registry_arc,
                &current_dir,
                &repl_style,
            )
            .await?
            {
                CliUiActionOutcome::Break => break,
                CliUiActionOutcome::Continue => continue,
            }
        }

        if let Some(cmd) = parse_interactive_command(&trimmed) {
            if let Some(invocation) = cmd.ui_action_invocation() {
                match cli_execute_ui_action(
                    invocation.action_id,
                    invocation.argument.as_deref(),
                    &mut runtime,
                    &api_client,
                    &provider_registry,
                    &agent_registry_arc,
                    &current_dir,
                    &repl_style,
                )
                .await?
                {
                    CliUiActionOutcome::Break => break,
                    CliUiActionOutcome::Continue => continue,
                }
            }
            match cmd {
                InteractiveCommand::Abort => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "No active run to abort. Use /abort while a response is running.",
                        )),
                        &repl_style,
                    );
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
                    run_server_prompt(
                        &mut runtime,
                        &api_client,
                        &mut sse_rx,
                        &action.prompt,
                        &repl_style,
                        false,
                    )
                    .await?;
                }
                InteractiveCommand::ClearScreen => {
                    if let Some(surface) = runtime.terminal_surface.as_ref() {
                        let _ = surface.clear_transcript();
                    } else {
                        print!("\x1B[2J\x1B[1;1H");
                        io::stdout().flush()?;
                    }
                }
                InteractiveCommand::ListChildSessions => {
                    cli_list_child_sessions(&runtime);
                }
                InteractiveCommand::FocusChildSession(session_id) => {
                    match cli_focus_child_session(&runtime, &session_id) {
                        Ok(true) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Focused child session: {}",
                                    session_id
                                ))),
                                &repl_style,
                            );
                        }
                        Ok(false) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::warning(format!(
                                    "Unknown child session: {}. Use /child list first.",
                                    session_id
                                ))),
                                &repl_style,
                            );
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to focus child session: {}",
                                    error
                                ))),
                                &repl_style,
                            );
                        }
                    }
                }
                InteractiveCommand::FocusNextChildSession => {
                    match cli_cycle_child_session(&runtime, true) {
                        Ok(Some((session_id, index, total))) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Focused child session [{}/{}]: {}",
                                    index, total, session_id
                                ))),
                                &repl_style,
                            );
                        }
                        Ok(None) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::warning(
                                    "No child sessions available. Use /child list to inspect the cache.",
                                )),
                                &repl_style,
                            );
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to switch to next child session: {}",
                                    error
                                ))),
                                &repl_style,
                            );
                        }
                    }
                }
                InteractiveCommand::FocusPreviousChildSession => {
                    match cli_cycle_child_session(&runtime, false) {
                        Ok(Some((session_id, index, total))) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Focused child session [{}/{}]: {}",
                                    index, total, session_id
                                ))),
                                &repl_style,
                            );
                        }
                        Ok(None) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::warning(
                                    "No child sessions available. Use /child list to inspect the cache.",
                                )),
                                &repl_style,
                            );
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(&runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to switch to previous child session: {}",
                                    error
                                ))),
                                &repl_style,
                            );
                        }
                    }
                }
                InteractiveCommand::BackToRootSession => match cli_focus_root_session(&runtime) {
                    Ok(true) => {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::title(
                                "Returned to root session view.",
                            )),
                            &repl_style,
                        );
                    }
                    Ok(false) => {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::warning(
                                "Already viewing the root session.",
                            )),
                            &repl_style,
                        );
                    }
                    Err(error) => {
                        let _ = print_block(
                            Some(&runtime),
                            OutputBlock::Status(StatusBlock::error(format!(
                                "Failed to restore root session view: {}",
                                error
                            ))),
                            &repl_style,
                        );
                    }
                },
                InteractiveCommand::Compact => {}
                InteractiveCommand::ShowTask(id) => {
                    cli_show_task(&id, Some(&runtime));
                }
                InteractiveCommand::KillTask(id) => {
                    cli_kill_task(&id, Some(&runtime));
                }
                InteractiveCommand::ToggleActive => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "CLI mode renders stage activity inline in the transcript; no separate active panel is kept onscreen.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ScrollUp => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "Use your terminal's native scrollback in CLI mode.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ScrollDown => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "Use your terminal's native scrollback in CLI mode.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::ScrollBottom => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "Use your terminal's native scrollback in CLI mode.",
                        )),
                        &repl_style,
                    );
                }
                InteractiveCommand::InspectStage(stage_filter) => {
                    let _ = print_block(
                        Some(&runtime),
                        OutputBlock::Status(StatusBlock::title(
                            if let Some(ref sid) = stage_filter {
                                format!("Stage inspect: {} (use Web UI for full details)", sid)
                            } else {
                                "Stage inspect: use Web UI at /session/{{id}}/events for full details".to_string()
                            },
                        )),
                        &repl_style,
                    );
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
                InteractiveCommand::Exit
                | InteractiveCommand::ShowHelp
                | InteractiveCommand::ShowRecovery
                | InteractiveCommand::NewSession
                | InteractiveCommand::ShowStatus
                | InteractiveCommand::ListModels
                | InteractiveCommand::ListProviders
                | InteractiveCommand::ListThemes
                | InteractiveCommand::ListPresets
                | InteractiveCommand::ListSessions
                | InteractiveCommand::ParentSession
                | InteractiveCommand::ListTasks
                | InteractiveCommand::ListAgents
                | InteractiveCommand::Copy
                | InteractiveCommand::ToggleSidebar
                | InteractiveCommand::SelectModel(_)
                | InteractiveCommand::SelectPreset(_)
                | InteractiveCommand::SelectAgent(_) => {}
            }
            continue;
        }

        runtime.busy_flag.store(true, Ordering::SeqCst);

        run_server_prompt(
            &mut runtime,
            &api_client,
            &mut sse_rx,
            &trimmed,
            &repl_style,
            true,
        )
        .await?;

        // Drain any SSE events that arrived during processing.
        while let Ok(event) = sse_rx.try_recv() {
            match event {
                CliServerEvent::QuestionCreated {
                    request_id,
                    session_id: _,
                    questions_json,
                } => {
                    handle_question_from_sse(&runtime, &api_client, &request_id, &questions_json)
                        .await;
                }
                CliServerEvent::PermissionRequested {
                    session_id,
                    permission_id,
                    info_json,
                } => {
                    if cli_tracks_related_session(&runtime, &session_id) {
                        handle_permission_from_sse(
                            &runtime,
                            &api_client,
                            &permission_id,
                            &info_json,
                        )
                        .await;
                    }
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

async fn run_server_prompt(
    runtime: &mut CliExecutionRuntime,
    api_client: &Arc<CliApiClient>,
    sse_rx: &mut mpsc::UnboundedReceiver<CliServerEvent>,
    input: &str,
    style: &CliStyle,
    update_recovery_base: bool,
) -> anyhow::Result<()> {
    if update_recovery_base {
        runtime.recovery_base_prompt = Some(input.to_string());
    }
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
        style,
    )?;

    let Some(session_id) = runtime.server_session_id.clone() else {
        anyhow::bail!("CLI server session is not initialized");
    };

    {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = Some(CliActiveAbortHandle::Server {
            api_client: api_client.clone(),
            session_id: session_id.clone(),
        });
    }

    let prompt_agent = cli_prompt_agent_override(
        &runtime.resolved_agent_name,
        runtime.resolved_scheduler_profile_name.as_deref(),
    );

    if let Err(error) = api_client
        .send_prompt(
            &session_id,
            input.to_string(),
            prompt_agent,
            runtime.resolved_scheduler_profile_name.clone(),
            (runtime.resolved_model_label != "auto").then(|| runtime.resolved_model_label.clone()),
            None,
        )
        .await
    {
        cli_frontend_set_phase(
            &runtime.frontend_projection,
            CliFrontendPhase::Failed,
            Some("send prompt failed".to_string()),
        );
        let _ = print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::error(format!(
                "Failed to send prompt: {}",
                error
            ))),
            style,
        );
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = None;
        cli_frontend_clear(runtime);
        return Ok(());
    }

    loop {
        match sse_rx.recv().await {
            Some(CliServerEvent::QuestionCreated {
                request_id,
                session_id,
                questions_json,
            }) => {
                if cli_tracks_related_session(runtime, &session_id) {
                    handle_question_from_sse(runtime, api_client, &request_id, &questions_json)
                        .await;
                }
            }
            Some(CliServerEvent::PermissionRequested {
                session_id,
                permission_id,
                info_json,
            }) => {
                if cli_tracks_related_session(runtime, &session_id) {
                    handle_permission_from_sse(runtime, api_client, &permission_id, &info_json)
                        .await;
                }
            }
            Some(CliServerEvent::ConfigUpdated) => {
                cli_handle_config_updated_from_sse(runtime, api_client).await;
            }
            Some(CliServerEvent::SessionUpdated { session_id, source }) => {
                handle_session_updated_from_sse(
                    runtime,
                    api_client,
                    &session_id,
                    source.as_deref(),
                    style,
                )
                .await;
            }
            Some(CliServerEvent::SessionIdle {
                session_id: idle_session_id,
            }) => {
                let is_current_session = runtime
                    .server_session_id
                    .as_deref()
                    .is_some_and(|current| current == idle_session_id);
                handle_sse_event(
                    runtime,
                    CliServerEvent::SessionIdle {
                        session_id: idle_session_id,
                    },
                    style,
                );
                if !is_current_session {
                    continue;
                }
                handle_session_updated_from_sse(
                    runtime,
                    api_client,
                    &session_id,
                    Some("prompt.done"),
                    style,
                )
                .await;
                if let Ok(mut topology) = runtime.observed_topology.lock() {
                    topology.finish_run(Some("Completed".to_string()));
                }
                cli_frontend_clear(runtime);
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::success("Done.")),
                    style,
                );
                break;
            }
            Some(other) => {
                handle_sse_event(runtime, other, style);
            }
            None => break,
        }
    }

    {
        let mut active_abort = runtime.active_abort.lock().await;
        *active_abort = None;
    }
    Ok(())
}

fn cli_prompt_agent_override(
    resolved_agent_name: &str,
    resolved_scheduler_profile_name: Option<&str>,
) -> Option<String> {
    if resolved_scheduler_profile_name.is_some() {
        None
    } else {
        Some(resolved_agent_name.to_string())
    }
}

async fn cli_handle_config_updated_from_sse(
    runtime: &CliExecutionRuntime,
    _api_client: &CliApiClient,
) {
    let _ = runtime;
}

/// Handle an incoming SSE event from the server — update topology,
/// frontend projection, and render output blocks.
fn handle_sse_event(runtime: &CliExecutionRuntime, event: CliServerEvent, style: &CliStyle) {
    let root_session_id = runtime.server_session_id.as_deref();
    let focused_session_id = cli_focused_session_id(runtime);
    let is_root_session = |event_session_id: &str| {
        root_session_id.is_none_or(|sid| event_session_id.is_empty() || sid == event_session_id)
    };
    let is_related_session =
        |event_session_id: &str| cli_tracks_related_session(runtime, event_session_id);

    match event {
        CliServerEvent::ConfigUpdated => {
            tracing::debug!("config.updated reached sync handler");
        }
        CliServerEvent::SessionUpdated { session_id, source } => {
            if !is_root_session(&session_id) {
                return;
            }
            tracing::debug!(session_id, ?source, "session updated");
        }
        CliServerEvent::SessionBusy { session_id } => {
            if !is_root_session(&session_id) {
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
            if !is_root_session(&session_id) {
                return;
            }
            cli_frontend_set_phase(&runtime.frontend_projection, CliFrontendPhase::Idle, None);
            cli_refresh_prompt(runtime);
        }
        CliServerEvent::SessionRetrying { session_id } => {
            if !is_root_session(&session_id) {
                return;
            }
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning("Retrying…")),
                style,
            );
        }
        CliServerEvent::QuestionCreated {
            request_id,
            session_id,
            ..
        } => {
            // Handled inline in the REPL loop (needs async). Should not reach here.
            tracing::warn!(
                request_id,
                session_id,
                "question.created reached sync handler — skipping"
            );
        }
        CliServerEvent::QuestionResolved { request_id } => {
            tracing::debug!(request_id, "question resolved");
        }
        CliServerEvent::PermissionRequested {
            session_id,
            permission_id,
            ..
        } => {
            tracing::warn!(
                session_id,
                permission_id,
                "permission.requested reached sync handler — skipping"
            );
        }
        CliServerEvent::PermissionResolved { permission_id } => {
            tracing::debug!(permission_id, "permission resolved");
        }
        CliServerEvent::ToolCallStarted {
            session_id,
            tool_call_id,
            tool_name,
        } => {
            if !is_related_session(&session_id) {
                return;
            }
            if let Ok(mut topology) = runtime.observed_topology.lock() {
                topology.active = true;
            }
            tracing::debug!(tool_call_id, tool_name, "tool call started");
            if !is_root_session(&session_id) {
                return;
            }
            let status = OutputBlock::Status(StatusBlock::title(format!("⚙ {}", tool_name)));
            if cli_is_root_focused(runtime) {
                let _ = print_block(Some(runtime), status, style);
            } else {
                cli_cache_root_session_block(runtime, &status, style);
            }
        }
        CliServerEvent::ToolCallCompleted {
            session_id,
            tool_call_id,
        } => {
            if !is_related_session(&session_id) {
                return;
            }
            tracing::debug!(tool_call_id, "tool call completed");
        }
        CliServerEvent::ChildSessionAttached {
            parent_id,
            child_id,
        } => {
            if cli_track_child_session(runtime, &parent_id, &child_id) {
                tracing::debug!(parent_id, child_id, "tracked child session");
            }
        }
        CliServerEvent::ChildSessionDetached {
            parent_id,
            child_id,
        } => {
            if cli_untrack_child_session(runtime, &parent_id, &child_id) {
                tracing::debug!(parent_id, child_id, "untracked child session");
            }
        }
        CliServerEvent::OutputBlock {
            session_id,
            id,
            payload,
        } => {
            if !is_related_session(&session_id) {
                return;
            }
            let block_payload = payload.get(wire_keys::BLOCK).unwrap_or(&payload);
            let Some(block) = parse_output_block(block_payload) else {
                tracing::debug!(?id, payload = %block_payload, "failed to parse output_block");
                return;
            };
            if matches!(block, OutputBlock::Reasoning(_))
                && !runtime.show_thinking.load(Ordering::SeqCst)
            {
                return;
            }
            if let Ok(mut topology) = runtime.observed_topology.lock() {
                topology.observe_block(&block);
            }
            if let OutputBlock::SchedulerStage(stage) = &block {
                if let Some(child_id) = stage.child_session_id.as_deref() {
                    let _ = cli_track_child_session(runtime, &session_id, child_id);
                }
            }
            cli_frontend_observe_block(&runtime.frontend_projection, &block);
            if !is_root_session(&session_id) {
                cli_cache_child_session_block(runtime, &session_id, &block, style);
                if focused_session_id.as_deref() == Some(session_id.as_str()) {
                    let _ = print_block(Some(runtime), block, style);
                }
                return;
            }
            match &block {
                OutputBlock::SchedulerStage(stage)
                    if !cli_should_emit_scheduler_stage_block(
                        &runtime.scheduler_stage_snapshots,
                        stage,
                    ) => {}
                OutputBlock::SchedulerStage(stage)
                    if !cli_is_terminal_stage_status(stage.status.as_deref()) =>
                {
                    if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        projection.active_stage = Some(stage.as_ref().clone());
                        projection.active_collapsed = false;
                    }
                    cli_refresh_prompt(runtime);
                }
                OutputBlock::SchedulerStage(_) => {
                    if let Ok(mut projection) = runtime.frontend_projection.lock() {
                        projection.active_stage = None;
                        projection.active_collapsed = true;
                    }
                    cli_refresh_prompt(runtime);
                    cli_cache_root_session_block(runtime, &block, style);
                    if cli_is_root_focused(runtime) {
                        let _ = print_block(Some(runtime), block, style);
                    }
                }
                _ => {
                    cli_cache_root_session_block(runtime, &block, style);
                    if cli_is_root_focused(runtime) {
                        let _ = print_block(Some(runtime), block, style);
                    }
                }
            }
        }
        CliServerEvent::Error {
            session_id,
            error,
            message_id,
            done,
        } => {
            if !is_related_session(&session_id) {
                return;
            }
            if !is_root_session(&session_id) {
                tracing::error!(session_id, error, ?message_id, ?done, "child session error");
                return;
            }
            tracing::error!(error, ?message_id, ?done, "server error");
            let status = OutputBlock::Status(StatusBlock::error(error));
            if cli_is_root_focused(runtime) {
                let _ = print_block(Some(runtime), status, style);
            } else {
                cli_cache_root_session_block(runtime, &status, style);
            }
        }
        CliServerEvent::Usage {
            session_id,
            prompt_tokens,
            completion_tokens,
            message_id,
        } => {
            if !is_related_session(&session_id) {
                return;
            }
            tracing::debug!(prompt_tokens, completion_tokens, ?message_id, "token usage");
            if let Ok(mut projection) = runtime.frontend_projection.lock() {
                projection.token_stats.input_tokens = projection
                    .token_stats
                    .input_tokens
                    .saturating_add(prompt_tokens);
                projection.token_stats.output_tokens = projection
                    .token_stats
                    .output_tokens
                    .saturating_add(completion_tokens);
            }
            if !is_root_session(&session_id) {
                return;
            }
            if prompt_tokens > 0 || completion_tokens > 0 {
                let status = OutputBlock::Status(StatusBlock::success(format!(
                    "tokens: prompt={} completion={}",
                    prompt_tokens, completion_tokens
                )));
                if cli_is_root_focused(runtime) {
                    let _ = print_block(Some(runtime), status, style);
                } else {
                    cli_cache_root_session_block(runtime, &status, style);
                }
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
    let questions: Vec<rocode_tool::QuestionDef> =
        match serde_json::from_value(questions_json.clone()) {
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
    let guard = runtime
        .spinner_guard
        .lock()
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

async fn handle_permission_from_sse(
    runtime: &CliExecutionRuntime,
    api_client: &Arc<CliApiClient>,
    permission_id: &str,
    info_json: &serde_json::Value,
) {
    let info: crate::api_client::PermissionRequestInfo =
        match serde_json::from_value(info_json.clone()) {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(permission_id, %error, "failed to parse permission info from SSE");
                let _ = api_client
                    .reply_permission(
                        permission_id,
                        PermissionReplyWire::Reject.as_str(),
                        Some("Invalid permission request payload".to_string()),
                    )
                    .await;
                return;
            }
        };

    let input = info.input.as_object().cloned().unwrap_or_default();
    let permission = input
        .get(permission_keys::REQUEST_PERMISSION)
        .and_then(|value| value.as_str())
        .unwrap_or(info.tool.as_str())
        .to_string();
    let patterns = input
        .get(permission_keys::REQUEST_PATTERNS)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let metadata = input
        .get(permission_keys::REQUEST_METADATA)
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    {
        let memory = runtime.permission_memory.lock().await;
        if memory.is_granted(&permission, &patterns) {
            let _ = api_client
                .reply_permission(
                    permission_id,
                    PermissionReplyWire::Once.as_str(),
                    Some("auto-approved".to_string()),
                )
                .await;
            return;
        }
    }

    let guard = runtime
        .spinner_guard
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| SpinnerGuard::noop());
    guard.pause();

    let decision = {
        let permission = permission.clone();
        let patterns = patterns.clone();
        let metadata = metadata.clone();
        tokio::task::spawn_blocking(move || {
            let style = CliStyle::detect();
            prompt_permission(&permission, &patterns, &metadata, &style)
        })
        .await
    };

    guard.resume();

    let decision = match decision {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => {
            tracing::error!(permission_id, %error, "permission prompt IO error");
            let _ = api_client
                .reply_permission(
                    permission_id,
                    PermissionReplyWire::Reject.as_str(),
                    Some(format!("Permission prompt IO error: {}", error)),
                )
                .await;
            return;
        }
        Err(error) => {
            tracing::error!(permission_id, %error, "permission prompt task failed");
            let _ = api_client
                .reply_permission(
                    permission_id,
                    PermissionReplyWire::Reject.as_str(),
                    Some(format!("Permission prompt failed: {}", error)),
                )
                .await;
            return;
        }
    };

    let (reply, message) = match decision {
        PermissionDecision::Allow => (PermissionReplyWire::Once, Some("approved".to_string())),
        PermissionDecision::AllowAlways => {
            let mut memory = runtime.permission_memory.lock().await;
            memory.grant_always(&permission, &patterns);
            (
                PermissionReplyWire::Always,
                Some("approved always".to_string()),
            )
        }
        PermissionDecision::Deny => (PermissionReplyWire::Reject, Some("rejected".to_string())),
    };

    if let Err(error) = api_client
        .reply_permission(permission_id, reply.as_str(), message)
        .await
    {
        tracing::error!(permission_id, %error, "failed to reply permission");
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

/// Handle a `session.updated` SSE event by refreshing cheap metadata only.
async fn handle_session_updated_from_sse(
    runtime: &CliExecutionRuntime,
    api_client: &Arc<CliApiClient>,
    session_id: &str,
    source: Option<&str>,
    _style: &CliStyle,
) {
    let server_sid = match runtime.server_session_id.as_deref() {
        Some(sid) if sid == session_id => sid,
        _ => return, // Not our session.
    };
    let should_refresh = cli_session_update_requires_refresh(source);
    if !should_refresh {
        return;
    }
    match api_client.get_session(server_sid).await {
        Ok(session) => {
            if let Ok(mut projection) = runtime.frontend_projection.lock() {
                projection.session_title = Some(session.title);
            }
        }
        Err(error) => {
            tracing::debug!(
                "Failed to refresh session title after session.updated: {}",
                error
            );
        }
    }
    cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(server_sid)).await;
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
            style.dim("Send a prompt first, then use /recover"),
        ];
        let _ = print_cli_list_on_surface(Some(runtime), "Recovery Actions", None, &lines, &style);
        return;
    }
    let mut lines = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        lines.push(format!(
            "{}  {} {}",
            style.bold(&format!("{}.", index + 1)),
            action.label,
            style.dim(&format!("[{}]", action.key)),
        ));
        lines.push(format!("   {}", style.dim(&action.description)));
    }
    let _ = print_cli_list_on_surface(
        Some(runtime),
        "Recovery Actions",
        Some("Use /recover <number|key> to execute"),
        &lines,
        &style,
    );
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
    actions.into_iter().find(|action| action.key == normalized)
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

    // Ensure terminal is in a clean state for the interactive selector:
    // disable raw mode (the selector will re-enable it), show cursor, and
    // clear any leftover retained-layout artifacts below the current line.
    {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(
            stdout,
            crossterm::cursor::Show,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
        );
        let _ = stdout.flush();
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

        // Run the interactive selector on a dedicated blocking thread so it
        // doesn't block the tokio worker.  crossterm::event::read() is a
        // synchronous blocking call that must not run on an async executor.
        let q_question = q.question.clone();
        let q_header = q.header.clone();
        let q_multiple = q.multiple;
        let style_clone = style.clone();
        let result = tokio::task::spawn_blocking(move || {
            tracing::info!(
                question = %q_question,
                options_count = options.len(),
                multiple = q_multiple,
                style_color = style_clone.color,
                "CLI question: presenting selector"
            );
            if options.is_empty() {
                // No options — free text input
                prompt_free_text(&q_question, q_header.as_deref(), &style_clone)
            } else if q_multiple {
                interactive_multi_select(&q_question, q_header.as_deref(), &options, &style_clone)
            } else {
                interactive_select(&q_question, q_header.as_deref(), &options, &style_clone)
            }
        })
        .await
        .unwrap_or_else(|e| Err(io::Error::other(format!("Selector task panicked: {}", e))));

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
                    topology.finish_question(QuestionInteractionStatus::Cancelled.as_str());
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
                    topology.finish_question(QuestionInteractionStatus::Error.as_str());
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
        topology.finish_question(QuestionInteractionStatus::Answered.as_str());
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

fn cli_list_tasks(runtime: Option<&CliExecutionRuntime>) {
    let style = CliStyle::detect();
    let tasks = global_task_registry().list();
    if tasks.is_empty() {
        let _ = print_cli_list_on_surface(
            runtime,
            "Agent Tasks",
            None,
            &[style.dim("No agent tasks.")],
            &style,
        );
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let mut lines = Vec::new();
    let mut running = 0usize;
    let mut done = 0usize;
    for task in &tasks {
        let (icon, status_str) = match &task.status {
            AgentTaskStatus::Pending => ("◯", task.status.kind().as_str().to_string()),
            AgentTaskStatus::Running { step } => {
                running += 1;
                let steps = task
                    .max_steps
                    .map(|m| format!("{}/{}", step, m))
                    .unwrap_or(format!("{}/？", step));
                ("◐", format!("{}  {}", task.status.kind().as_str(), steps))
            }
            AgentTaskStatus::Completed { steps } => {
                done += 1;
                ("●", format!("{}  {}", task.status.kind().as_str(), steps))
            }
            AgentTaskStatus::Cancelled => {
                done += 1;
                ("✗", task.status.kind().as_str().to_string())
            }
            AgentTaskStatus::Failed { .. } => {
                done += 1;
                ("✗", task.status.kind().as_str().to_string())
            }
        };
        let elapsed = now - task.started_at;
        let elapsed_str = if elapsed < 60 {
            format!("{}s ago", elapsed)
        } else {
            format!("{}m ago", elapsed / 60)
        };
        lines.push(format!(
            "{}  {}  {:<20} {:<16} {}",
            icon, task.id, task.agent_name, status_str, elapsed_str
        ));
    }
    let footer = format!("{} running, {} finished", running, done);
    let _ = print_cli_list_on_surface(runtime, "Agent Tasks", Some(&footer), &lines, &style);
}

fn cli_show_task(id: &str, runtime: Option<&CliExecutionRuntime>) {
    let style = CliStyle::detect();
    match global_task_registry().get(id) {
        Some(task) => {
            let (status_label, step_info) = match &task.status {
                AgentTaskStatus::Pending => {
                    (task.status.kind().as_str().to_string(), String::new())
                }
                AgentTaskStatus::Running { step } => {
                    let steps = task
                        .max_steps
                        .map(|m| format!(" (step {}/{})", step, m))
                        .unwrap_or(format!(" (step {}/?)", step));
                    (task.status.kind().as_str().to_string(), steps)
                }
                AgentTaskStatus::Completed { steps } => (
                    task.status.kind().as_str().to_string(),
                    format!(" ({} steps)", steps),
                ),
                AgentTaskStatus::Cancelled => {
                    (task.status.kind().as_str().to_string(), String::new())
                }
                AgentTaskStatus::Failed { error } => (
                    format!("{}: {}", task.status.kind().as_str(), error),
                    String::new(),
                ),
            };
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - task.started_at;
            let elapsed_str = if elapsed < 60 {
                format!("{}s ago", elapsed)
            } else {
                format!("{}m ago", elapsed / 60)
            };
            let mut lines = vec![
                format!("{} {}{}", style.bold("Status:"), status_label, step_info),
                format!("{} {}", style.bold("Started:"), elapsed_str),
                format!("{} {}", style.bold("Prompt:"), task.prompt),
            ];
            if !task.output_tail.is_empty() {
                lines.push(String::new());
                lines.push(style.bold("Recent output:"));
                for line in &task.output_tail {
                    lines.push(format!("  {}", line));
                }
            }
            let title = format!("Task {} — {}", task.id, task.agent_name);
            let _ = print_cli_list_on_surface(runtime, &title, None, &lines, &style);
        }
        None => {
            let lines = vec![format!("Task \"{}\" not found", id)];
            let _ = print_cli_list_on_surface(runtime, "Task Detail", None, &lines, &style);
        }
    }
}

fn cli_kill_task(id: &str, runtime: Option<&CliExecutionRuntime>) {
    let style = CliStyle::detect();
    match rocode_orchestrator::global_lifecycle().cancel_task(id) {
        Ok(()) => {
            let lines = vec![format!(
                "{} Task {} cancelled",
                style.bold_green(style.check()),
                id
            )];
            let _ = print_cli_list_on_surface(runtime, "Task Cancel", None, &lines, &style);
        }
        Err(err) => {
            let lines = vec![format!("{} {}", style.bold_red(style.cross()), err)];
            let _ = print_cli_list_on_surface(runtime, "Task Cancel", None, &lines, &style);
        }
    }
}

// ── CLI session listing ─────────────────────────────────────────────

async fn cli_list_sessions(runtime: Option<&CliExecutionRuntime>) {
    let style = CliStyle::detect();

    let db = match rocode_storage::Database::new().await {
        Ok(db) => db,
        Err(e) => {
            let lines = vec![format!("Failed to open session database: {}", e)];
            let _ = print_cli_list_on_surface(runtime, "Sessions", None, &lines, &style);
            return;
        }
    };

    let session_repo = rocode_storage::SessionRepository::new(db.pool().clone());

    let sessions = match session_repo.list(None, 20).await {
        Ok(sessions) => sessions,
        Err(e) => {
            let lines = vec![format!("Failed to list sessions: {}", e)];
            let _ = print_cli_list_on_surface(runtime, "Sessions", None, &lines, &style);
            return;
        }
    };

    let lines: Vec<String> = if sessions.is_empty() {
        vec![style.dim("No sessions found.")]
    } else {
        sessions
            .iter()
            .map(|session| {
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
                format!("{} {} {}", style.dim(id_short), title, style.dim(&time_str))
            })
            .collect()
    };

    let _ = print_cli_list_on_surface(
        runtime,
        "Recent Sessions",
        Some("Use --continue to resume a previous session."),
        &lines,
        &style,
    );
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

#[cfg(test)]
mod tests {
    use super::scheduler_keys;
    use super::session_keys;
    use super::{
        cli_cycle_child_session, cli_focus_child_session, cli_focus_root_session,
        cli_prompt_agent_override, cli_prompt_assist_view, cli_prompt_screen_lines,
        cli_recent_session_info_for_directory, cli_render_retained_layout,
        cli_render_startup_banner, cli_resolve_registry_ui_action, cli_resolve_show_thinking,
        cli_session_update_requires_refresh, cli_should_emit_scheduler_stage_block,
        CliExecutionRuntime, CliFrontendPhase, CliFrontendProjection, CliObservedExecutionTopology,
        CliPromptCatalog, CliPromptSelectionState, CliRecentSessionInfo, CliRetainedTranscript,
        CliSessionTokenStats, PermissionMemory,
    };
    use crate::api_client::SessionInfo;
    use chrono::Utc;
    use rocode_command::cli_style::CliStyle;
    use rocode_command::output_blocks::SchedulerStageBlock;
    use rocode_command::{CommandRegistry, ResolvedUiCommand, UiActionId, UiCommandArgumentKind};
    use rocode_config::{Config, UiPreferencesConfig};
    use rocode_core::contracts::scheduler::SchedulerStageStatus;
    use rocode_tui::api::SessionTimeInfo;
    use std::collections::{BTreeSet, HashMap, VecDeque};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use tokio::sync::Mutex as AsyncMutex;

    use rocode_command::cli_spinner::SpinnerGuard;

    #[test]
    fn cli_prompt_omits_agent_when_scheduler_profile_is_active() {
        assert_eq!(cli_prompt_agent_override("build", Some("atlas")), None);
        assert_eq!(
            cli_prompt_agent_override("build", None),
            Some("build".to_string())
        );
    }

    #[test]
    fn cli_show_thinking_defaults_to_visible_in_cli() {
        assert!(cli_resolve_show_thinking(false, None, true));
        assert!(cli_resolve_show_thinking(
            false,
            Some(&Config {
                ui_preferences: Some(UiPreferencesConfig {
                    show_thinking: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            true,
        ));
        assert!(cli_resolve_show_thinking(true, None, false));
    }

    fn stage_with_status(status: &str) -> SchedulerStageBlock {
        SchedulerStageBlock {
            stage_id: None,
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
            loop_budget: None,
            available_skill_count: None,
            available_agent_count: None,
            available_category_count: None,
            active_skills: Vec::new(),
            active_agents: Vec::new(),
            active_categories: Vec::new(),
            done_agent_count: 0,
            total_agent_count: 0,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            decision: None,
            child_session_id: None,
        }
    }

    fn test_runtime_with_child_focus_data() -> CliExecutionRuntime {
        let mut root_transcript = CliRetainedTranscript::default();
        root_transcript.append_rendered("● root line\n");

        let mut child_transcript = CliRetainedTranscript::default();
        child_transcript.append_rendered("● child line\n");

        CliExecutionRuntime {
            resolved_agent_name: "build".to_string(),
            resolved_scheduler_profile_name: None,
            resolved_model_label: "openai/gpt-4.1".to_string(),
            observed_topology: Arc::new(Mutex::new(CliObservedExecutionTopology::default())),
            frontend_projection: Arc::new(Mutex::new(CliFrontendProjection {
                transcript: root_transcript.clone(),
                ..Default::default()
            })),
            scheduler_stage_snapshots: Arc::new(Mutex::new(HashMap::new())),
            terminal_surface: None,
            prompt_chrome: None,
            prompt_session: None,
            prompt_session_slot: Arc::new(std::sync::Mutex::new(None)),
            queued_inputs: Arc::new(AsyncMutex::new(VecDeque::new())),
            busy_flag: Arc::new(AtomicBool::new(false)),
            exit_requested: Arc::new(AtomicBool::new(false)),
            active_abort: Arc::new(AsyncMutex::new(None)),
            recovery_base_prompt: None,
            spinner_guard: Arc::new(std::sync::Mutex::new(SpinnerGuard::noop())),
            api_client: None,
            server_session_id: Some("root-session".to_string()),
            related_session_ids: Arc::new(Mutex::new(BTreeSet::from([
                "root-session".to_string(),
                "child-session-a".to_string(),
            ]))),
            root_session_transcript: Arc::new(Mutex::new(root_transcript)),
            child_session_transcripts: Arc::new(Mutex::new(HashMap::from([(
                "child-session-a".to_string(),
                child_transcript,
            )]))),
            focused_session_id: Arc::new(Mutex::new(None)),
            permission_memory: Arc::new(AsyncMutex::new(PermissionMemory::new())),
            show_thinking: Arc::new(AtomicBool::new(true)),
        }
    }

    fn test_runtime_with_multiple_child_sessions() -> CliExecutionRuntime {
        let runtime = test_runtime_with_child_focus_data();
        runtime
            .related_session_ids
            .lock()
            .expect("related session ids")
            .insert("child-session-b".to_string());
        runtime
            .child_session_transcripts
            .lock()
            .expect("child transcripts")
            .insert("child-session-b".to_string(), {
                let mut transcript = CliRetainedTranscript::default();
                transcript.append_rendered("● second child line\n");
                transcript
            });
        runtime
    }

    #[test]
    fn cli_prints_scheduler_stage_snapshots_only_on_change() {
        let snapshots = Arc::new(Mutex::new(HashMap::new()));
        let running = stage_with_status(SchedulerStageStatus::Running.as_str());
        let done = stage_with_status(SchedulerStageStatus::Done.as_str());

        assert!(cli_should_emit_scheduler_stage_block(&snapshots, &running));
        assert!(!cli_should_emit_scheduler_stage_block(&snapshots, &running));
        assert!(cli_should_emit_scheduler_stage_block(&snapshots, &done));
    }

    #[test]
    fn registry_ui_action_resolves_shared_cli_slash_aliases() {
        let registry = CommandRegistry::new();

        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/share"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::ShareSession,
                argument_kind: UiCommandArgumentKind::None,
                argument: None,
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/unshare"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::UnshareSession,
                argument_kind: UiCommandArgumentKind::None,
                argument: None,
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/palette"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::ToggleCommandPalette,
                argument_kind: UiCommandArgumentKind::None,
                argument: None,
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/copy"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::CopySession,
                argument_kind: UiCommandArgumentKind::None,
                argument: None,
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/rename demo"),
            None
        );
    }

    #[test]
    fn registry_ui_action_resolves_parameterized_shared_cli_commands() {
        let registry = CommandRegistry::new();

        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/model openai/gpt-5"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenModelList,
                argument_kind: UiCommandArgumentKind::ModelRef,
                argument: Some("openai/gpt-5".to_string()),
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/agent build"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenAgentList,
                argument_kind: UiCommandArgumentKind::AgentRef,
                argument: Some("build".to_string()),
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/preset atlas"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenPresetList,
                argument_kind: UiCommandArgumentKind::PresetRef,
                argument: Some("atlas".to_string()),
            })
        );
        assert_eq!(
            cli_resolve_registry_ui_action(&registry, "/session abc123"),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenSessionList,
                argument_kind: UiCommandArgumentKind::SessionTarget,
                argument: Some("abc123".to_string()),
            })
        );
    }

    #[test]
    fn retained_transcript_merges_partial_lines() {
        let mut transcript = CliRetainedTranscript::default();
        transcript.append_rendered("● hello");
        transcript.append_rendered(" world\n");
        transcript.append_rendered("next line\n");

        assert_eq!(
            transcript.committed_lines,
            vec!["● hello world", "next line"]
        );
        assert!(transcript.open_line.is_empty());
        assert_eq!(transcript.rendered_text(), "● hello world\nnext line\n");
    }

    #[test]
    fn focus_child_session_switches_visible_transcript_but_keeps_root_session() {
        let runtime = test_runtime_with_child_focus_data();

        assert!(cli_focus_child_session(&runtime, "child-session-a").expect("focus child session"));

        let visible = runtime
            .frontend_projection
            .lock()
            .expect("frontend projection")
            .transcript
            .rendered_text();
        assert_eq!(visible, "● child line\n");
        assert_eq!(runtime.server_session_id.as_deref(), Some("root-session"));
        assert_eq!(
            runtime
                .focused_session_id
                .lock()
                .expect("focused session")
                .as_deref(),
            Some("child-session-a")
        );
        assert_eq!(
            runtime
                .frontend_projection
                .lock()
                .expect("frontend projection")
                .view_label
                .as_deref(),
            Some("view child child-se")
        );

        assert!(cli_focus_root_session(&runtime).expect("back to root session"));
        let visible = runtime
            .frontend_projection
            .lock()
            .expect("frontend projection")
            .transcript
            .rendered_text();
        assert_eq!(visible, "● root line\n");
        assert_eq!(
            runtime
                .focused_session_id
                .lock()
                .expect("focused session")
                .as_deref(),
            None
        );
        assert_eq!(
            runtime
                .frontend_projection
                .lock()
                .expect("frontend projection")
                .view_label,
            None
        );
    }

    #[test]
    fn cycle_child_session_moves_forward_and_backward() {
        let runtime = test_runtime_with_multiple_child_sessions();

        let first = cli_cycle_child_session(&runtime, true)
            .expect("cycle next from root")
            .expect("first child");
        assert_eq!(first.0, "child-session-a");
        assert_eq!((first.1, first.2), (1, 2));

        let second = cli_cycle_child_session(&runtime, true)
            .expect("cycle next from first")
            .expect("second child");
        assert_eq!(second.0, "child-session-b");
        assert_eq!((second.1, second.2), (2, 2));

        let previous = cli_cycle_child_session(&runtime, false)
            .expect("cycle prev from second")
            .expect("previous child");
        assert_eq!(previous.0, "child-session-a");
        assert_eq!((previous.1, previous.2), (1, 2));
    }

    #[test]
    fn cli_prompt_screen_lines_are_empty_for_transcript_first_mode() {
        assert!(cli_prompt_screen_lines().is_empty());
    }

    #[test]
    fn prompt_assist_completes_switch_command_names() {
        let catalog = CliPromptCatalog {
            models: vec!["openai/gpt-4.1".to_string()],
            agents: vec!["build".to_string()],
            presets: vec!["prometheus".to_string()],
        };
        let selection = CliPromptSelectionState {
            model: "openai/gpt-4.1".to_string(),
            agent: "build".to_string(),
            preset: Some("prometheus".to_string()),
        };

        let assist = cli_prompt_assist_view(&catalog, &selection, "/mo", 3);

        assert!(assist
            .screen_lines
            .iter()
            .any(|line| line.contains("/model")));
        assert_eq!(
            assist.completion,
            Some(rocode_command::cli_prompt::PromptCompletion {
                line: "/model ".to_string(),
                cursor_pos: 7,
            })
        );
    }

    #[test]
    fn prompt_assist_filters_model_candidates() {
        let catalog = CliPromptCatalog {
            models: vec![
                "anthropic/claude-3.7-sonnet".to_string(),
                "dashscope/qwen-max".to_string(),
                "dashscope/qwen-plus".to_string(),
            ],
            agents: vec!["build".to_string()],
            presets: vec!["prometheus".to_string()],
        };
        let selection = CliPromptSelectionState {
            model: "dashscope/qwen-plus".to_string(),
            agent: "build".to_string(),
            preset: Some("prometheus".to_string()),
        };

        let assist = cli_prompt_assist_view(&catalog, &selection, "/model qwen", 11);

        assert!(assist
            .screen_lines
            .iter()
            .any(|line| line.contains("dashscope/qwen-max")));
        assert!(assist
            .screen_lines
            .iter()
            .any(|line| line.contains("dashscope/qwen-plus [active]")));
        assert_eq!(
            assist.completion,
            Some(rocode_command::cli_prompt::PromptCompletion {
                line: "/model dashscope/qwen-max".to_string(),
                cursor_pos: 25,
            })
        );
    }

    #[test]
    fn prompt_assist_shows_preset_values_after_exact_command() {
        let catalog = CliPromptCatalog {
            models: vec!["openai/gpt-4.1".to_string()],
            agents: vec!["build".to_string()],
            presets: vec!["atlas".to_string(), "prometheus".to_string()],
        };
        let selection = CliPromptSelectionState {
            model: "openai/gpt-4.1".to_string(),
            agent: "build".to_string(),
            preset: Some("atlas".to_string()),
        };

        let assist = cli_prompt_assist_view(&catalog, &selection, "/preset", 7);

        assert!(assist
            .screen_lines
            .iter()
            .any(|line| line.contains("/preset suggestions")));
        assert_eq!(
            assist.completion,
            Some(rocode_command::cli_prompt::PromptCompletion {
                line: "/preset ".to_string(),
                cursor_pos: 8,
            })
        );
    }

    #[test]
    fn startup_banner_uses_recent_session_metadata() {
        let now = Utc::now().timestamp_millis();
        let sessions = vec![SessionInfo {
            id: "s1".to_string(),
            slug: "s1".to_string(),
            project_id: "p1".to_string(),
            directory: "/tmp/project".to_string(),
            parent_id: None,
            title: "Research Session".to_string(),
            version: "v1".to_string(),
            time: SessionTimeInfo {
                created: now,
                updated: now,
                compacting: None,
                archived: None,
            },
            revert: None,
            metadata: Some(HashMap::from([
                (
                    session_keys::MODEL_PROVIDER.to_string(),
                    serde_json::json!("zhipuai"),
                ),
                (
                    session_keys::MODEL_ID.to_string(),
                    serde_json::json!("GLM-5"),
                ),
                (
                    scheduler_keys::PROFILE.to_string(),
                    serde_json::json!("prometheus"),
                ),
            ])),
        }];
        let info = cli_recent_session_info_for_directory(&sessions, Path::new("/tmp/project"))
            .expect("recent session info");
        assert_eq!(
            info,
            CliRecentSessionInfo {
                title: Some("Research Session".to_string()),
                model_label: Some("zhipuai/GLM-5".to_string()),
                preset_label: Some("prometheus".to_string()),
            }
        );

        let banner = cli_render_startup_banner(&CliStyle::plain(), Some(&info));
        assert!(banner.contains("ROCode"));
        assert!(banner.contains("Research Session"));
        assert!(banner.contains("zhipuai/GLM-5"));
        assert!(banner.contains("prometheus"));
    }

    #[test]
    fn retained_layout_emits_session_messages_sidebar_and_active_boxes() {
        let style = CliStyle::plain();
        let mut projection = CliFrontendProjection {
            phase: CliFrontendPhase::Busy,
            active_label: Some("assistant response".to_string()),
            view_label: Some("view child child-abc".to_string()),
            queue_len: 2,
            active_stage: Some(stage_with_status(SchedulerStageStatus::Running.as_str())),
            transcript: CliRetainedTranscript::default(),
            sidebar_collapsed: false,
            active_collapsed: false,
            session_title: Some("Test Session".to_string()),
            scroll_offset: 0,
            token_stats: CliSessionTokenStats::default(),
            mcp_servers: Vec::new(),
            lsp_servers: Vec::new(),
        };
        projection
            .transcript
            .append_rendered("● user prompt\n\n● assistant reply\n");
        let topology = CliObservedExecutionTopology {
            active: true,
            ..Default::default()
        };

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
        assert!(joined.contains("view child child-abc"));
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
        assert!(!joined.contains("╭ Sidebar"));
        assert!(joined.contains("Active"));
    }

    #[test]
    fn footer_text_surfaces_child_focus_state() {
        let projection = CliFrontendProjection {
            phase: CliFrontendPhase::Busy,
            view_label: Some("view child abcd1234".to_string()),
            ..Default::default()
        };

        let footer = projection.footer_text();

        assert!(footer.contains("Busy"));
        assert!(footer.contains("view child abcd1234"));
        assert!(footer.contains("/child"));
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

    #[test]
    fn retained_layout_active_panel_adapts_to_content() {
        let style = CliStyle::plain();
        let topology = CliObservedExecutionTopology::default();
        let minimal_stage = stage_with_status(SchedulerStageStatus::Running.as_str());

        let proj_minimal = CliFrontendProjection {
            phase: CliFrontendPhase::Busy,
            active_stage: Some(minimal_stage),
            sidebar_collapsed: true,
            active_collapsed: false,
            session_title: Some("Test".to_string()),
            ..Default::default()
        };
        let lines_minimal = cli_render_retained_layout(
            "Agent build",
            "Model auto",
            "~/test",
            &proj_minimal,
            &topology,
            &style,
        );

        let mut rich_stage = stage_with_status(SchedulerStageStatus::Running.as_str());
        rich_stage.focus = Some("analyzing codebase".to_string());
        rich_stage.last_event = Some("tool_call: read_file".to_string());
        rich_stage.activity = Some("Reviewing architecture".to_string());
        rich_stage.available_skill_count = Some(12);
        rich_stage.available_agent_count = Some(4);
        rich_stage.active_skills = vec!["planner".to_string(), "reviewer".to_string()];
        rich_stage.total_agent_count = 3;
        rich_stage.done_agent_count = 1;
        rich_stage.child_session_id = Some("child-abc".to_string());

        let proj_rich = CliFrontendProjection {
            phase: CliFrontendPhase::Busy,
            active_stage: Some(rich_stage),
            sidebar_collapsed: true,
            active_collapsed: false,
            session_title: Some("Test".to_string()),
            ..Default::default()
        };
        let lines_rich = cli_render_retained_layout(
            "Agent build",
            "Model auto",
            "~/test",
            &proj_rich,
            &topology,
            &style,
        );

        assert!(
            lines_rich.len() > lines_minimal.len(),
            "Rich active panel ({} lines) should be taller than minimal ({} lines)",
            lines_rich.len(),
            lines_minimal.len(),
        );

        let joined_rich = lines_rich.join("\n");
        assert!(joined_rich.contains("Active"));
        assert!(joined_rich.contains("child-abc"));
        assert!(joined_rich.contains("planner"));
    }

    #[test]
    fn session_updated_refresh_allowlist_is_explicit() {
        assert!(cli_session_update_requires_refresh(Some("prompt.final")));
        assert!(cli_session_update_requires_refresh(Some("stream.final")));
        assert!(cli_session_update_requires_refresh(Some(
            "prompt.completed"
        )));
        assert!(cli_session_update_requires_refresh(Some(
            "session.title.set"
        )));
        assert!(!cli_session_update_requires_refresh(Some(
            "prompt.scheduler.stage.content"
        )));
        assert!(!cli_session_update_requires_refresh(Some("prompt.stream")));
        assert!(!cli_session_update_requires_refresh(None));
    }
}
