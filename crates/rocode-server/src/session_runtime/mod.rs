pub(crate) mod events;
pub(crate) mod state;

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use self::events::{
    broadcast_child_session_attached, broadcast_child_session_detached, broadcast_server_event,
    broadcast_session_updated, emit_output_block_via_hook, DiffEntry, ServerEvent,
};
use crate::runtime_control::{ExecutionPatch, ExecutionStatus, FieldUpdate};
use crate::ServerState;
use rocode_command::output_blocks::{
    MessageBlock, MessageRole as OutputMessageRole, OutputBlock, ReasoningBlock,
    SchedulerDecisionBlock, SchedulerDecisionField, SchedulerDecisionRenderSpec,
    SchedulerDecisionSection, SchedulerStageBlock,
};
use rocode_core::contracts::output_blocks::{
    keys as output_keys, scheduler_decision_spec_keys as output_decision_spec_keys,
};
use rocode_core::contracts::patch::keys as patch_keys;
use rocode_core::contracts::scheduler::{
    decision_keys as scheduler_decision_keys, gate_keys as scheduler_gate_keys,
    keys as scheduler_keys, SchedulerDecisionFieldLabelEmphasis, SchedulerDecisionFieldOrder,
    SchedulerDecisionKind, SchedulerDecisionRenderSpecVersion, SchedulerDecisionSectionSpacing,
    SchedulerDecisionStatusPalette, SchedulerDecisionUpdatePolicy, SchedulerStageName,
    SchedulerStageStatus, SchedulerStageWaitingOn,
};
use rocode_core::contracts::todo::{keys as todo_keys, TodoPriority, TodoStatus};
use rocode_core::contracts::tools::BuiltinToolName;
use rocode_core::contracts::wire::fields as wire_fields;
use rocode_orchestrator::{
    parse_execution_gate_decision, parse_route_decision, scheduler_stage_observability,
    ExecutionContext as OrchestratorExecutionContext, LifecycleHook, RouteDecision,
    SchedulerExecutionGateDecision, SchedulerStageCapabilities,
    ToolOutput as OrchestratorToolOutput,
};
use rocode_provider::Provider;
use rocode_session::prompt::{OutputBlockEvent, OutputBlockHook};
use rocode_session::snapshot::Snapshot;
use rocode_session::{MessageRole, MessageUsage, PartType, Session, SessionMessage};

#[derive(Clone)]
struct ActiveStageMessage {
    message_id: String,
    execution_id: String,
    stage_name: String,
    step_count: u32,
    committed_usage: rocode_orchestrator::runtime::events::StepUsage,
    live_usage: rocode_orchestrator::runtime::events::StepUsage,
    /// If this stage creates an isolated child session, its session ID.
    child_session_id: Option<String>,
    /// The assistant message ID within the child session where content flows.
    child_message_id: Option<String>,
    /// Whether a reasoning stream has started for the child-session assistant message.
    child_reasoning_started: bool,
    /// Whether a reasoning stream has started for the main session message.
    reasoning_started: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ModelPricing {
    input_per_million: f64,
    output_per_million: f64,
    /// Per-million cost for cache-read tokens. Falls back to `input_per_million`
    /// when the provider does not publish a separate cache-read price.
    cache_read_per_million: f64,
    /// Per-million cost for cache-write tokens. Falls back to `input_per_million`
    /// when the provider does not publish a separate cache-write price.
    cache_write_per_million: f64,
}

impl ModelPricing {
    pub(crate) fn new(
        input_per_million: f64,
        output_per_million: f64,
        cache_read_per_million: Option<f64>,
        cache_write_per_million: Option<f64>,
    ) -> Self {
        Self {
            input_per_million,
            output_per_million,
            cache_read_per_million: cache_read_per_million.unwrap_or(input_per_million),
            cache_write_per_million: cache_write_per_million.unwrap_or(input_per_million),
        }
    }

    /// Build from the runtime `ModelInfo`.
    ///
    /// The runtime struct currently only carries input/output prices, so cache
    /// prices fall back to the input price — matching the original
    /// `ModelCost::compute` behaviour (`unwrap_or(self.input)`).
    pub(crate) fn from_model_info(info: &rocode_provider::ModelInfo) -> Self {
        Self::new(
            info.cost_per_million_input,
            info.cost_per_million_output,
            None, // cache_read  — no dedicated field on ModelInfo yet
            None, // cache_write — no dedicated field on ModelInfo yet
        )
    }

    /// Compute cost in dollars, identical semantics to the original
    /// `ModelCost::compute` from `rocode_provider::models`.
    pub(crate) fn compute(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> f64 {
        let input_cost = self.input_per_million * (input_tokens as f64) / 1_000_000.0;
        let output_cost = self.output_per_million * (output_tokens as f64) / 1_000_000.0;
        let cache_read_cost =
            self.cache_read_per_million * (cache_read_tokens as f64) / 1_000_000.0;
        let cache_write_cost =
            self.cache_write_per_million * (cache_write_tokens as f64) / 1_000_000.0;
        input_cost + output_cost + cache_read_cost + cache_write_cost
    }
}

#[derive(Clone)]
pub(crate) struct SessionSchedulerLifecycleHook {
    state: Arc<ServerState>,
    session_id: String,
    scheduler_profile: String,
    output_hook: Option<OutputBlockHook>,
    /// Tracks the currently streaming stage messages as a stack.
    active_stage_messages: Arc<Mutex<Vec<ActiveStageMessage>>>,
    /// Model pricing info for cost calculation.
    model_pricing: Option<ModelPricing>,
}

impl SessionSchedulerLifecycleHook {
    pub(crate) fn new(
        state: Arc<ServerState>,
        session_id: String,
        scheduler_profile: String,
    ) -> Self {
        Self {
            state,
            session_id,
            scheduler_profile,
            output_hook: None,
            active_stage_messages: Arc::new(Mutex::new(Vec::new())),
            model_pricing: None,
        }
    }

    pub(crate) fn with_model_pricing(mut self, model_pricing: Option<ModelPricing>) -> Self {
        self.model_pricing = model_pricing;
        self
    }

    pub(crate) fn with_output_hook(mut self, output_hook: Option<OutputBlockHook>) -> Self {
        self.output_hook = output_hook;
        self
    }

    async fn emit_stage_message(
        &self,
        stage_name: &str,
        stage_index: u32,
        stage_total: u32,
        content: &str,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        emit_scheduler_stage_message(SchedulerStageMessageInput {
            state: &self.state,
            session_id: &self.session_id,
            scheduler_profile: &self.scheduler_profile,
            stage_name,
            stage_index,
            stage_total,
            content,
            exec_ctx,
            output_hook: self.output_hook.as_ref(),
        })
        .await;
    }

    async fn update_active_stage_message<F>(&self, mut update: F, _source: &'static str)
    where
        F: FnMut(&mut SessionMessage, &mut ActiveStageMessage),
    {
        // Snapshot the active entry under a brief lock.
        let active = {
            let guard = self.active_stage_messages.lock().await;
            guard.last().cloned()
        };
        let Some(mut active) = active else {
            return;
        };

        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        let mut runtime_patch = None;
        let mut execution_id = None;
        let mut message_snapshot = None;
        if let Some(message) = session.get_message_mut(&active.message_id) {
            update(message, &mut active);
            runtime_patch = Some(stage_execution_patch_from_message(message));
            execution_id = Some(active.execution_id.clone());
            message_snapshot = Some(message.clone());
            session.touch();
            sessions.update(session);
            drop(sessions);

            // Write the updated snapshot back to the canonical entry.
            let mut guard = self.active_stage_messages.lock().await;
            if let Some(last) = guard.last_mut() {
                if last.message_id == active.message_id {
                    *last = active;
                }
            }
        }

        if let Some(message) = message_snapshot.as_ref() {
            self.emit_stage_block(message).await;
        }

        if let (Some(execution_id), Some(patch)) = (execution_id, runtime_patch) {
            self.state
                .runtime_control
                .update_scheduler_stage(&execution_id, patch)
                .await;
        }
    }

    async fn emit_stage_block(&self, message: &SessionMessage) {
        if let Some(block) = scheduler_stage_block_from_message(message) {
            self.emit_realtime_block(OutputBlockEvent {
                session_id: self.session_id.clone(),
                block: OutputBlock::SchedulerStage(Box::new(block)),
                id: Some(message.id.clone()),
            })
            .await;
        }
    }

    async fn emit_realtime_block(&self, event: OutputBlockEvent) {
        emit_output_block_via_hook(self.output_hook.as_ref(), event).await;
    }

    async fn emit_output_block(&self, session_id: String, block: OutputBlock, id: Option<String>) {
        self.emit_realtime_block(OutputBlockEvent {
            session_id,
            block,
            id,
        })
        .await;
    }

    /// Capture a git worktree snapshot and store its hash in the active stage
    /// message metadata under the given key.
    ///
    /// Runs `Snapshot::track()` on the session's worktree directory in a
    /// blocking task to avoid stalling the async runtime.
    async fn track_snapshot(&self, metadata_key: &str) {
        let worktree = {
            let sessions = self.state.sessions.lock().await;
            sessions.get(&self.session_id).map(|s| s.directory.clone())
        };
        let Some(worktree) = worktree else {
            return;
        };

        let worktree_path = std::path::PathBuf::from(&worktree);
        let snapshot_hash =
            tokio::task::spawn_blocking(move || Snapshot::track(&worktree_path)).await;

        let hash = match snapshot_hash {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                tracing::debug!("Snapshot::track() failed: {e}");
                return;
            }
            Err(e) => {
                tracing::debug!("Snapshot::track() task panicked: {e}");
                return;
            }
        };

        let key = metadata_key.to_string();
        self.update_active_stage_message(
            move |message, _active| {
                message
                    .metadata
                    .insert(key.clone(), serde_json::json!(hash));
            },
            "prompt.scheduler.snapshot",
        )
        .await;
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SchedulerAbortInfo {
    pub execution_id: Option<String>,
    pub scheduler_profile: Option<String>,
    pub stage_name: Option<String>,
    pub stage_index: Option<u32>,
}

pub(crate) async fn request_active_scheduler_stage_abort(
    state: &Arc<ServerState>,
    session_id: &str,
) -> Option<SchedulerAbortInfo> {
    let info = update_active_scheduler_stage_message(
        state,
        session_id,
        |message| {
            let info = scheduler_abort_info(message);
            message.metadata.insert(
                scheduler_keys::STATUS.to_string(),
                serde_json::json!(SchedulerStageStatus::Cancelling.as_str()),
            );
            message.metadata.insert(
                scheduler_keys::WAITING_ON.to_string(),
                serde_json::json!(SchedulerStageWaitingOn::None.as_str()),
            );
            message.metadata.insert(
                scheduler_keys::LAST_EVENT.to_string(),
                serde_json::json!("Cancellation requested by user"),
            );
            Some(info)
        },
        "prompt.scheduler.stage.abort.requested",
    )
    .await;
    if let Some(execution_id) = info.as_ref().and_then(|info| info.execution_id.as_deref()) {
        state
            .runtime_control
            .mark_scheduler_stage_cancelling(execution_id)
            .await;
    }
    info
}

pub(crate) async fn finalize_active_scheduler_stage_cancelled(
    state: &Arc<ServerState>,
    session_id: &str,
) -> Option<SchedulerAbortInfo> {
    let info = update_active_scheduler_stage_message(
        state,
        session_id,
        |message| {
            let info = scheduler_abort_info(message);
            message.metadata.remove(scheduler_keys::STREAMING);
            message.metadata.insert(
                scheduler_keys::STATUS.to_string(),
                serde_json::json!(SchedulerStageStatus::Cancelled.as_str()),
            );
            message.metadata.insert(
                scheduler_keys::WAITING_ON.to_string(),
                serde_json::json!(SchedulerStageWaitingOn::None.as_str()),
            );
            message.metadata.insert(
                scheduler_keys::LAST_EVENT.to_string(),
                serde_json::json!("Stage cancelled by user"),
            );
            Some(info)
        },
        "prompt.scheduler.stage.abort.finalized",
    )
    .await;
    if let Some(execution_id) = info.as_ref().and_then(|info| info.execution_id.as_deref()) {
        state
            .runtime_control
            .finish_scheduler_stage(execution_id)
            .await;
    }
    info
}

async fn update_active_scheduler_stage_message<T, F>(
    state: &Arc<ServerState>,
    session_id: &str,
    mut update: F,
    source: &'static str,
) -> Option<T>
where
    F: FnMut(&mut SessionMessage) -> Option<T>,
{
    let mut sessions = state.sessions.lock().await;
    let mut session = sessions.get(session_id).cloned()?;
    let message = find_active_scheduler_stage_message_mut(&mut session)?;
    let result = update(message)?;
    session.touch();
    sessions.update(session);
    drop(sessions);

    broadcast_session_updated(state, session_id.to_string(), source.to_string());
    Some(result)
}

fn find_active_scheduler_stage_message_mut(session: &mut Session) -> Option<&mut SessionMessage> {
    session.messages.iter_mut().rev().find(|message| {
        message.role == MessageRole::Assistant
            && message
                .metadata
                .get(scheduler_keys::EMITTED)
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            && (message
                .metadata
                .get(scheduler_keys::STREAMING)
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                || matches!(
                    message
                        .metadata
                        .get(scheduler_keys::STATUS)
                        .and_then(|value| value.as_str()),
                    Some(status)
                        if matches!(
                            SchedulerStageStatus::parse(status),
                            Some(
                                SchedulerStageStatus::Running
                                    | SchedulerStageStatus::Waiting
                                    | SchedulerStageStatus::Cancelling
                            )
                        )
                ))
    })
}

fn scheduler_abort_info(message: &SessionMessage) -> SchedulerAbortInfo {
    SchedulerAbortInfo {
        execution_id: message
            .metadata
            .get(scheduler_keys::STAGE_ID)
            .and_then(|value| value.as_str())
            .map(str::to_string),
        scheduler_profile: message
            .metadata
            .get(scheduler_keys::PROFILE)
            .and_then(|value| value.as_str())
            .map(str::to_string),
        stage_name: message
            .metadata
            .get(scheduler_keys::STAGE)
            .and_then(|value| value.as_str())
            .map(str::to_string),
        stage_index: message
            .metadata
            .get(scheduler_keys::STAGE_INDEX)
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
    }
}

fn write_stage_usage_totals(
    message: &mut SessionMessage,
    committed_usage: &rocode_orchestrator::runtime::events::StepUsage,
    live_usage: &rocode_orchestrator::runtime::events::StepUsage,
    allow_zero_fields: bool,
    model_pricing: Option<ModelPricing>,
) {
    let prompt_tokens = committed_usage.prompt_tokens + live_usage.prompt_tokens;
    let completion_tokens = committed_usage.completion_tokens + live_usage.completion_tokens;
    let reasoning_tokens = committed_usage.reasoning_tokens + live_usage.reasoning_tokens;
    let cache_read_tokens = committed_usage.cache_read_tokens + live_usage.cache_read_tokens;
    let cache_write_tokens = committed_usage.cache_write_tokens + live_usage.cache_write_tokens;

    let mut has_any_visible_usage = false;
    for (key, value) in [
        (scheduler_keys::PROMPT_TOKENS, prompt_tokens),
        (scheduler_keys::COMPLETION_TOKENS, completion_tokens),
        (scheduler_keys::REASONING_TOKENS, reasoning_tokens),
        (scheduler_keys::CACHE_READ_TOKENS, cache_read_tokens),
        (scheduler_keys::CACHE_WRITE_TOKENS, cache_write_tokens),
    ] {
        if value > 0 || allow_zero_fields {
            has_any_visible_usage = true;
            message
                .metadata
                .insert(key.to_string(), serde_json::json!(value));
        } else {
            message.metadata.remove(key);
        }
    }

    if has_any_visible_usage {
        let total_cost = model_pricing
            .map(|pricing| {
                pricing.compute(
                    prompt_tokens,
                    completion_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                )
            })
            .or_else(|| message.usage.as_ref().map(|u| u.total_cost))
            .unwrap_or(0.0);
        message.usage = Some(MessageUsage {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            reasoning_tokens,
            cache_write_tokens,
            cache_read_tokens,
            total_cost,
        });
    } else {
        message.usage = None;
    }
}

/// Returns `true` for tools that modify files on disk (edit, write, apply_patch).
/// These are the tools that warrant capturing a snapshot after completion.
fn is_file_modifying_tool(tool_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(tool_name),
        Some(
            BuiltinToolName::Write
                | BuiltinToolName::Edit
                | BuiltinToolName::MultiEdit
                | BuiltinToolName::ApplyPatch
        )
    )
}

#[async_trait]
impl LifecycleHook for SessionSchedulerLifecycleHook {
    async fn on_orchestration_start(
        &self,
        _: &str,
        _: Option<u32>,
        _: &OrchestratorExecutionContext,
    ) {
    }

    async fn on_step_start(
        &self,
        _: &str,
        _: &str,
        step_index: u32,
        _: &OrchestratorExecutionContext,
    ) {
        // Capture a "before" snapshot at the first step so compute_diff()
        // can later compare against the final snapshot.
        if step_index == 0 {
            self.track_snapshot("step_start_snapshot").await;
        }

        self.update_active_stage_message(
            |message, active| {
                active.step_count += 1;
                active.live_usage = rocode_orchestrator::runtime::events::StepUsage::default();
                write_stage_usage_totals(
                    message,
                    &active.committed_usage,
                    &active.live_usage,
                    false,
                    None,
                );
                message.metadata.insert(
                    scheduler_keys::STEP.to_string(),
                    serde_json::json!(active.step_count),
                );
                message.metadata.insert(
                    scheduler_keys::STATUS.to_string(),
                    serde_json::json!(SchedulerStageStatus::Running.as_str()),
                );
                message.metadata.insert(
                    scheduler_keys::LAST_EVENT.to_string(),
                    serde_json::json!(format!("Step {} started", active.step_count)),
                );
                message.metadata.insert(
                    scheduler_keys::WAITING_ON.to_string(),
                    serde_json::json!(SchedulerStageWaitingOn::Model.as_str()),
                );
            },
            "prompt.scheduler.stage.step",
        )
        .await;
    }

    async fn on_tool_start(
        &self,
        _: &str,
        tool_call_id: &str,
        tool_name: &str,
        tool_args: &serde_json::Value,
        _: &OrchestratorExecutionContext,
    ) {
        // Register tool call into RuntimeControlRegistry for topology visibility.
        let (parent_id, stage_id) = {
            let guard = self.active_stage_messages.lock().await;
            let pid = guard.last().map(|s| s.execution_id.clone());
            let sid = guard.last().map(|s| s.execution_id.clone());
            (pid, sid)
        };
        self.state
            .runtime_control
            .register_tool_call(
                tool_call_id,
                &self.session_id,
                tool_name,
                parent_id,
                stage_id,
            )
            .await;

        // Update aggregated runtime state.
        self.state
            .runtime_state
            .tool_started(&self.session_id, tool_call_id, tool_name)
            .await;

        self.update_active_stage_message(
            |message, _active| {
                apply_stage_capability_activity_evidence(
                    message,
                    extract_stage_capability_activity(
                        tool_name,
                        StageCapabilityActivitySource::ToolArgs(tool_args),
                    ),
                );
                if let Some(activity) = summarize_tool_activity(tool_name, tool_args) {
                    message.metadata.insert(
                        scheduler_keys::ACTIVITY.to_string(),
                        serde_json::json!(activity),
                    );
                }
                if BuiltinToolName::parse(tool_name) == Some(BuiltinToolName::Question) {
                    message.metadata.insert(
                        scheduler_keys::STATUS.to_string(),
                        serde_json::json!(SchedulerStageStatus::Waiting.as_str()),
                    );
                    message.metadata.insert(
                        scheduler_keys::WAITING_ON.to_string(),
                        serde_json::json!(SchedulerStageWaitingOn::User.as_str()),
                    );
                    message.metadata.insert(
                        scheduler_keys::LAST_EVENT.to_string(),
                        serde_json::json!("Waiting for user answer"),
                    );
                } else {
                    message.metadata.insert(
                        scheduler_keys::STATUS.to_string(),
                        serde_json::json!(SchedulerStageStatus::Running.as_str()),
                    );
                    message.metadata.insert(
                        scheduler_keys::WAITING_ON.to_string(),
                        serde_json::json!(SchedulerStageWaitingOn::Tool.as_str()),
                    );
                    message.metadata.insert(
                        scheduler_keys::LAST_EVENT.to_string(),
                        serde_json::json!(format!(
                            "Tool started: {}",
                            pretty_scheduler_stage_name(tool_name)
                        )),
                    );
                }
            },
            "prompt.scheduler.stage.tool.start",
        )
        .await;

        // Populate the TodoManager when a todowrite tool is invoked so the
        // /session/{id}/todo endpoint returns live data.
        if BuiltinToolName::parse(tool_name) == Some(BuiltinToolName::TodoWrite) {
            if let Some(todos) = extract_todo_items_from_args(tool_args) {
                self.state
                    .todo_manager
                    .update(&self.session_id, todos)
                    .await;
            }
        }
    }

    async fn on_tool_end(
        &self,
        _: &str,
        tool_call_id: &str,
        tool_name: &str,
        tool_output: &OrchestratorToolOutput,
        _: &OrchestratorExecutionContext,
    ) {
        // Remove tool call from RuntimeControlRegistry.
        self.state
            .runtime_control
            .finish_tool_call(tool_call_id)
            .await;

        // Update aggregated runtime state.
        self.state
            .runtime_state
            .tool_ended(&self.session_id, tool_call_id)
            .await;

        self.update_active_stage_message(
            |message, _active| {
                apply_stage_capability_activity_evidence(
                    message,
                    extract_stage_capability_activity(
                        tool_name,
                        StageCapabilityActivitySource::ToolOutput(tool_output),
                    ),
                );
                if let Some(activity) = summarize_tool_result_activity(tool_name, tool_output) {
                    message.metadata.insert(
                        scheduler_keys::ACTIVITY.to_string(),
                        serde_json::json!(activity),
                    );
                }
                message.metadata.insert(
                    scheduler_keys::STATUS.to_string(),
                    serde_json::json!(SchedulerStageStatus::Running.as_str()),
                );
                message.metadata.insert(
                    scheduler_keys::WAITING_ON.to_string(),
                    serde_json::json!(SchedulerStageWaitingOn::Model.as_str()),
                );
                let event = if BuiltinToolName::parse(tool_name) == Some(BuiltinToolName::Question)
                {
                    if tool_output.is_error {
                        "Question tool failed".to_string()
                    } else {
                        "User answer received".to_string()
                    }
                } else if tool_output.is_error {
                    format!("Tool failed: {}", pretty_scheduler_stage_name(tool_name))
                } else {
                    format!("Tool finished: {}", pretty_scheduler_stage_name(tool_name))
                };
                message.metadata.insert(
                    scheduler_keys::LAST_EVENT.to_string(),
                    serde_json::json!(event),
                );
            },
            "prompt.scheduler.stage.tool.end",
        )
        .await;

        // Capture an "after" snapshot when a file-modifying tool completes
        // successfully so that compute_diff() can measure the delta.
        if !tool_output.is_error && is_file_modifying_tool(tool_name) {
            self.track_snapshot("step_finish_snapshot").await;
        }
    }

    async fn on_orchestration_end(&self, _: &str, _: u32, _: &OrchestratorExecutionContext) {
        // Compute cumulative session diffs and persist them.
        // We scan message metadata directly for snapshot hashes that were
        // recorded during step lifecycle events, then call Snapshot::diff_full().
        let session_id = self.session_id.clone();
        let state = self.state.clone();

        let result: Option<()> = async {
            let sessions_guard = state.sessions.lock().await;
            let session = sessions_guard.get(&session_id)?;
            let worktree = session.directory.clone();

            // Find the earliest step_start_snapshot and latest step_finish_snapshot
            // across all messages in the session.
            let mut from_snapshot: Option<String> = None;
            let mut to_snapshot: Option<String> = None;

            for msg in &session.messages {
                if from_snapshot.is_none() {
                    if let Some(s) = msg
                        .metadata
                        .get("step_start_snapshot")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        from_snapshot = Some(s.to_string());
                    }
                }
                if let Some(s) = msg
                    .metadata
                    .get("step_finish_snapshot")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    to_snapshot = Some(s.to_string());
                }
            }

            drop(sessions_guard);

            let (from_hash, to_hash) = match (from_snapshot, to_snapshot) {
                (Some(f), Some(t)) if f != t => (f, t),
                _ => return Some(()), // no snapshots or identical — nothing to diff
            };

            let worktree_path = std::path::PathBuf::from(&worktree);
            let diff_result = tokio::task::spawn_blocking(move || {
                Snapshot::diff_full(&worktree_path, &from_hash, &to_hash)
            })
            .await;

            let file_diffs = match diff_result {
                Ok(Ok(diffs)) => diffs,
                Ok(Err(e)) => {
                    tracing::debug!("Snapshot::diff_full() failed: {e}");
                    return Some(());
                }
                Err(e) => {
                    tracing::debug!("Snapshot::diff_full() task panicked: {e}");
                    return Some(());
                }
            };

            if file_diffs.is_empty() {
                return Some(());
            }

            let summary_diffs: Vec<rocode_session::summary::SummaryFileDiff> = file_diffs
                .iter()
                .map(|d| rocode_session::summary::SummaryFileDiff {
                    file: rocode_session::summary::unquote_git_path(&d.path),
                    additions: d.additions,
                    deletions: d.deletions,
                })
                .collect();

            let summary_data = rocode_session::summary::SessionSummaryData {
                additions: summary_diffs.iter().map(|d| d.additions).sum(),
                deletions: summary_diffs.iter().map(|d| d.deletions).sum(),
                files: summary_diffs.len() as u64,
                diffs: summary_diffs.clone(),
            };

            // Persist summary into session record.
            let mut sessions_guard = state.sessions.lock().await;
            let mut session = sessions_guard.get(&session_id)?.clone();
            rocode_session::summary::set_session_summary(&mut session, &summary_data);
            let _ = rocode_session::summary::persist_session_diffs(
                &mut session,
                &session_id,
                &summary_data.diffs,
            );
            session.touch();
            sessions_guard.update(session);
            drop(sessions_guard);

            // Broadcast canonical diff-updated event for SSE consumers.
            broadcast_server_event(
                state.as_ref(),
                &ServerEvent::DiffUpdated {
                    session_id: session_id.clone(),
                    diff: summary_diffs
                        .iter()
                        .map(|d| DiffEntry {
                            path: d.file.clone(),
                            additions: d.additions,
                            deletions: d.deletions,
                        })
                        .collect(),
                },
            );

            Some(())
        }
        .await;

        if result.is_none() {
            tracing::debug!(
                session_id = %self.session_id,
                "Skipped orchestration-end diff summarization (session not found)"
            );
        }
    }

    async fn on_scheduler_stage_start(
        &self,
        _agent_name: &str,
        stage_name: &str,
        stage_index: u32,
        capabilities: Option<&SchedulerStageCapabilities>,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        let wants_child_session = capabilities.map(|caps| caps.child_session).unwrap_or(false);

        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        // ── Create child session if requested ──
        let (child_session_id, child_message_id) = if wants_child_session {
            let mut child = Session::child(&session);
            child.title = format!(
                "Stage: {} — {}",
                pretty_scheduler_stage_name(stage_name),
                &self.scheduler_profile
            );
            let child_id = child.id.clone();
            let child_msg = child.add_assistant_message();
            let child_msg_id = child_msg.id.clone();
            child_msg.add_text(String::new());
            child.touch();
            sessions.update(child);
            (Some(child_id), Some(child_msg_id))
        } else {
            (None, None)
        };
        if let (Some(child_sid), Some(child_mid)) =
            (child_session_id.as_ref(), child_message_id.as_ref())
        {
            broadcast_child_session_attached(
                &self.state,
                self.session_id.clone(),
                child_sid.clone(),
            );
            // Update aggregated runtime state.
            self.state
                .runtime_state
                .child_attached(&self.session_id, child_sid)
                .await;
            self.emit_output_block(
                child_sid.clone(),
                OutputBlock::Message(MessageBlock::start(OutputMessageRole::Assistant)),
                Some(child_mid.clone()),
            )
            .await;
        }

        let message = session.add_assistant_message();
        let message_id = message.id.clone();
        let execution_id = format!("stage_{}", uuid::Uuid::new_v4().simple());
        message.metadata.insert(
            scheduler_keys::STAGE_ID.to_string(),
            serde_json::json!(&execution_id),
        );
        message.metadata.insert(
            scheduler_keys::PROFILE.to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message.metadata.insert(
            scheduler_keys::RESOLVED_PROFILE.to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message.metadata.insert(
            scheduler_keys::STAGE.to_string(),
            serde_json::json!(stage_name),
        );
        message.metadata.insert(
            scheduler_keys::STAGE_INDEX.to_string(),
            serde_json::json!(stage_index),
        );
        message
            .metadata
            .insert(scheduler_keys::EMITTED.to_string(), serde_json::json!(true));
        message.metadata.insert(
            scheduler_keys::AGENT.to_string(),
            serde_json::json!(&exec_ctx.agent_name),
        );
        message.metadata.insert(
            scheduler_keys::STREAMING.to_string(),
            serde_json::json!(true),
        );
        message.metadata.insert(
            scheduler_keys::STATUS.to_string(),
            serde_json::json!(SchedulerStageStatus::Running.as_str()),
        );
        message.metadata.insert(
            scheduler_keys::FOCUS.to_string(),
            serde_json::json!(scheduler_stage_focus(stage_name)),
        );
        message.metadata.insert(
            scheduler_keys::LAST_EVENT.to_string(),
            serde_json::json!("Stage started"),
        );
        message.metadata.insert(
            scheduler_keys::WAITING_ON.to_string(),
            serde_json::json!(SchedulerStageWaitingOn::Model.as_str()),
        );
        if let Some(observability) =
            scheduler_stage_observability(&self.scheduler_profile, stage_name)
        {
            message.metadata.insert(
                scheduler_keys::PROJECTION.to_string(),
                serde_json::json!(observability.projection),
            );
            message.metadata.insert(
                scheduler_keys::TOOL_POLICY.to_string(),
                serde_json::json!(observability.tool_policy),
            );
            message.metadata.insert(
                scheduler_keys::LOOP_BUDGET.to_string(),
                serde_json::json!(observability.loop_budget),
            );
        }
        // Write per-stage capability pool counts into metadata. Concrete
        // runtime usage is tracked separately from tool invocations.
        if let Some(caps) = capabilities {
            message.metadata.insert(
                scheduler_keys::AVAILABLE_SKILL_COUNT.to_string(),
                serde_json::json!(caps.skill_list.len()),
            );
            message.metadata.insert(
                scheduler_keys::AVAILABLE_AGENT_COUNT.to_string(),
                serde_json::json!(caps.agents.len()),
            );
            message.metadata.insert(
                scheduler_keys::AVAILABLE_CATEGORY_COUNT.to_string(),
                serde_json::json!(caps.categories.len()),
            );
        }
        message.metadata.insert(
            scheduler_keys::ACTIVE_SKILLS.to_string(),
            serde_json::json!(Vec::<String>::new()),
        );
        message.metadata.insert(
            scheduler_keys::ACTIVE_AGENTS.to_string(),
            serde_json::json!(Vec::<String>::new()),
        );
        message.metadata.insert(
            scheduler_keys::ACTIVE_CATEGORIES.to_string(),
            serde_json::json!(Vec::<String>::new()),
        );

        // Store child session reference in metadata for persistence/reconstruction.
        if let Some(ref child_id) = child_session_id {
            message.metadata.insert(
                scheduler_keys::CHILD_SESSION_ID.to_string(),
                serde_json::json!(child_id),
            );
        }

        // Start with an empty body; title is rendered from metadata, not persisted text.
        message.add_text(String::new());

        session.touch();
        sessions.update(session);
        drop(sessions);

        if let Some(snapshot) = {
            let sessions = self.state.sessions.lock().await;
            sessions
                .get(&self.session_id)
                .and_then(|session| session.get_message(&message_id).cloned())
        } {
            self.emit_stage_block(&snapshot).await;
        }

        self.state
            .runtime_control
            .register_scheduler_stage(
                &self.session_id,
                execution_id.clone(),
                pretty_scheduler_stage_name(stage_name),
                scheduler_stage_execution_metadata(
                    &self.scheduler_profile,
                    stage_name,
                    stage_index,
                    None,
                    &exec_ctx.agent_name,
                ),
            )
            .await;

        self.active_stage_messages
            .lock()
            .await
            .push(ActiveStageMessage {
                message_id,
                execution_id,
                stage_name: stage_name.to_string(),
                step_count: 0,
                committed_usage: rocode_orchestrator::runtime::events::StepUsage::default(),
                live_usage: rocode_orchestrator::runtime::events::StepUsage::default(),
                child_session_id,
                child_message_id,
                child_reasoning_started: false,
                reasoning_started: false,
            });
    }

    async fn on_scheduler_stage_content(
        &self,
        stage_name: &str,
        _stage_index: u32,
        content_delta: &str,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
        let (message_id, child_session_id, child_message_id) = {
            let guard = self.active_stage_messages.lock().await;
            match guard.last() {
                Some(active) => (
                    active.message_id.clone(),
                    active.child_session_id.clone(),
                    active.child_message_id.clone(),
                ),
                None => return,
            }
        };

        // If a child session exists, route content there instead of the parent stage message.
        if let (Some(child_sid), Some(child_mid)) = (child_session_id, child_message_id) {
            let mut sessions = self.state.sessions.lock().await;
            if let Some(mut child) = sessions.get(&child_sid).cloned() {
                if let Some(msg) = child.get_message_mut(&child_mid) {
                    msg.append_text(content_delta);
                }
                child.touch();
                sessions.update(child);
            }
            drop(sessions);
            self.emit_output_block(
                child_sid,
                OutputBlock::Message(MessageBlock::delta(
                    OutputMessageRole::Assistant,
                    content_delta.to_string(),
                )),
                Some(child_mid),
            )
            .await;
            return;
        }

        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        let mut message_snapshot = None;
        if let Some(message) = session.get_message_mut(&message_id) {
            message.append_text(content_delta);
            apply_scheduler_decision_metadata(stage_name, message);
            message_snapshot = Some(message.clone());
        }
        session.touch();
        sessions.update(session);
        drop(sessions);

        if let Some(message) = message_snapshot.as_ref() {
            self.emit_stage_block(message).await;
        }
    }

    async fn on_scheduler_stage_reasoning(
        &self,
        stage_name: &str,
        _stage_index: u32,
        reasoning_delta: &str,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
        tracing::debug!(
            session_id = %self.session_id,
            stage_name = %stage_name,
            reasoning_len = reasoning_delta.len(),
            "on_scheduler_stage_reasoning called"
        );

        let (
            message_id,
            child_session_id,
            child_message_id,
            start_child_reasoning,
            start_reasoning,
        ) = {
            let mut guard = self.active_stage_messages.lock().await;
            match guard.last_mut() {
                Some(active) => {
                    let start_child_reasoning = active.child_session_id.is_some()
                        && active.child_message_id.is_some()
                        && !active.child_reasoning_started;
                    if start_child_reasoning {
                        active.child_reasoning_started = true;
                    }
                    // For main session (non-child), track reasoning started
                    let start_reasoning =
                        active.child_session_id.is_none() && !active.reasoning_started;
                    if start_reasoning {
                        active.reasoning_started = true;
                    }
                    (
                        Some(active.message_id.clone()),
                        active.child_session_id.clone(),
                        active.child_message_id.clone(),
                        start_child_reasoning,
                        start_reasoning,
                    )
                }
                None => {
                    // Non-scheduler-stage mode: find current assistant message
                    drop(guard);
                    let sessions = self.state.sessions.lock().await;
                    if let Some(session) = sessions.get(&self.session_id) {
                        if let Some(last_assistant) = session
                            .messages
                            .iter()
                            .rev()
                            .find(|m| m.role == MessageRole::Assistant)
                        {
                            (Some(last_assistant.id.clone()), None, None, false, false)
                        } else {
                            (None, None, None, false, false)
                        }
                    } else {
                        (None, None, None, false, false)
                    }
                }
            }
        };

        let Some(message_id) = message_id else {
            return;
        };

        // If a child session exists, route reasoning there.
        if let (Some(child_sid), Some(child_mid)) = (child_session_id, child_message_id) {
            let mut sessions = self.state.sessions.lock().await;
            if let Some(mut child) = sessions.get(&child_sid).cloned() {
                if let Some(msg) = child.get_message_mut(&child_mid) {
                    msg.add_reasoning(reasoning_delta);
                }
                child.touch();
                sessions.update(child);
            }
            drop(sessions);
            if start_child_reasoning {
                self.emit_output_block(
                    child_sid.clone(),
                    OutputBlock::Reasoning(ReasoningBlock::start()),
                    Some(child_mid.clone()),
                )
                .await;
            }
            self.emit_output_block(
                child_sid.clone(),
                OutputBlock::Reasoning(ReasoningBlock::delta(reasoning_delta.to_string())),
                Some(child_mid),
            )
            .await;
            return;
        }

        // Non-child session: emit reasoning events for TUI/CLI to display
        if start_reasoning {
            self.emit_output_block(
                self.session_id.clone(),
                OutputBlock::Reasoning(ReasoningBlock::start()),
                Some(message_id.clone()),
            )
            .await;
        }
        self.emit_output_block(
            self.session_id.clone(),
            OutputBlock::Reasoning(ReasoningBlock::delta(reasoning_delta.to_string())),
            Some(message_id.clone()),
        )
        .await;

        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        let mut message_snapshot = None;
        if let Some(message) = session.get_message_mut(&message_id) {
            message.add_reasoning(reasoning_delta);
            apply_scheduler_decision_metadata(stage_name, message);
            message_snapshot = Some(message.clone());
        }
        session.touch();
        sessions.update(session);
        drop(sessions);

        if let Some(message) = message_snapshot.as_ref() {
            self.emit_stage_block(message).await;
        }
    }

    async fn on_scheduler_stage_usage(
        &self,
        _stage_name: &str,
        _stage_index: u32,
        usage: &rocode_orchestrator::runtime::events::StepUsage,
        finalized: bool,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
        let model_pricing = self.model_pricing;
        self.update_active_stage_message(
            |message, active| {
                active.live_usage.merge_snapshot(usage);
                if finalized {
                    let live_usage = active.live_usage.clone();
                    active.committed_usage.accumulate(&live_usage);
                    active.live_usage = rocode_orchestrator::runtime::events::StepUsage::default();
                }
                write_stage_usage_totals(
                    message,
                    &active.committed_usage,
                    &active.live_usage,
                    finalized,
                    model_pricing,
                );
            },
            "prompt.scheduler.stage.usage",
        )
        .await;
    }

    async fn on_scheduler_stage_end(
        &self,
        _: &str,
        stage_name: &str,
        stage_index: u32,
        stage_total: u32,
        content: &str,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        let active = {
            let mut guard = self.active_stage_messages.lock().await;
            guard.pop()
        };

        match active {
            Some(active) if active.stage_name == stage_name => {
                // Finalize the streaming message: replace content with final version.
                let body = content.trim();
                let mut sessions = self.state.sessions.lock().await;
                let Some(mut session) = sessions.get(&self.session_id).cloned() else {
                    return;
                };
                let mut message_snapshot = None;
                if let Some(message) = session.get_message_mut(&active.message_id) {
                    message.set_text(body.to_string());
                    message.metadata.insert(
                        scheduler_keys::STAGE_TOTAL.to_string(),
                        serde_json::json!(stage_total),
                    );
                    message.metadata.remove(scheduler_keys::STREAMING);
                    let status = if body.starts_with("Stage error:") {
                        SchedulerStageStatus::Blocked
                    } else {
                        SchedulerStageStatus::Done
                    };
                    message.metadata.insert(
                        scheduler_keys::STATUS.to_string(),
                        serde_json::json!(status.as_str()),
                    );
                    message.metadata.insert(
                        scheduler_keys::FOCUS.to_string(),
                        serde_json::json!(scheduler_stage_focus(stage_name)),
                    );
                    message.metadata.insert(
                        scheduler_keys::LAST_EVENT.to_string(),
                        serde_json::json!(if body.starts_with("Stage error:") {
                            "Stage failed"
                        } else {
                            "Stage completed"
                        }),
                    );
                    message.metadata.insert(
                        scheduler_keys::WAITING_ON.to_string(),
                        serde_json::json!(SchedulerStageWaitingOn::None.as_str()),
                    );
                    if active.step_count > 0 {
                        message.metadata.insert(
                            scheduler_keys::STEP.to_string(),
                            serde_json::json!(active.step_count),
                        );
                    }
                    apply_scheduler_decision_metadata(stage_name, message);
                    message_snapshot = Some(message.clone());
                }

                let child_session_id = active.child_session_id.clone();
                let child_message_id = active.child_message_id.clone();

                // Finalize child session assistant message if present.
                if let (Some(ref child_sid), Some(ref child_mid)) =
                    (child_session_id.as_ref(), child_message_id.as_ref())
                {
                    if let Some(mut child) = sessions.get(child_sid).cloned() {
                        if let Some(msg) = child.get_message_mut(child_mid) {
                            msg.finish = Some("end_turn".to_string());
                        }
                        child.touch();
                        sessions.update(child);
                    }
                }

                session.touch();
                sessions.update(session);
                drop(sessions);

                if let Some(message) = message_snapshot.as_ref() {
                    self.emit_stage_block(message).await;
                }
                if let (Some(child_sid), Some(child_mid)) = (child_session_id, child_message_id) {
                    if active.child_reasoning_started {
                        self.emit_output_block(
                            child_sid.clone(),
                            OutputBlock::Reasoning(ReasoningBlock::end()),
                            Some(child_mid.clone()),
                        )
                        .await;
                    }
                    self.emit_output_block(
                        child_sid.clone(),
                        OutputBlock::Message(MessageBlock::end(OutputMessageRole::Assistant)),
                        Some(child_mid.clone()),
                    )
                    .await;
                    broadcast_child_session_detached(
                        &self.state,
                        self.session_id.clone(),
                        child_sid.clone(),
                    );
                    // Update aggregated runtime state.
                    self.state
                        .runtime_state
                        .child_detached(&self.session_id, &child_sid)
                        .await;
                } else {
                    // Non-child session: emit reasoning end if reasoning was started
                    if active.reasoning_started {
                        self.emit_output_block(
                            self.session_id.clone(),
                            OutputBlock::Reasoning(ReasoningBlock::end()),
                            Some(active.message_id.clone()),
                        )
                        .await;
                    }
                }
                self.state
                    .runtime_control
                    .finish_scheduler_stage(&active.execution_id)
                    .await;
            }
            Some(_) => {
                self.emit_stage_message(stage_name, stage_index, stage_total, content, exec_ctx)
                    .await;
            }
            None => {
                // Fallback: no streaming message was created, emit full message.
                self.emit_stage_message(stage_name, stage_index, stage_total, content, exec_ctx)
                    .await;
            }
        }
    }
}

fn stage_execution_patch_from_message(message: &SessionMessage) -> ExecutionPatch {
    ExecutionPatch {
        status: message
            .metadata
            .get(scheduler_keys::STATUS)
            .and_then(|value| value.as_str())
            .and_then(runtime_execution_status_from_stage_status),
        waiting_on: message
            .metadata
            .get(scheduler_keys::WAITING_ON)
            .and_then(|value| value.as_str())
            .filter(|value| *value != SchedulerStageWaitingOn::None.as_str() && !value.is_empty())
            .map(|value| FieldUpdate::Set(value.to_string()))
            .unwrap_or(FieldUpdate::Clear),
        recent_event: message
            .metadata
            .get(scheduler_keys::LAST_EVENT)
            .and_then(|value| value.as_str())
            .map(|value| FieldUpdate::Set(value.to_string()))
            .unwrap_or(FieldUpdate::Keep),
        metadata: FieldUpdate::Set(scheduler_stage_runtime_metadata(message)),
        ..ExecutionPatch::default()
    }
}

fn runtime_execution_status_from_stage_status(value: &str) -> Option<ExecutionStatus> {
    match SchedulerStageStatus::parse(value)? {
        SchedulerStageStatus::Running => Some(ExecutionStatus::Running),
        SchedulerStageStatus::Waiting | SchedulerStageStatus::Blocked => {
            Some(ExecutionStatus::Waiting)
        }
        SchedulerStageStatus::Cancelling => Some(ExecutionStatus::Cancelling),
        SchedulerStageStatus::Retrying => Some(ExecutionStatus::Retry),
        SchedulerStageStatus::Done | SchedulerStageStatus::Cancelled => Some(ExecutionStatus::Done),
    }
}

fn scheduler_stage_runtime_metadata(message: &SessionMessage) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    for key in [
        scheduler_keys::PROFILE,
        scheduler_keys::RESOLVED_PROFILE,
        scheduler_keys::STAGE,
        scheduler_keys::STAGE_INDEX,
        scheduler_keys::STAGE_TOTAL,
        scheduler_keys::AGENT,
        scheduler_keys::STEP,
        scheduler_keys::FOCUS,
        scheduler_keys::PROJECTION,
        scheduler_keys::TOOL_POLICY,
        scheduler_keys::LOOP_BUDGET,
        scheduler_keys::ACTIVITY,
        scheduler_keys::AVAILABLE_SKILL_COUNT,
        scheduler_keys::AVAILABLE_AGENT_COUNT,
        scheduler_keys::AVAILABLE_CATEGORY_COUNT,
        scheduler_keys::ACTIVE_SKILLS,
        scheduler_keys::ACTIVE_AGENTS,
        scheduler_keys::ACTIVE_CATEGORIES,
        scheduler_keys::PROMPT_TOKENS,
        scheduler_keys::COMPLETION_TOKENS,
        scheduler_keys::REASONING_TOKENS,
        scheduler_keys::CACHE_READ_TOKENS,
        scheduler_keys::CACHE_WRITE_TOKENS,
    ] {
        if let Some(value) = message.metadata.get(key).cloned() {
            metadata.insert(key.to_string(), value);
        }
    }
    serde_json::Value::Object(metadata)
}

fn scheduler_stage_execution_metadata(
    scheduler_profile: &str,
    stage_name: &str,
    stage_index: u32,
    stage_total: Option<u32>,
    agent_name: &str,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        scheduler_keys::PROFILE.to_string(),
        serde_json::json!(scheduler_profile),
    );
    metadata.insert(
        scheduler_keys::STAGE.to_string(),
        serde_json::json!(stage_name),
    );
    metadata.insert(
        scheduler_keys::STAGE_INDEX.to_string(),
        serde_json::json!(stage_index),
    );
    if let Some(stage_total) = stage_total {
        metadata.insert(
            scheduler_keys::STAGE_TOTAL.to_string(),
            serde_json::json!(stage_total),
        );
    }
    metadata.insert(
        scheduler_keys::AGENT.to_string(),
        serde_json::json!(agent_name),
    );
    metadata.insert(
        scheduler_keys::FOCUS.to_string(),
        serde_json::json!(scheduler_stage_focus(stage_name)),
    );
    serde_json::Value::Object(metadata)
}

fn summarize_tool_activity(tool_name: &str, tool_args: &serde_json::Value) -> Option<String> {
    match BuiltinToolName::parse(tool_name) {
        Some(BuiltinToolName::Question) => summarize_question_args(tool_args),
        Some(BuiltinToolName::TodoWrite) => summarize_todo_args(tool_args),
        Some(BuiltinToolName::TodoRead) => Some("Todo list read".to_string()),
        Some(BuiltinToolName::Task) => summarize_task_args(tool_args),
        Some(BuiltinToolName::TaskFlow) => summarize_task_flow_args(tool_args),
        Some(BuiltinToolName::Bash) => summarize_bash_args(tool_args),
        Some(BuiltinToolName::Read) => summarize_read_args(tool_args),
        Some(BuiltinToolName::Write) => summarize_write_args(tool_args),
        Some(BuiltinToolName::Edit) => summarize_edit_args(tool_args),
        Some(BuiltinToolName::Glob) => summarize_glob_args(tool_args),
        Some(BuiltinToolName::Grep) => summarize_grep_args(tool_args),
        Some(BuiltinToolName::WebFetch) => summarize_webfetch_args(tool_args),
        Some(BuiltinToolName::WebSearch | BuiltinToolName::CodeSearch) => {
            summarize_search_args(tool_name, tool_args)
        }
        Some(BuiltinToolName::Lsp) => summarize_lsp_args(tool_args),
        Some(BuiltinToolName::Batch) => summarize_batch_args(tool_args),
        Some(BuiltinToolName::Skill) => summarize_skill_args(tool_args),
        Some(BuiltinToolName::ApplyPatch) => Some("Apply Patch".to_string()),
        Some(BuiltinToolName::Ls) => summarize_list_args(tool_args),
        Some(BuiltinToolName::NotebookEdit) => summarize_notebook_edit_args(tool_args),
        _ => summarize_generic_tool_args(tool_name, tool_args),
    }
}

fn summarize_tool_result_activity(
    tool_name: &str,
    tool_output: &OrchestratorToolOutput,
) -> Option<String> {
    match BuiltinToolName::parse(tool_name) {
        Some(BuiltinToolName::Question) => summarize_question_result(tool_output.metadata.as_ref()),
        Some(BuiltinToolName::TodoWrite | BuiltinToolName::TodoRead) => {
            summarize_todo_result(tool_output.metadata.as_ref())
        }
        _ => None,
    }
}

fn summarize_question_args(tool_args: &serde_json::Value) -> Option<String> {
    let questions = tool_args.get(wire_fields::QUESTIONS)?.as_array()?;
    if questions.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Question ({})", questions.len())];
    for question in questions.iter().take(3) {
        let header = question
            .get(wire_fields::HEADER)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Prompt");
        let text = question
            .get(wire_fields::QUESTION)
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if !text.is_empty() {
            lines.push(format!("- {header}: {}", collapse_text(text, 96)));
        }
    }
    Some(lines.join("\n"))
}

fn summarize_todo_args(tool_args: &serde_json::Value) -> Option<String> {
    let todos = tool_args.get(todo_keys::TODOS)?.as_array()?;
    if todos.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Todo list ({})", todos.len())];
    for todo in todos.iter().take(5) {
        let content = todo
            .get(todo_keys::CONTENT)
            .and_then(|value| value.as_str())?;
        let status = todo
            .get(todo_keys::STATUS)
            .and_then(|value| value.as_str())
            .unwrap_or(TodoStatus::Pending.as_str());
        lines.push(format!(
            "- [{}] {}",
            prettify_token(status),
            collapse_text(content, 88)
        ));
    }
    Some(lines.join("\n"))
}

fn summarize_task_args(tool_args: &serde_json::Value) -> Option<String> {
    let agent = tool_args
        .get("subagent_type")
        .or_else(|| tool_args.get("subagentType"))
        .or_else(|| tool_args.get("category"))
        .and_then(|value| value.as_str())
        .unwrap_or("subagent");
    let description = tool_args
        .get("description")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let prompt = tool_args
        .get("prompt")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let mut lines = vec![format!("Task → {}", prettify_token(agent))];
    if !description.is_empty() {
        lines.push(format!("- label: {}", collapse_text(description, 88)));
    }
    if !prompt.is_empty() {
        lines.push(format!("- prompt: {}", collapse_text(prompt, 88)));
    }
    Some(lines.join("\n"))
}

fn summarize_task_flow_args(tool_args: &serde_json::Value) -> Option<String> {
    let operation = tool_args
        .get("operation")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let mut lines = vec![format!("TaskFlow → {}", prettify_token(operation))];
    if let Some(agent) = tool_args.get("agent").and_then(|value| value.as_str()) {
        lines.push(format!("- agent: {}", prettify_token(agent)));
    }
    if let Some(description) = tool_args
        .get("description")
        .and_then(|value| value.as_str())
    {
        lines.push(format!("- label: {}", collapse_text(description, 88)));
    }
    if let Some(prompt) = tool_args.get("prompt").and_then(|value| value.as_str()) {
        lines.push(format!("- prompt: {}", collapse_text(prompt, 88)));
    }
    if let Some(todo_item) = tool_args
        .get("todo_item")
        .and_then(|value| value.as_object())
    {
        if let Some(content) = todo_item
            .get(todo_keys::CONTENT)
            .and_then(|value| value.as_str())
        {
            lines.push(format!("- todo: {}", collapse_text(content, 88)));
        }
    }
    Some(lines.join("\n"))
}

/// Runtime evidence for which scheduler capabilities were actually activated
/// inside the current stage.
///
/// Governance rule:
/// - `SchedulerStageCapabilities` is the stage's available resource pool.
/// - `scheduler_stage_active_*` is runtime evidence only.
/// - Adapters render these fields but never infer them.
/// - Evidence may arrive from request-time tool arguments or result-time tool
///   metadata, so both sides feed the same authority here.
#[derive(Default)]
struct StageCapabilityActivityEvidence {
    agents: Vec<String>,
    categories: Vec<String>,
    skills: Vec<String>,
}

impl StageCapabilityActivityEvidence {
    fn is_empty(&self) -> bool {
        self.agents.is_empty() && self.categories.is_empty() && self.skills.is_empty()
    }

    fn push_agent(&mut self, value: Option<&str>) {
        push_unique_trimmed(&mut self.agents, value);
    }

    fn push_category(&mut self, value: Option<&str>) {
        push_unique_trimmed(&mut self.categories, value);
    }

    fn push_skills_from_array(&mut self, value: Option<&serde_json::Value>) {
        let Some(values) = value.and_then(|value| value.as_array()) else {
            return;
        };
        for skill in values.iter().filter_map(|value| value.as_str()) {
            push_unique_trimmed(&mut self.skills, Some(skill));
        }
    }
}

enum StageCapabilityActivitySource<'a> {
    ToolArgs(&'a serde_json::Value),
    ToolOutput(&'a OrchestratorToolOutput),
}

/// Extract the single authority view of runtime capability activation for a
/// scheduler stage.
///
/// This intentionally tracks only concrete scheduling choices:
/// - selected agent
/// - selected category
/// - explicitly loaded skills
///
/// It does not treat generic tool usage, questions, summaries, or stage
/// capability pools as "active" capability evidence.
fn extract_stage_capability_activity(
    tool_name: &str,
    source: StageCapabilityActivitySource<'_>,
) -> StageCapabilityActivityEvidence {
    let mut evidence = StageCapabilityActivityEvidence::default();

    match source {
        StageCapabilityActivitySource::ToolArgs(args) => {
            if !tool_supports_stage_capability_activity_args(tool_name) {
                return evidence;
            }

            evidence.push_agent(
                args.get("subagent_type")
                    .or_else(|| args.get("subagentType"))
                    .or_else(|| args.get("agent"))
                    .and_then(|value| value.as_str()),
            );
            evidence.push_category(args.get("category").and_then(|value| value.as_str()));
            evidence.push_skills_from_array(
                args.get("load_skills").or_else(|| args.get("loadedSkills")),
            );
        }
        StageCapabilityActivitySource::ToolOutput(tool_output) => {
            let Some(metadata) = tool_output.metadata.as_ref() else {
                return evidence;
            };
            if !tool_supports_stage_capability_activity_output(tool_name, metadata) {
                return evidence;
            }

            evidence.push_agent(
                metadata
                    .get("agent")
                    .and_then(|value| value.as_str())
                    .or_else(|| {
                        metadata
                            .get("task")
                            .and_then(|value| value.get("agent"))
                            .and_then(|value| value.as_str())
                    }),
            );
            evidence.push_category(metadata.get("category").and_then(|value| value.as_str()));
            evidence.push_skills_from_array(
                metadata
                    .get("loadedSkills")
                    .or_else(|| metadata.get("load_skills"))
                    .or_else(|| {
                        metadata
                            .get("task")
                            .and_then(|value| value.get("loadedSkills"))
                    }),
            );
        }
    }

    evidence
}

fn tool_supports_stage_capability_activity_args(tool_name: &str) -> bool {
    matches!(
        BuiltinToolName::parse(tool_name),
        Some(BuiltinToolName::Task | BuiltinToolName::TaskFlow)
    )
}

fn tool_supports_stage_capability_activity_output(
    tool_name: &str,
    metadata: &serde_json::Value,
) -> bool {
    matches!(
        BuiltinToolName::parse(tool_name),
        Some(BuiltinToolName::Task | BuiltinToolName::TaskFlow)
    ) || metadata
        .get("delegated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        || metadata.get("agentTaskId").is_some()
        || metadata.get("task").is_some()
}

fn apply_stage_capability_activity_evidence(
    message: &mut SessionMessage,
    evidence: StageCapabilityActivityEvidence,
) {
    if evidence.is_empty() {
        return;
    }

    for agent in evidence.agents {
        push_stage_active_value(message, scheduler_keys::ACTIVE_AGENTS, &agent);
    }
    for category in evidence.categories {
        push_stage_active_value(message, scheduler_keys::ACTIVE_CATEGORIES, &category);
    }
    for skill in evidence.skills {
        push_stage_active_value(message, scheduler_keys::ACTIVE_SKILLS, &skill);
    }
}

fn push_stage_active_value(message: &mut SessionMessage, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }

    let entry = message
        .metadata
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!([]));

    if !entry.is_array() {
        *entry = serde_json::json!([]);
    }

    let Some(values) = entry.as_array_mut() else {
        return;
    };

    if values
        .iter()
        .any(|existing| existing.as_str() == Some(value))
    {
        return;
    }

    values.push(serde_json::json!(value));
}

fn push_unique_trimmed(target: &mut Vec<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if target.iter().any(|existing| existing == value) {
        return;
    }
    target.push(value.to_string());
}

fn summarize_generic_tool_args(tool_name: &str, tool_args: &serde_json::Value) -> Option<String> {
    if tool_args.is_null() {
        return None;
    }
    let name = pretty_scheduler_stage_name(tool_name);
    // Extract key=value pairs from primitive fields, omitting large text blobs.
    if let Some(object) = tool_args.as_object() {
        let preview = format_activity_primitive_args(object);
        if let Some(preview) = preview {
            return Some(format!("{name} → {preview}"));
        }
    }
    // Fallback: collapse raw JSON
    let raw = collapse_text(&tool_args.to_string(), 120);
    Some(format!("{name} → {raw}"))
}

// ── Tool-specific activity summarizers ──────────────────────────────────

fn summarize_bash_args(tool_args: &serde_json::Value) -> Option<String> {
    let command = activity_extract_string(tool_args, &["command", "cmd", "script", "input"])?;
    Some(format!("Bash → $ {}", collapse_text(&command, 120)))
}

fn summarize_read_args(tool_args: &serde_json::Value) -> Option<String> {
    let path = activity_extract_string(
        tool_args,
        &[
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
            patch_keys::LEGACY_PATH,
            "file",
        ],
    )?;
    Some(format!("Read → {path}"))
}

fn summarize_write_args(tool_args: &serde_json::Value) -> Option<String> {
    let path = activity_extract_string(
        tool_args,
        &[
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
            patch_keys::LEGACY_PATH,
            "file",
        ],
    )?;
    Some(format!("Write ← {path}"))
}

fn summarize_edit_args(tool_args: &serde_json::Value) -> Option<String> {
    let path = activity_extract_string(
        tool_args,
        &[
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
            patch_keys::LEGACY_PATH,
            "file",
        ],
    )?;
    Some(format!("Edit ← {path}"))
}

fn summarize_glob_args(tool_args: &serde_json::Value) -> Option<String> {
    let pattern = activity_extract_string(tool_args, &["pattern"])?;
    let target = activity_extract_string(
        tool_args,
        &[
            patch_keys::LEGACY_PATH,
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
        ],
    );
    let summary = match target {
        Some(path) => format!("Glob → \"{}\" in {}", pattern, path),
        None => format!("Glob → \"{}\"", pattern),
    };
    Some(summary)
}

fn summarize_grep_args(tool_args: &serde_json::Value) -> Option<String> {
    let pattern = activity_extract_string(tool_args, &["pattern", "query"])?;
    let target = activity_extract_string(
        tool_args,
        &[
            patch_keys::LEGACY_PATH,
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
        ],
    );
    let summary = match target {
        Some(path) => format!("Grep → \"{}\" in {}", pattern, path),
        None => format!("Grep → \"{}\"", pattern),
    };
    Some(summary)
}

fn summarize_webfetch_args(tool_args: &serde_json::Value) -> Option<String> {
    let url = activity_extract_string(tool_args, &["url"])?;
    Some(format!("Web Fetch → {url}"))
}

fn summarize_search_args(tool_name: &str, tool_args: &serde_json::Value) -> Option<String> {
    let query = activity_extract_string(tool_args, &["query"])?;
    let name = pretty_scheduler_stage_name(tool_name);
    Some(format!("{name} → \"{query}\""))
}

fn summarize_lsp_args(tool_args: &serde_json::Value) -> Option<String> {
    let operation = activity_extract_string(tool_args, &["operation"])?;
    let target = activity_extract_string(
        tool_args,
        &[
            patch_keys::FILE_PATH,
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::LEGACY_PATH,
        ],
    );
    let summary = match target {
        Some(path) => format!("LSP → {} {}", operation, path),
        None => format!("LSP → {}", operation),
    };
    Some(summary)
}

fn summarize_batch_args(tool_args: &serde_json::Value) -> Option<String> {
    let calls = tool_args
        .get("toolCalls")
        .or_else(|| tool_args.get("tool_calls"))
        .and_then(|v| v.as_array())?;
    let count = calls.len();
    let mut names: Vec<String> = calls
        .iter()
        .filter_map(|call| {
            call.get("tool")
                .or_else(|| call.get("name"))
                .or_else(|| call.get("tool_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    names.dedup();
    if names.is_empty() {
        Some(format!("Batch → {} tools", count))
    } else {
        Some(format!("Batch → {} tools ({})", count, names.join(", ")))
    }
}

fn summarize_skill_args(tool_args: &serde_json::Value) -> Option<String> {
    let name = activity_extract_string(tool_args, &["name", "skill"])?;
    Some(format!("Skill → \"{}\"", name))
}

fn summarize_list_args(tool_args: &serde_json::Value) -> Option<String> {
    let path = activity_extract_string(
        tool_args,
        &[
            patch_keys::LEGACY_PATH,
            patch_keys::FILE_PATH_SNAKE,
            patch_keys::FILE_PATH,
        ],
    );
    match path {
        Some(path) => Some(format!("List → {path}")),
        None => Some("List → .".to_string()),
    }
}

fn summarize_notebook_edit_args(tool_args: &serde_json::Value) -> Option<String> {
    let path = activity_extract_string(
        tool_args,
        &[
            "notebook_path",
            "notebookPath",
            patch_keys::LEGACY_PATH,
            patch_keys::FILE_PATH_SNAKE,
        ],
    );
    let mode = activity_extract_string(tool_args, &["edit_mode", "editMode"]);
    let summary = match (path, mode) {
        (Some(path), Some(mode)) => format!("Notebook Edit → {} {}", mode, path),
        (Some(path), None) => format!("Notebook Edit → {}", path),
        (None, Some(mode)) => format!("Notebook Edit → {}", mode),
        (None, None) => "Notebook Edit".to_string(),
    };
    Some(summary)
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Extract the first non-empty string value for any of the given keys.
fn activity_extract_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(content) = object.get(*key).and_then(|v| v.as_str()) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Format an object's primitive fields as `key=value` pairs, omitting large
/// text blobs to keep the summary readable.
fn format_activity_primitive_args(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    const OMIT: &[&str] = &[
        todo_keys::CONTENT,
        "new_string",
        "old_string",
        "new_source",
        "patch",
        "prompt",
        "questions",
        todo_keys::TODOS,
        "body",
        "text",
    ];
    let mut parts = Vec::new();
    for (key, value) in object {
        if OMIT.contains(&key.as_str()) {
            continue;
        }
        let rendered = match value {
            serde_json::Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    continue;
                }
                collapse_text(trimmed, 40)
            }
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
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

fn summarize_question_result(metadata: Option<&serde_json::Value>) -> Option<String> {
    let fields = metadata?
        .get(output_keys::DISPLAY_FIELDS)
        .and_then(|value| value.as_array())?;
    if fields.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Answered ({})", fields.len())];
    for field in fields.iter().take(3) {
        let key = field
            .get(output_keys::DISPLAY_FIELD_KEY)
            .and_then(|value| value.as_str())
            .unwrap_or("Question");
        let value = field
            .get(output_keys::DISPLAY_FIELD_VALUE)
            .and_then(|value| value.as_str())
            .unwrap_or("");
        lines.push(format!(
            "- {}: {}",
            collapse_text(key, 48),
            collapse_text(value, 72)
        ));
    }
    Some(lines.join("\n"))
}

fn summarize_todo_result(metadata: Option<&serde_json::Value>) -> Option<String> {
    let todos = metadata?
        .get(todo_keys::TODOS)
        .and_then(|value| value.as_array())?;
    if todos.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Todo list ({})", todos.len())];
    for todo in todos.iter().take(5) {
        let content = todo
            .get(todo_keys::CONTENT)
            .and_then(|value| value.as_str())?;
        let status = todo
            .get(todo_keys::STATUS)
            .and_then(|value| value.as_str())
            .unwrap_or(TodoStatus::Pending.as_str());
        lines.push(format!(
            "- [{}] {}",
            prettify_token(status),
            collapse_text(content, 88)
        ));
    }
    Some(lines.join("\n"))
}

fn collapse_text(input: &str, max_chars: usize) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (index, ch) in normalized.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Extract todo items from a todowrite tool call's arguments so they can be
/// stored in the TodoManager (single authority for todo state — Art. 5).
fn extract_todo_items_from_args(
    tool_args: &serde_json::Value,
) -> Option<Vec<rocode_session::TodoInfo>> {
    let todos = tool_args.get(todo_keys::TODOS)?.as_array()?;
    if todos.is_empty() {
        return None;
    }
    let items = todos
        .iter()
        .filter_map(|todo| {
            let content = todo
                .get(todo_keys::CONTENT)
                .and_then(|v| v.as_str())?
                .to_string();
            let status = todo
                .get(todo_keys::STATUS)
                .and_then(|v| v.as_str())
                .unwrap_or(TodoStatus::Pending.as_str())
                .to_string();
            let priority = todo
                .get(todo_keys::PRIORITY)
                .and_then(|v| v.as_str())
                .unwrap_or(TodoPriority::Medium.as_str())
                .to_string();
            Some(rocode_session::TodoInfo {
                content,
                status,
                priority,
            })
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn apply_scheduler_decision_metadata(stage_name: &str, message: &mut SessionMessage) {
    clear_scheduler_decision_metadata(message);
    let text = message.get_text();
    let body = scheduler_stage_body(&text);
    match SchedulerStageName::parse(stage_name) {
        Some(SchedulerStageName::Route) => {
            let Some(decision) = parse_route_decision(&body) else {
                return;
            };
            write_scheduler_route_metadata(message, &decision);
        }
        Some(SchedulerStageName::CoordinationGate | SchedulerStageName::AutonomousGate) => {
            let Some(decision) = parse_execution_gate_decision(&body) else {
                return;
            };
            write_scheduler_gate_metadata(message, &decision);
        }
        _ => {}
    }
}

fn clear_scheduler_decision_metadata(message: &mut SessionMessage) {
    for key in [
        scheduler_decision_keys::KIND,
        scheduler_decision_keys::TITLE,
        scheduler_decision_keys::FIELDS,
        scheduler_decision_keys::SECTIONS,
        scheduler_gate_keys::STATUS,
        scheduler_gate_keys::SUMMARY,
        scheduler_gate_keys::NEXT_INPUT,
        scheduler_gate_keys::FINAL_RESPONSE,
    ] {
        message.metadata.remove(key);
    }
}

fn write_scheduler_route_metadata(message: &mut SessionMessage, decision: &RouteDecision) {
    let mut fields = Vec::new();
    let (outcome, outcome_tone) = route_outcome_field(decision);
    fields.push(decision_field("Outcome", &outcome, Some(outcome_tone)));
    if let Some(preset) = decision.preset.as_deref().filter(|value| !value.is_empty()) {
        fields.push(decision_field(
            "Preset",
            &prettify_decision_value(preset),
            Some("info"),
        ));
    }
    if let Some(review_mode) = decision.review_mode {
        let raw = format!("{:?}", review_mode).to_ascii_lowercase();
        fields.push(decision_field(
            "Review",
            &prettify_decision_value(&raw),
            Some(if raw == "skip" { "warning" } else { "success" }),
        ));
    }
    if let Some(insert_plan_stage) = decision.insert_plan_stage {
        fields.push(decision_field(
            "Plan Stage",
            if insert_plan_stage { "Yes" } else { "No" },
            Some(if insert_plan_stage {
                "success"
            } else {
                "muted"
            }),
        ));
    }
    if !decision.rationale_summary.trim().is_empty() {
        fields.push(decision_field(
            "Why",
            decision.rationale_summary.trim(),
            None,
        ));
    }
    let mut sections = Vec::new();
    if let Some(context_append) = decision
        .context_append
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(decision_section("Appended Context", context_append));
    }
    if let Some(direct_response) = decision
        .direct_response
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(decision_section("Response", direct_response));
    }

    write_scheduler_decision_metadata(
        message,
        SchedulerDecisionKind::Route.as_str(),
        "Decision",
        fields,
        sections,
    );
}

fn write_scheduler_gate_metadata(
    message: &mut SessionMessage,
    decision: &SchedulerExecutionGateDecision,
) {
    let status = format!("{:?}", decision.status).to_ascii_lowercase();
    let mut fields = vec![decision_field(
        "Outcome",
        &gate_outcome_label(&status),
        Some("status"),
    )];
    if !decision.summary.is_empty() {
        fields.push(decision_field("Why", &decision.summary, None));
    }
    if let Some(next_input) = decision
        .next_input
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        fields.push(decision_field("Next Action", next_input, Some("warning")));
    }
    let mut sections = Vec::new();
    if let Some(final_response) = decision
        .final_response
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        sections.push(decision_section("Final Response", final_response));
    }
    write_scheduler_decision_metadata(
        message,
        SchedulerDecisionKind::Gate.as_str(),
        "Decision",
        fields,
        sections,
    );
    message.metadata.insert(
        scheduler_gate_keys::STATUS.to_string(),
        serde_json::json!(status),
    );
    if !decision.summary.is_empty() {
        message.metadata.insert(
            scheduler_gate_keys::SUMMARY.to_string(),
            serde_json::json!(decision.summary),
        );
    }
    if let Some(next_input) = decision
        .next_input
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        message.metadata.insert(
            scheduler_gate_keys::NEXT_INPUT.to_string(),
            serde_json::json!(next_input),
        );
    }
    if let Some(final_response) = decision
        .final_response
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        message.metadata.insert(
            scheduler_gate_keys::FINAL_RESPONSE.to_string(),
            serde_json::json!(final_response),
        );
    }
}

fn write_scheduler_decision_metadata(
    message: &mut SessionMessage,
    kind: &str,
    title: &str,
    fields: Vec<serde_json::Value>,
    sections: Vec<serde_json::Value>,
) {
    message.metadata.insert(
        scheduler_decision_keys::KIND.to_string(),
        serde_json::json!(kind),
    );
    message.metadata.insert(
        scheduler_decision_keys::TITLE.to_string(),
        serde_json::json!(title),
    );
    message.metadata.insert(
        scheduler_decision_keys::FIELDS.to_string(),
        serde_json::json!(fields),
    );
    message.metadata.insert(
        scheduler_decision_keys::SECTIONS.to_string(),
        serde_json::json!(sections),
    );
    message.metadata.insert(
        scheduler_decision_keys::SPEC.to_string(),
        scheduler_decision_render_spec_json(),
    );
}

fn decision_field(label: &str, value: &str, tone: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        output_keys::LABEL: label,
        output_keys::VALUE: value,
        output_keys::TONE: tone,
    })
}

fn decision_section(title: &str, body: &str) -> serde_json::Value {
    serde_json::json!({
        output_keys::TITLE: title,
        output_keys::BODY: body,
    })
}

fn scheduler_decision_render_spec_json() -> serde_json::Value {
    serde_json::json!({
        output_decision_spec_keys::VERSION: SchedulerDecisionRenderSpecVersion::DecisionCardV1.as_str(),
        output_decision_spec_keys::SHOW_HEADER_DIVIDER: true,
        output_decision_spec_keys::FIELD_ORDER: SchedulerDecisionFieldOrder::AsProvided.as_str(),
        output_decision_spec_keys::FIELD_LABEL_EMPHASIS: SchedulerDecisionFieldLabelEmphasis::Bold.as_str(),
        output_decision_spec_keys::STATUS_PALETTE: SchedulerDecisionStatusPalette::Semantic.as_str(),
        output_decision_spec_keys::SECTION_SPACING: SchedulerDecisionSectionSpacing::Loose.as_str(),
        output_decision_spec_keys::UPDATE_POLICY: SchedulerDecisionUpdatePolicy::StableShellLiveRuntimeAppendDecision.as_str(),
    })
}

fn prettify_decision_value(raw: &str) -> String {
    raw.split(['-', '_', ' '])
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

fn route_outcome_field(decision: &RouteDecision) -> (String, &'static str) {
    match decision.mode {
        rocode_orchestrator::RouteMode::Direct => match decision.direct_kind {
            Some(rocode_orchestrator::DirectKind::Reply) => ("Direct Reply".to_string(), "warning"),
            Some(rocode_orchestrator::DirectKind::Clarify) => {
                ("Direct Clarification".to_string(), "warning")
            }
            None => ("Direct".to_string(), "warning"),
        },
        rocode_orchestrator::RouteMode::Orchestrate => ("Orchestrate".to_string(), "success"),
    }
}

fn gate_outcome_label(status: &str) -> String {
    match status {
        "continue" => "Continue".to_string(),
        "done" => "Done".to_string(),
        "blocked" => "Blocked".to_string(),
        other => prettify_decision_value(other),
    }
}

#[cfg(test)]
pub(crate) fn scheduler_stage_title(scheduler_profile: &str, stage_name: &str) -> String {
    format!(
        "{} · {}",
        scheduler_profile,
        pretty_scheduler_stage_name(stage_name)
    )
}

pub(crate) fn scheduler_stage_focus(stage_name: &str) -> &'static str {
    match SchedulerStageName::parse(stage_name) {
        Some(SchedulerStageName::Route) => {
            "Decide the correct workflow and preserve request intent."
        }
        Some(SchedulerStageName::Interview) => {
            "Clarify scope, requirements, and blocking ambiguities."
        }
        Some(SchedulerStageName::Plan) => "Draft the executable plan and its guardrails.",
        Some(SchedulerStageName::Review) => "Audit the current artifact for gaps and readiness.",
        Some(SchedulerStageName::Handoff) => {
            "Prepare the next-step handoff for execution or approval."
        }
        Some(SchedulerStageName::ExecutionOrchestration) => {
            "Drive the active execution workflow to concrete results."
        }
        Some(SchedulerStageName::Synthesis) => {
            "Merge stage outputs into a final user-facing delivery."
        }
        Some(SchedulerStageName::CoordinationVerification) => {
            "Verify delegated work against actual evidence."
        }
        Some(SchedulerStageName::CoordinationGate) => {
            "Decide whether the coordination loop can finish."
        }
        Some(SchedulerStageName::CoordinationRetry) => {
            "Prepare the bounded retry focus for the next round."
        }
        Some(SchedulerStageName::AutonomousVerification) => {
            "Verify autonomous execution against the task boundary."
        }
        Some(SchedulerStageName::AutonomousGate) => {
            "Decide whether autonomous execution is complete."
        }
        Some(SchedulerStageName::AutonomousRetry) => "Prepare the bounded recovery retry.",
        _ => "Advance the current scheduler stage.",
    }
}

fn pretty_scheduler_stage_name(stage_name: &str) -> String {
    prettify_token(stage_name)
}

fn prettify_token(token: &str) -> String {
    token
        .split(['-', '_'])
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

pub(crate) async fn emit_scheduler_stage_message(input: SchedulerStageMessageInput<'_>) {
    let SchedulerStageMessageInput {
        state,
        session_id,
        scheduler_profile,
        stage_name,
        stage_index,
        stage_total,
        content,
        exec_ctx,
        output_hook,
    } = input;

    let body = content.trim();
    if body.is_empty() {
        return;
    }

    let mut sessions = state.sessions.lock().await;
    let Some(mut session) = sessions.get(session_id).cloned() else {
        return;
    };

    let message = session.add_assistant_message();
    let stage_id = format!("stage_{}", uuid::Uuid::new_v4().simple());
    message.metadata.insert(
        scheduler_keys::STAGE_ID.to_string(),
        serde_json::json!(&stage_id),
    );
    message.metadata.insert(
        scheduler_keys::PROFILE.to_string(),
        serde_json::json!(scheduler_profile),
    );
    message.metadata.insert(
        scheduler_keys::RESOLVED_PROFILE.to_string(),
        serde_json::json!(scheduler_profile),
    );
    message.metadata.insert(
        scheduler_keys::STAGE.to_string(),
        serde_json::json!(stage_name),
    );
    message.metadata.insert(
        scheduler_keys::STAGE_INDEX.to_string(),
        serde_json::json!(stage_index),
    );
    message.metadata.insert(
        scheduler_keys::STAGE_TOTAL.to_string(),
        serde_json::json!(stage_total),
    );
    message
        .metadata
        .insert(scheduler_keys::EMITTED.to_string(), serde_json::json!(true));
    message.metadata.insert(
        scheduler_keys::AGENT.to_string(),
        serde_json::json!(exec_ctx.agent_name.clone()),
    );
    let status = if body.starts_with("Stage error:") {
        SchedulerStageStatus::Blocked
    } else {
        SchedulerStageStatus::Done
    };
    message.metadata.insert(
        scheduler_keys::STATUS.to_string(),
        serde_json::json!(status.as_str()),
    );
    message.metadata.insert(
        scheduler_keys::FOCUS.to_string(),
        serde_json::json!(scheduler_stage_focus(stage_name)),
    );
    message.metadata.insert(
        scheduler_keys::LAST_EVENT.to_string(),
        serde_json::json!(if body.starts_with("Stage error:") {
            "Stage failed"
        } else {
            "Stage completed"
        }),
    );
    message.metadata.insert(
        scheduler_keys::WAITING_ON.to_string(),
        serde_json::json!(SchedulerStageWaitingOn::None.as_str()),
    );
    if let Some(observability) = scheduler_stage_observability(scheduler_profile, stage_name) {
        message.metadata.insert(
            scheduler_keys::PROJECTION.to_string(),
            serde_json::json!(observability.projection),
        );
        message.metadata.insert(
            scheduler_keys::TOOL_POLICY.to_string(),
            serde_json::json!(observability.tool_policy),
        );
        message.metadata.insert(
            scheduler_keys::LOOP_BUDGET.to_string(),
            serde_json::json!(observability.loop_budget),
        );
    }
    message.add_text(body.to_string());
    apply_scheduler_decision_metadata(stage_name, message);
    let message_snapshot = message.clone();
    session.touch();
    sessions.update(session);
    drop(sessions);

    if let Some(block) = scheduler_stage_block_from_message(&message_snapshot) {
        emit_output_block_via_hook(
            output_hook,
            OutputBlockEvent {
                session_id: session_id.to_string(),
                block: OutputBlock::SchedulerStage(Box::new(block)),
                id: Some(message_snapshot.id.clone()),
            },
        )
        .await;
    }
}

pub(crate) struct SchedulerStageMessageInput<'a> {
    pub state: &'a Arc<ServerState>,
    pub session_id: &'a str,
    pub scheduler_profile: &'a str,
    pub stage_name: &'a str,
    pub stage_index: u32,
    pub stage_total: u32,
    pub content: &'a str,
    pub exec_ctx: &'a OrchestratorExecutionContext,
    pub output_hook: Option<&'a OutputBlockHook>,
}

pub fn assistant_visible_text(message: &SessionMessage) -> String {
    let mut out = String::new();
    for part in &message.parts {
        if let PartType::Text { text, ignored, .. } = &part.part_type {
            if ignored.unwrap_or(false) {
                continue;
            }
            out.push_str(text);
        }
    }
    rocode_session::sanitize_display_text(&out)
}

pub fn scheduler_stage_block_from_message(message: &SessionMessage) -> Option<SchedulerStageBlock> {
    let metadata = &message.metadata;
    let text = assistant_visible_text(message);

    // Delegate bulk field extraction to the shared canonical path.
    let mut block = SchedulerStageBlock::from_metadata(&text, metadata)?;

    // Override title when from_metadata() produced an empty title (no ## heading).
    if block.title.is_empty() {
        block.title = pretty_scheduler_stage_title(metadata, &block.stage);
    }

    // Enrich with decision block (requires full text + stage for contextual parsing).
    block.decision = scheduler_decision_block(metadata, &block.stage, &text);

    Some(block)
}

fn scheduler_decision_block(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
    stage: &str,
    text: &str,
) -> Option<SchedulerDecisionBlock> {
    decision_from_metadata(metadata).or_else(|| decision_from_stage_text(stage, text))
}

fn decision_from_metadata(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<SchedulerDecisionBlock> {
    let kind = metadata
        .get(scheduler_decision_keys::KIND)
        .and_then(|value| value.as_str())?
        .to_string();
    let title = metadata
        .get(scheduler_decision_keys::TITLE)
        .and_then(|value| value.as_str())
        .unwrap_or("Decision")
        .to_string();
    Some(SchedulerDecisionBlock {
        kind,
        title,
        spec: decision_spec_from_metadata(metadata).unwrap_or_else(default_decision_render_spec),
        fields: metadata
            .get(scheduler_decision_keys::FIELDS)
            .and_then(|value| value.as_array())
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|field| {
                        Some(SchedulerDecisionField {
                            label: field.get(output_keys::LABEL)?.as_str()?.to_string(),
                            value: field.get(output_keys::VALUE)?.as_str()?.to_string(),
                            tone: field
                                .get(output_keys::TONE)
                                .and_then(|value| value.as_str())
                                .map(|value| value.to_string()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        sections: metadata
            .get(scheduler_decision_keys::SECTIONS)
            .and_then(|value| value.as_array())
            .map(|sections| {
                sections
                    .iter()
                    .filter_map(|section| {
                        Some(SchedulerDecisionSection {
                            title: section.get(output_keys::TITLE)?.as_str()?.to_string(),
                            body: section.get(output_keys::BODY)?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn decision_from_stage_text(stage: &str, text: &str) -> Option<SchedulerDecisionBlock> {
    let body = scheduler_stage_body(text);
    match SchedulerStageName::parse(stage) {
        Some(SchedulerStageName::Route) => {
            let decision = parse_route_decision(&body)?;
            let mut fields = Vec::new();
            let (outcome, outcome_tone) = route_outcome_field(&decision);
            fields.push(SchedulerDecisionField {
                label: "Outcome".to_string(),
                value: outcome,
                tone: Some(outcome_tone.to_string()),
            });
            if let Some(preset) = decision.preset.as_deref().filter(|value| !value.is_empty()) {
                fields.push(SchedulerDecisionField {
                    label: "Preset".to_string(),
                    value: prettify_decision_value(preset),
                    tone: Some("info".to_string()),
                });
            }
            if let Some(review_mode) = decision.review_mode {
                let raw = format!("{:?}", review_mode).to_ascii_lowercase();
                fields.push(SchedulerDecisionField {
                    label: "Review".to_string(),
                    value: prettify_decision_value(&raw),
                    tone: Some(if raw == "skip" { "warning" } else { "success" }.to_string()),
                });
            }
            if let Some(insert_plan_stage) = decision.insert_plan_stage {
                fields.push(SchedulerDecisionField {
                    label: "Plan Stage".to_string(),
                    value: if insert_plan_stage { "Yes" } else { "No" }.to_string(),
                    tone: Some(
                        if insert_plan_stage {
                            "success"
                        } else {
                            "muted"
                        }
                        .to_string(),
                    ),
                });
            }
            if !decision.rationale_summary.trim().is_empty() {
                fields.push(SchedulerDecisionField {
                    label: "Why".to_string(),
                    value: decision.rationale_summary.trim().to_string(),
                    tone: None,
                });
            }
            let mut sections = Vec::new();
            if let Some(context_append) = decision
                .context_append
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                sections.push(SchedulerDecisionSection {
                    title: "Appended Context".to_string(),
                    body: context_append.to_string(),
                });
            }
            if let Some(direct_response) = decision
                .direct_response
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                sections.push(SchedulerDecisionSection {
                    title: "Response".to_string(),
                    body: direct_response.to_string(),
                });
            }
            Some(SchedulerDecisionBlock {
                kind: SchedulerDecisionKind::Route.as_str().to_string(),
                title: "Decision".to_string(),
                spec: default_decision_render_spec(),
                fields,
                sections,
            })
        }
        Some(SchedulerStageName::CoordinationGate | SchedulerStageName::AutonomousGate) => {
            let decision = parse_execution_gate_decision(&body)?;
            let mut fields = vec![SchedulerDecisionField {
                label: "Outcome".to_string(),
                value: gate_outcome_label(&format!("{:?}", decision.status).to_ascii_lowercase()),
                tone: Some("status".to_string()),
            }];
            if !decision.summary.is_empty() {
                fields.push(SchedulerDecisionField {
                    label: "Why".to_string(),
                    value: decision.summary,
                    tone: None,
                });
            }
            if let Some(next_input) = decision.next_input.filter(|value| !value.is_empty()) {
                fields.push(SchedulerDecisionField {
                    label: "Next Action".to_string(),
                    value: next_input,
                    tone: Some("warning".to_string()),
                });
            }
            let sections = decision
                .final_response
                .filter(|value| !value.is_empty())
                .map(|body| {
                    vec![SchedulerDecisionSection {
                        title: "Final Response".to_string(),
                        body,
                    }]
                })
                .unwrap_or_default();
            Some(SchedulerDecisionBlock {
                kind: SchedulerDecisionKind::Gate.as_str().to_string(),
                title: "Decision".to_string(),
                spec: default_decision_render_spec(),
                fields,
                sections,
            })
        }
        _ => None,
    }
}

fn decision_spec_from_metadata(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<SchedulerDecisionRenderSpec> {
    let spec = metadata.get(scheduler_decision_keys::SPEC)?;
    Some(SchedulerDecisionRenderSpec {
        version: spec
            .get(output_decision_spec_keys::VERSION)?
            .as_str()?
            .to_string(),
        show_header_divider: spec
            .get(output_decision_spec_keys::SHOW_HEADER_DIVIDER)?
            .as_bool()?,
        field_order: spec
            .get(output_decision_spec_keys::FIELD_ORDER)?
            .as_str()?
            .to_string(),
        field_label_emphasis: spec
            .get(output_decision_spec_keys::FIELD_LABEL_EMPHASIS)?
            .as_str()?
            .to_string(),
        status_palette: spec
            .get(output_decision_spec_keys::STATUS_PALETTE)?
            .as_str()?
            .to_string(),
        section_spacing: spec
            .get(output_decision_spec_keys::SECTION_SPACING)?
            .as_str()?
            .to_string(),
        update_policy: spec
            .get(output_decision_spec_keys::UPDATE_POLICY)?
            .as_str()?
            .to_string(),
    })
}

fn default_decision_render_spec() -> SchedulerDecisionRenderSpec {
    SchedulerDecisionRenderSpec {
        version: SchedulerDecisionRenderSpecVersion::DecisionCardV1
            .as_str()
            .to_string(),
        show_header_divider: true,
        field_order: SchedulerDecisionFieldOrder::AsProvided.as_str().to_string(),
        field_label_emphasis: SchedulerDecisionFieldLabelEmphasis::Bold
            .as_str()
            .to_string(),
        status_palette: SchedulerDecisionStatusPalette::Semantic
            .as_str()
            .to_string(),
        section_spacing: SchedulerDecisionSectionSpacing::Loose.as_str().to_string(),
        update_policy: SchedulerDecisionUpdatePolicy::StableShellLiveRuntimeAppendDecision
            .as_str()
            .to_string(),
    }
}

fn scheduler_stage_body(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("## ") {
        if let Some((_, body)) = rest.split_once('\n') {
            return body.trim_start().to_string();
        }
    }
    trimmed.to_string()
}

fn pretty_scheduler_stage_title(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
    stage: &str,
) -> String {
    let stage_title = prettify_decision_value(stage);
    match metadata
        .get(scheduler_keys::RESOLVED_PROFILE)
        .or_else(|| metadata.get(scheduler_keys::PROFILE))
        .and_then(|value| value.as_str())
    {
        Some(profile) if !profile.is_empty() => format!("{profile} · {stage_title}"),
        _ => stage_title,
    }
}

pub(crate) fn first_user_message_text(session: &Session) -> Option<String> {
    session
        .messages
        .iter()
        .find(|message| matches!(message.role, MessageRole::User))
        .map(|message| message.get_text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(crate) async fn ensure_default_session_title(
    session: &mut Session,
    provider: Arc<dyn Provider>,
    model_id: &str,
) {
    let Some((_, fallback)) = rocode_session::compose_session_title_source(session) else {
        return;
    };

    if !session.allows_auto_title_regeneration() && session.title.trim() != fallback.trim() {
        return;
    }

    let generated_title =
        rocode_session::generate_session_title_for_session(session, provider, model_id).await;
    if !generated_title.trim().is_empty() {
        tracing::info!(
            session_id = %session.id,
            old_title = %session.title,
            new_title = %generated_title,
            "Session title refined by LLM"
        );
        session.set_title(generated_title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use rocode_command::output_blocks::MessagePhase;
    use rocode_provider::{
        ChatRequest, ChatResponse, Choice, Content, Message, ModelInfo, Provider, ProviderError,
        Role, StreamResult,
    };
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    #[derive(Debug)]
    struct MockProvider {
        title: String,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }

        fn get_model(&self, _id: &str) -> Option<&ModelInfo> {
            None
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                id: "mock-response".to_string(),
                model: "mock-model".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Message {
                        role: Role::Assistant,
                        content: Content::Text(self.title.clone()),
                        cache_control: None,
                        provider_options: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            Ok(Box::pin(stream::iter(Vec::<
                Result<rocode_provider::StreamEvent, ProviderError>,
            >::new())))
        }
    }

    #[test]
    fn scheduler_stage_title_prettifies_hyphenated_stage_names() {
        assert_eq!(
            scheduler_stage_title("prometheus", "execution-orchestration"),
            "prometheus · Execution Orchestration"
        );
    }

    #[test]
    fn first_user_message_text_uses_first_real_user_message() {
        let mut session = Session::new("project", ".");
        session.add_assistant_message().add_text("hello");
        session.add_user_message("  First prompt  ");
        session.add_user_message("Second prompt");

        assert_eq!(
            first_user_message_text(&session).as_deref(),
            Some("First prompt")
        );
    }

    #[tokio::test]
    async fn emit_scheduler_stage_message_appends_assistant_stage_message() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };

        emit_scheduler_stage_message(SchedulerStageMessageInput {
            state: &state,
            session_id: &session_id,
            scheduler_profile: "prometheus",
            stage_name: "plan",
            stage_index: 3,
            stage_total: 4,
            content: "## Plan\n- step",
            exec_ctx: &exec_ctx,
            output_hook: None,
        })
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(message.get_text(), "## Plan\n- step");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STAGE)
                .and_then(|value| value.as_str()),
            Some("plan")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::PROJECTION)
                .and_then(|value| value.as_str()),
            Some("transcript")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::LOOP_BUDGET)
                .and_then(|value| value.as_str()),
            Some("unbounded")
        );
    }

    #[tokio::test]
    async fn emit_internal_scheduler_stage_message_is_still_renderable() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };

        emit_scheduler_stage_message(SchedulerStageMessageInput {
            state: &state,
            session_id: &session_id,
            scheduler_profile: "atlas",
            stage_name: "coordination-verification",
            stage_index: 1,
            stage_total: 3,
            content: "## Coordination Verification\n\nMissing proof for task B.",
            exec_ctx: &exec_ctx,
            output_hook: None,
        })
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message.get_text(),
            "## Coordination Verification\n\nMissing proof for task B."
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STAGE)
                .and_then(|value| value.as_str()),
            Some("coordination-verification")
        );
        assert!(!message.metadata.contains_key(scheduler_keys::PROJECTION));
    }

    #[tokio::test]
    async fn lifecycle_hook_updates_stage_runtime_metadata() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        );

        hook.on_scheduler_stage_start("prometheus", "plan", 3, None, &exec_ctx)
            .await;
        hook.on_step_start("prometheus", "model", 1, &exec_ctx)
            .await;
        hook.on_tool_start(
            "prometheus",
            "tc_question_1",
            BuiltinToolName::Question.as_str(),
            &serde_json::json!({
                "questions": [{
                    "header": "Scope",
                    "question": "Proceed with schema migration?",
                    "options": [{"label": "Yes"}]
                }]
            }),
            &exec_ctx,
        )
        .await;
        hook.on_tool_end(
            "prometheus",
            "tc_question_1",
            BuiltinToolName::Question.as_str(),
            &OrchestratorToolOutput {
                output: "{}".to_string(),
                is_error: false,
                title: Some("User response received".to_string()),
                metadata: Some(serde_json::Value::Object(serde_json::Map::from_iter([(
                    output_keys::DISPLAY_FIELDS.to_string(),
                    serde_json::Value::Array(vec![serde_json::Value::Object(
                        serde_json::Map::from_iter([
                            (
                                output_keys::DISPLAY_FIELD_KEY.to_string(),
                                serde_json::json!("Proceed with schema migration?"),
                            ),
                            (
                                output_keys::DISPLAY_FIELD_VALUE.to_string(),
                                serde_json::json!("Yes"),
                            ),
                        ]),
                    )]),
                )]))),
            },
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_end("prometheus", "plan", 3, 5, "## Plan\n\n- step", &exec_ctx)
            .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STEP)
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STATUS)
                .and_then(|value| value.as_str()),
            Some(SchedulerStageStatus::Done.as_str())
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::FOCUS)
                .and_then(|value| value.as_str()),
            Some("Draft the executable plan and its guardrails.")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::LAST_EVENT)
                .and_then(|value| value.as_str()),
            Some("Stage completed")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::WAITING_ON)
                .and_then(|value| value.as_str()),
            Some(SchedulerStageWaitingOn::None.as_str())
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVITY)
                .and_then(|value| value.as_str()),
            Some("Answered (1)\n- Proceed with schema migration?: Yes")
        );
    }

    #[tokio::test]
    async fn lifecycle_hook_accumulates_stage_usage_metadata() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        );

        hook.on_scheduler_stage_start("prometheus", "plan", 2, None, &exec_ctx)
            .await;
        hook.on_scheduler_stage_usage(
            "plan",
            2,
            &rocode_orchestrator::runtime::events::StepUsage {
                prompt_tokens: 1200,
                completion_tokens: 320,
                reasoning_tokens: 40,
                cache_read_tokens: 2,
                cache_write_tokens: 1,
            },
            false,
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_usage(
            "plan",
            2,
            &rocode_orchestrator::runtime::events::StepUsage {
                prompt_tokens: 1300,
                completion_tokens: 340,
                reasoning_tokens: 0,
                cache_read_tokens: 2,
                cache_write_tokens: 1,
            },
            true,
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::PROMPT_TOKENS)
                .and_then(|value| value.as_u64()),
            Some(1300)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::COMPLETION_TOKENS)
                .and_then(|value| value.as_u64()),
            Some(340)
        );
        let usage = message.usage.as_ref().expect("usage should exist");
        assert_eq!(usage.input_tokens, 1300);
        assert_eq!(usage.output_tokens, 340);
        assert_eq!(usage.reasoning_tokens, 40);
        assert_eq!(usage.cache_read_tokens, 2);
        assert_eq!(usage.cache_write_tokens, 1);
        // No model pricing attached → total_cost defaults to 0.
        assert_eq!(usage.total_cost, 0.0);
    }

    #[tokio::test]
    async fn lifecycle_hook_computes_total_cost_with_pricing() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };
        // Anthropic Sonnet-like pricing: input $3/M, output $15/M,
        // cache_read $0.30/M, cache_write $3.75/M.
        let pricing = ModelPricing::new(3.0, 15.0, Some(0.30), Some(3.75));
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        )
        .with_model_pricing(Some(pricing));

        hook.on_scheduler_stage_start("prometheus", "plan", 2, None, &exec_ctx)
            .await;
        hook.on_scheduler_stage_usage(
            "plan",
            2,
            &rocode_orchestrator::runtime::events::StepUsage {
                prompt_tokens: 1_000_000,
                completion_tokens: 100_000,
                reasoning_tokens: 0,
                cache_read_tokens: 500_000,
                cache_write_tokens: 200_000,
            },
            true,
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        let usage = message.usage.as_ref().expect("usage should exist");
        // Expected: 3.0 * 1M/1M + 15.0 * 100K/1M + 0.30 * 500K/1M + 3.75 * 200K/1M
        //         = 3.0     + 1.5      + 0.15       + 0.75
        //         = 5.40
        let expected = 3.0 + 1.5 + 0.15 + 0.75;
        assert!(
            (usage.total_cost - expected).abs() < 1e-10,
            "expected total_cost ≈ {}, got {}",
            expected,
            usage.total_cost
        );
    }

    #[tokio::test]
    async fn lifecycle_hook_merges_split_stage_usage_snapshots() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        );

        hook.on_scheduler_stage_start("atlas", "coordination-gate", 2, None, &exec_ctx)
            .await;
        hook.on_scheduler_stage_usage(
            "coordination-gate",
            2,
            &rocode_orchestrator::runtime::events::StepUsage {
                prompt_tokens: 1200,
                completion_tokens: 0,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            false,
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_usage(
            "coordination-gate",
            2,
            &rocode_orchestrator::runtime::events::StepUsage {
                prompt_tokens: 0,
                completion_tokens: 320,
                reasoning_tokens: 40,
                cache_read_tokens: 2,
                cache_write_tokens: 1,
            },
            true,
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::PROMPT_TOKENS)
                .and_then(|value| value.as_u64()),
            Some(1200)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::COMPLETION_TOKENS)
                .and_then(|value| value.as_u64()),
            Some(320)
        );
        let usage = message.usage.as_ref().expect("usage should exist");
        assert_eq!(usage.input_tokens, 1200);
        assert_eq!(usage.output_tokens, 320);
        assert_eq!(usage.reasoning_tokens, 40);
        assert_eq!(usage.cache_read_tokens, 2);
        assert_eq!(usage.cache_write_tokens, 1);
    }

    #[tokio::test]
    async fn lifecycle_hook_tracks_active_stage_capabilities_from_tool_args() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        );

        hook.on_scheduler_stage_start(
            "atlas",
            "execution-orchestration",
            2,
            Some(&SchedulerStageCapabilities {
                skill_list: vec!["debug".to_string(), "frontend-ui-ux".to_string()],
                agents: vec!["build".to_string(), "explore".to_string()],
                categories: vec!["frontend".to_string()],
                child_session: false,
            }),
            &exec_ctx,
        )
        .await;
        hook.on_tool_start(
            "atlas",
            "tc_task_flow_1",
            BuiltinToolName::TaskFlow.as_str(),
            &serde_json::json!({
                "operation": "create",
                "agent": "build",
                "load_skills": ["frontend-ui-ux"],
                "category": "frontend",
                "description": "Implement UI polish"
            }),
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::AVAILABLE_SKILL_COUNT)
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::AVAILABLE_AGENT_COUNT)
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::AVAILABLE_CATEGORY_COUNT)
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVE_AGENTS)
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("build")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVE_SKILLS)
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("frontend-ui-ux")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVE_CATEGORIES)
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("frontend")
        );
    }

    #[tokio::test]
    async fn lifecycle_hook_routes_child_session_content_to_child_session() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let emitted = Arc::new(StdMutex::new(Vec::<OutputBlockEvent>::new()));
        let emitted_hook = emitted.clone();
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        )
        .with_output_hook(Some(Arc::new(move |event| {
            let emitted_hook = emitted_hook.clone();
            Box::pin(async move {
                emitted_hook
                    .lock()
                    .expect("output block lock should not poison")
                    .push(event);
            })
        })));

        hook.on_scheduler_stage_start(
            "atlas",
            "execution-orchestration",
            2,
            Some(&SchedulerStageCapabilities {
                skill_list: vec![],
                agents: vec![],
                categories: vec![],
                child_session: true,
            }),
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_content(
            "execution-orchestration",
            2,
            "child session streamed content",
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_reasoning(
            "execution-orchestration",
            2,
            "child session streamed reasoning",
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_end(
            "atlas",
            "execution-orchestration",
            2,
            2,
            "## Execution Orchestration\n\nFinal stage body",
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let parent = sessions
            .get(&session_id)
            .expect("parent session should exist");
        let parent_stage_message = parent.messages.last().expect("parent stage message");
        let child_session_id = parent_stage_message
            .metadata
            .get(scheduler_keys::CHILD_SESSION_ID)
            .and_then(|value| value.as_str())
            .expect("child session id")
            .to_string();

        let child = sessions
            .get(&child_session_id)
            .expect("child session should exist");
        let child_message = child.messages.last().expect("child assistant message");
        assert_eq!(child_message.get_text(), "child session streamed content");
        assert_eq!(child_message.finish.as_deref(), Some("end_turn"));
        assert_eq!(child.parent_id.as_deref(), Some(session_id.as_str()));
        drop(sessions);

        let emitted = emitted
            .lock()
            .expect("output block lock should not poison")
            .clone();
        let child_blocks = emitted
            .into_iter()
            .filter(|event| event.session_id == child_session_id)
            .map(|event| event.block)
            .collect::<Vec<_>>();
        assert!(matches!(
            child_blocks.as_slice(),
            [
                OutputBlock::Message(message_start),
                OutputBlock::Message(message_delta),
                OutputBlock::Reasoning(reasoning_start),
                OutputBlock::Reasoning(reasoning_delta),
                OutputBlock::Reasoning(reasoning_end),
                OutputBlock::Message(message_end),
            ] if message_start == &MessageBlock::start(OutputMessageRole::Assistant)
                && message_delta
                    == &MessageBlock::delta(
                        OutputMessageRole::Assistant,
                        "child session streamed content",
                    )
                && reasoning_start == &ReasoningBlock::start()
                && reasoning_delta == &ReasoningBlock::delta("child session streamed reasoning")
                && reasoning_end == &ReasoningBlock::end()
                && message_end == &MessageBlock::end(OutputMessageRole::Assistant)
        ));
    }

    #[tokio::test]
    async fn lifecycle_hook_emits_reasoning_blocks_for_non_child_session() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let emitted = Arc::new(StdMutex::new(Vec::<OutputBlockEvent>::new()));
        let emitted_hook = emitted.clone();
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        )
        .with_output_hook(Some(Arc::new(move |event| {
            let emitted_hook = emitted_hook.clone();
            Box::pin(async move {
                emitted_hook
                    .lock()
                    .expect("output block lock should not poison")
                    .push(event);
            })
        })));

        // Start stage without child session (child_session: false)
        hook.on_scheduler_stage_start(
            "atlas",
            "execution-orchestration",
            1,
            Some(&SchedulerStageCapabilities {
                skill_list: vec![],
                agents: vec![],
                categories: vec![],
                child_session: false,
            }),
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_reasoning(
            "execution-orchestration",
            1,
            "main session reasoning",
            &exec_ctx,
        )
        .await;
        hook.on_scheduler_stage_end(
            "atlas",
            "execution-orchestration",
            1,
            1,
            "Final content",
            &exec_ctx,
        )
        .await;

        let emitted_blocks = emitted.lock().expect("emitted blocks").clone();

        // Should emit reasoning start, delta, and end blocks
        let reasoning_start = emitted_blocks.iter().find(
            |e| matches!(&e.block, OutputBlock::Reasoning(b) if b.phase == MessagePhase::Start),
        );
        let reasoning_delta = emitted_blocks.iter().find(
            |e| matches!(&e.block, OutputBlock::Reasoning(b) if b.text == "main session reasoning"),
        );
        let reasoning_end = emitted_blocks.iter().find(
            |e| matches!(&e.block, OutputBlock::Reasoning(b) if b.phase == MessagePhase::End),
        );

        assert!(
            reasoning_start.is_some(),
            "should emit reasoning start for non-child session"
        );
        assert!(
            reasoning_delta.is_some(),
            "should emit reasoning delta for non-child session"
        );
        assert!(
            reasoning_end.is_some(),
            "should emit reasoning end for non-child session"
        );
    }

    #[tokio::test]
    async fn lifecycle_hook_tracks_active_stage_capabilities_from_tool_result_metadata() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        );

        hook.on_scheduler_stage_start(
            "atlas",
            "execution-orchestration",
            2,
            Some(&SchedulerStageCapabilities {
                skill_list: vec!["debug".to_string(), "frontend-ui-ux".to_string()],
                agents: vec!["build".to_string(), "explore".to_string()],
                categories: vec!["frontend".to_string()],
                child_session: false,
            }),
            &exec_ctx,
        )
        .await;
        hook.on_tool_end(
            "atlas",
            "tc_task_flow_2",
            BuiltinToolName::TaskFlow.as_str(),
            &OrchestratorToolOutput {
                output: "delegated".to_string(),
                is_error: false,
                title: None,
                metadata: Some(serde_json::json!({
                    "delegated": true,
                    "loadedSkills": ["frontend-ui-ux"],
                    "task": {
                        "agent": "build"
                    }
                })),
            },
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVE_AGENTS)
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("build")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::ACTIVE_SKILLS)
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("frontend-ui-ux")
        );
    }

    #[tokio::test]
    async fn request_active_scheduler_stage_abort_marks_stage_cancelling() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        );

        hook.on_scheduler_stage_start("prometheus", "plan", 2, None, &exec_ctx)
            .await;

        let info = request_active_scheduler_stage_abort(&state, &session_id)
            .await
            .expect("abort info should exist");
        assert_eq!(info.stage_name.as_deref(), Some("plan"));

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STATUS)
                .and_then(|value| value.as_str()),
            Some(SchedulerStageStatus::Cancelling.as_str())
        );
    }

    #[tokio::test]
    async fn finalize_active_scheduler_stage_cancelled_marks_terminal_status() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        );

        hook.on_scheduler_stage_start("prometheus", "interview", 1, None, &exec_ctx)
            .await;
        request_active_scheduler_stage_abort(&state, &session_id).await;
        let info = finalize_active_scheduler_stage_cancelled(&state, &session_id)
            .await
            .expect("cancel info should exist");
        assert_eq!(info.stage_name.as_deref(), Some("interview"));

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_keys::STATUS)
                .and_then(|value| value.as_str()),
            Some(SchedulerStageStatus::Cancelled.as_str())
        );
        assert!(!message.metadata.contains_key(scheduler_keys::STREAMING));
    }

    #[tokio::test]
    async fn route_stage_decision_is_normalized_into_metadata() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "router".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "prometheus".to_string(),
        );

        hook.on_scheduler_stage_start("prometheus", "route", 1, None, &exec_ctx)
            .await;
        hook.on_scheduler_stage_end(
            "prometheus",
            "route",
            1,
            4,
            r#"{"mode":"orchestrate","preset":"prometheus","insert_plan_stage":false,"review_mode":"normal","rationale_summary":"planner workflow required"}"#,
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_decision_keys::KIND)
                .and_then(|value| value.as_str()),
            Some("route")
        );
        let fields = message
            .metadata
            .get(scheduler_decision_keys::FIELDS)
            .and_then(|value| value.as_array())
            .expect("decision fields should exist");
        assert!(fields.iter().any(|field| {
            field
                .get(output_keys::LABEL)
                .and_then(|value| value.as_str())
                == Some("Outcome")
                && field
                    .get(output_keys::VALUE)
                    .and_then(|value| value.as_str())
                    == Some("Orchestrate")
        }));
    }

    #[tokio::test]
    async fn gate_stage_decision_is_normalized_into_metadata() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "atlas".to_string(),
            metadata: HashMap::new(),
        };
        let hook = SessionSchedulerLifecycleHook::new(
            state.clone(),
            session_id.clone(),
            "atlas".to_string(),
        );

        hook.on_scheduler_stage_start("atlas", "coordination-gate", 2, None, &exec_ctx)
            .await;
        hook.on_scheduler_stage_end(
            "atlas",
            "coordination-gate",
            2,
            3,
            r#"{"status":"continue","summary":"Task B still lacks evidence.","next_input":"Run one more worker round on task B."}"#,
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(
            message
                .metadata
                .get(scheduler_gate_keys::STATUS)
                .and_then(|value| value.as_str()),
            Some("continue")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_gate_keys::SUMMARY)
                .and_then(|value| value.as_str()),
            Some("Task B still lacks evidence.")
        );
        assert_eq!(
            message
                .metadata
                .get(scheduler_gate_keys::NEXT_INPUT)
                .and_then(|value| value.as_str()),
            Some("Run one more worker round on task B.")
        );
    }

    #[tokio::test]
    async fn ensure_default_session_title_updates_default_title_only() {
        let mut session = Session::new("project", ".");
        session.add_user_message("Fix the scheduler event flow");
        ensure_default_session_title(
            &mut session,
            Arc::new(MockProvider {
                title: "Scheduler Event Flow".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(session.title, "Scheduler Event Flow");

        let mut auto_named = Session::new("project", ".");
        auto_named.add_user_message("Fix the scheduler event flow");
        auto_named.set_auto_title("Fix the scheduler event flow");
        ensure_default_session_title(
            &mut auto_named,
            Arc::new(MockProvider {
                title: "Refined Scheduler Title".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(auto_named.title, "Refined Scheduler Title");

        let mut named = Session::new("project", ".");
        named.set_title("Pinned Title");
        named.add_user_message("Ignored input");
        ensure_default_session_title(
            &mut named,
            Arc::new(MockProvider {
                title: "Should Not Replace".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(named.title, "Pinned Title");

        let mut legacy_buggy = Session::new("project", ".");
        legacy_buggy.add_user_message("Fix the scheduler event flow");
        legacy_buggy.set_title("Fix the scheduler event flow");
        legacy_buggy
            .add_assistant_message()
            .add_text("Implemented a proper session title refresh after the first completed turn.");
        ensure_default_session_title(
            &mut legacy_buggy,
            Arc::new(MockProvider {
                title: "Refresh Session Titles After First Turn".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(
            legacy_buggy.title,
            "Refresh Session Titles After First Turn"
        );
    }
}
