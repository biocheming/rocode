use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime_control::{ExecutionPatch, ExecutionStatus, FieldUpdate};
use crate::ServerState;
use rocode_command::output_blocks::{
    OutputBlock, SchedulerDecisionBlock, SchedulerDecisionField, SchedulerDecisionRenderSpec,
    SchedulerDecisionSection, SchedulerStageBlock,
};
use rocode_orchestrator::{
    parse_execution_gate_decision, parse_route_decision, scheduler_stage_observability,
    ExecutionContext as OrchestratorExecutionContext, LifecycleHook, RouteDecision,
    SchedulerExecutionGateDecision, SchedulerStageCapabilities,
    ToolOutput as OrchestratorToolOutput,
};
use rocode_provider::Provider;
use rocode_session::{MessageRole, MessageUsage, PartType, Session, SessionMessage};

pub type SessionOutputBlockHook = Arc<dyn Fn(OutputBlock) + Send + Sync>;

#[derive(Clone)]
struct ActiveStageMessage {
    message_id: String,
    execution_id: String,
    stage_name: String,
    step_count: u32,
    committed_usage: rocode_orchestrator::runtime::events::StepUsage,
    live_usage: rocode_orchestrator::runtime::events::StepUsage,
}

#[derive(Clone)]
pub(crate) struct SessionSchedulerLifecycleHook {
    state: Arc<ServerState>,
    session_id: String,
    scheduler_profile: String,
    output_hook: Option<SessionOutputBlockHook>,
    /// Tracks the currently streaming stage messages as a stack.
    active_stage_messages: Arc<Mutex<Vec<ActiveStageMessage>>>,
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
        }
    }

    pub(crate) fn with_output_hook(mut self, output_hook: Option<SessionOutputBlockHook>) -> Self {
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
        emit_scheduler_stage_message(
            &self.state,
            &self.session_id,
            &self.scheduler_profile,
            stage_name,
            stage_index,
            stage_total,
            content,
            exec_ctx,
        )
        .await;
    }

    async fn update_active_stage_message<F>(&self, mut update: F, source: &'static str)
    where
        F: FnMut(&mut SessionMessage, &mut ActiveStageMessage),
    {
        let active = {
            let guard = self.active_stage_messages.lock().await;
            guard.last().cloned()
        };
        let Some(active) = active else {
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
            let mut updated = active;
            update(message, &mut updated);
            runtime_patch = Some(stage_execution_patch_from_message(message));
            execution_id = Some(updated.execution_id.clone());
            message_snapshot = Some(message.clone());
            session.touch();
            sessions.update(session);
            drop(sessions);

            let mut guard = self.active_stage_messages.lock().await;
            if let Some(last) = guard.last_mut() {
                if last.message_id == updated.message_id {
                    *last = updated;
                }
            }

            self.state.broadcast(
                &serde_json::json!({
                    "type": "session.updated",
                    "sessionID": &self.session_id,
                    "source": source,
                })
                .to_string(),
            );
        }

        if let Some(message) = message_snapshot.as_ref() {
            self.emit_stage_block(message);
        }

        if let (Some(execution_id), Some(patch)) = (execution_id, runtime_patch) {
            self.state
                .runtime_control
                .update_scheduler_stage(&execution_id, patch)
                .await;
        }
    }

    fn emit_stage_block(&self, message: &SessionMessage) {
        let Some(output_hook) = self.output_hook.as_ref() else {
            return;
        };
        if let Some(block) = scheduler_stage_block_from_message(message) {
            output_hook(OutputBlock::SchedulerStage(block));
        }
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
                "scheduler_stage_status".to_string(),
                serde_json::json!("cancelling"),
            );
            message.metadata.insert(
                "scheduler_stage_waiting_on".to_string(),
                serde_json::json!("none"),
            );
            message.metadata.insert(
                "scheduler_stage_last_event".to_string(),
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
            message.metadata.remove("scheduler_stage_streaming");
            message.metadata.insert(
                "scheduler_stage_status".to_string(),
                serde_json::json!("cancelled"),
            );
            message.metadata.insert(
                "scheduler_stage_waiting_on".to_string(),
                serde_json::json!("none"),
            );
            message.metadata.insert(
                "scheduler_stage_last_event".to_string(),
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
    let Some(mut session) = sessions.get(session_id).cloned() else {
        return None;
    };
    let message = find_active_scheduler_stage_message_mut(&mut session)?;
    let result = update(message)?;
    session.touch();
    sessions.update(session);
    drop(sessions);

    state.broadcast(
        &serde_json::json!({
            "type": "session.updated",
            "sessionID": session_id,
            "source": source,
        })
        .to_string(),
    );
    Some(result)
}

fn find_active_scheduler_stage_message_mut(session: &mut Session) -> Option<&mut SessionMessage> {
    session.messages.iter_mut().rev().find(|message| {
        message.role == MessageRole::Assistant
            && message
                .metadata
                .get("scheduler_stage_emitted")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            && (message
                .metadata
                .get("scheduler_stage_streaming")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                || matches!(
                    message
                        .metadata
                        .get("scheduler_stage_status")
                        .and_then(|value| value.as_str()),
                    Some("running" | "waiting" | "cancelling")
                ))
    })
}

fn scheduler_abort_info(message: &SessionMessage) -> SchedulerAbortInfo {
    SchedulerAbortInfo {
        execution_id: Some(message.id.clone()),
        scheduler_profile: message
            .metadata
            .get("scheduler_profile")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        stage_name: message
            .metadata
            .get("scheduler_stage")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        stage_index: message
            .metadata
            .get("scheduler_stage_index")
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
    }
}

fn write_stage_usage_totals(
    message: &mut SessionMessage,
    committed_usage: &rocode_orchestrator::runtime::events::StepUsage,
    live_usage: &rocode_orchestrator::runtime::events::StepUsage,
    allow_zero_fields: bool,
) {
    let prompt_tokens = committed_usage.prompt_tokens + live_usage.prompt_tokens;
    let completion_tokens = committed_usage.completion_tokens + live_usage.completion_tokens;
    let reasoning_tokens = committed_usage.reasoning_tokens + live_usage.reasoning_tokens;
    let cache_read_tokens = committed_usage.cache_read_tokens + live_usage.cache_read_tokens;
    let cache_write_tokens = committed_usage.cache_write_tokens + live_usage.cache_write_tokens;

    let mut has_any_visible_usage = false;
    for (key, value) in [
        ("scheduler_stage_prompt_tokens", prompt_tokens),
        ("scheduler_stage_completion_tokens", completion_tokens),
        ("scheduler_stage_reasoning_tokens", reasoning_tokens),
        ("scheduler_stage_cache_read_tokens", cache_read_tokens),
        ("scheduler_stage_cache_write_tokens", cache_write_tokens),
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
        message.usage = Some(MessageUsage {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            reasoning_tokens,
            cache_write_tokens,
            cache_read_tokens,
            total_cost: message
                .usage
                .as_ref()
                .map(|usage| usage.total_cost)
                .unwrap_or(0.0),
        });
    } else {
        message.usage = None;
    }
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

    async fn on_step_start(&self, _: &str, _: &str, _: u32, _: &OrchestratorExecutionContext) {
        self.update_active_stage_message(
            |message, active| {
                active.step_count += 1;
                active.live_usage = rocode_orchestrator::runtime::events::StepUsage::default();
                write_stage_usage_totals(
                    message,
                    &active.committed_usage,
                    &active.live_usage,
                    false,
                );
                message.metadata.insert(
                    "scheduler_stage_step".to_string(),
                    serde_json::json!(active.step_count),
                );
                message.metadata.insert(
                    "scheduler_stage_status".to_string(),
                    serde_json::json!("running"),
                );
                message.metadata.insert(
                    "scheduler_stage_last_event".to_string(),
                    serde_json::json!(format!("Step {} started", active.step_count)),
                );
                message.metadata.insert(
                    "scheduler_stage_waiting_on".to_string(),
                    serde_json::json!("model"),
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
        let parent_id = {
            let guard = self.active_stage_messages.lock().await;
            guard.last().map(|s| s.execution_id.clone())
        };
        self.state
            .runtime_control
            .register_tool_call(tool_call_id, &self.session_id, tool_name, parent_id)
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
                        "scheduler_stage_activity".to_string(),
                        serde_json::json!(activity),
                    );
                }
                if tool_name.eq_ignore_ascii_case("question") {
                    message.metadata.insert(
                        "scheduler_stage_status".to_string(),
                        serde_json::json!("waiting"),
                    );
                    message.metadata.insert(
                        "scheduler_stage_waiting_on".to_string(),
                        serde_json::json!("user"),
                    );
                    message.metadata.insert(
                        "scheduler_stage_last_event".to_string(),
                        serde_json::json!("Waiting for user answer"),
                    );
                } else {
                    message.metadata.insert(
                        "scheduler_stage_status".to_string(),
                        serde_json::json!("running"),
                    );
                    message.metadata.insert(
                        "scheduler_stage_waiting_on".to_string(),
                        serde_json::json!("tool"),
                    );
                    message.metadata.insert(
                        "scheduler_stage_last_event".to_string(),
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
                        "scheduler_stage_activity".to_string(),
                        serde_json::json!(activity),
                    );
                }
                message.metadata.insert(
                    "scheduler_stage_status".to_string(),
                    serde_json::json!("running"),
                );
                message.metadata.insert(
                    "scheduler_stage_waiting_on".to_string(),
                    serde_json::json!("model"),
                );
                let event = if tool_name.eq_ignore_ascii_case("question") {
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
                    "scheduler_stage_last_event".to_string(),
                    serde_json::json!(event),
                );
            },
            "prompt.scheduler.stage.tool.end",
        )
        .await;
    }

    async fn on_orchestration_end(&self, _: &str, _: u32, _: &OrchestratorExecutionContext) {}

    async fn on_scheduler_stage_start(
        &self,
        _agent_name: &str,
        stage_name: &str,
        stage_index: u32,
        capabilities: Option<&SchedulerStageCapabilities>,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        let message = session.add_assistant_message();
        let message_id = message.id.clone();
        let execution_id = message_id.clone();
        message.metadata.insert(
            "scheduler_profile".to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message.metadata.insert(
            "resolved_scheduler_profile".to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message
            .metadata
            .insert("scheduler_stage".to_string(), serde_json::json!(stage_name));
        message.metadata.insert(
            "scheduler_stage_index".to_string(),
            serde_json::json!(stage_index),
        );
        message.metadata.insert(
            "scheduler_stage_emitted".to_string(),
            serde_json::json!(true),
        );
        message.metadata.insert(
            "scheduler_stage_agent".to_string(),
            serde_json::json!(&exec_ctx.agent_name),
        );
        message.metadata.insert(
            "scheduler_stage_streaming".to_string(),
            serde_json::json!(true),
        );
        message.metadata.insert(
            "scheduler_stage_status".to_string(),
            serde_json::json!("running"),
        );
        message.metadata.insert(
            "scheduler_stage_focus".to_string(),
            serde_json::json!(scheduler_stage_focus(stage_name)),
        );
        message.metadata.insert(
            "scheduler_stage_last_event".to_string(),
            serde_json::json!("Stage started"),
        );
        message.metadata.insert(
            "scheduler_stage_waiting_on".to_string(),
            serde_json::json!("model"),
        );
        if let Some(observability) =
            scheduler_stage_observability(&self.scheduler_profile, stage_name)
        {
            message.metadata.insert(
                "scheduler_stage_projection".to_string(),
                serde_json::json!(observability.projection),
            );
            message.metadata.insert(
                "scheduler_stage_tool_policy".to_string(),
                serde_json::json!(observability.tool_policy),
            );
            message.metadata.insert(
                "scheduler_stage_loop_budget".to_string(),
                serde_json::json!(observability.loop_budget),
            );
        }
        // Write per-stage capability pool counts into metadata. Concrete
        // runtime usage is tracked separately from tool invocations.
        if let Some(caps) = capabilities {
            message.metadata.insert(
                "scheduler_stage_available_skill_count".to_string(),
                serde_json::json!(caps.skill_list.len()),
            );
            message.metadata.insert(
                "scheduler_stage_available_agent_count".to_string(),
                serde_json::json!(caps.agents.len()),
            );
            message.metadata.insert(
                "scheduler_stage_available_category_count".to_string(),
                serde_json::json!(caps.categories.len()),
            );
        }
        message.metadata.insert(
            "scheduler_stage_active_skills".to_string(),
            serde_json::json!(Vec::<String>::new()),
        );
        message.metadata.insert(
            "scheduler_stage_active_agents".to_string(),
            serde_json::json!(Vec::<String>::new()),
        );
        message.metadata.insert(
            "scheduler_stage_active_categories".to_string(),
            serde_json::json!(Vec::<String>::new()),
        );
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
            self.emit_stage_block(&snapshot);
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
            });

        self.state.broadcast(
            &serde_json::json!({
                "type": "session.updated",
                "sessionID": &self.session_id,
                "source": "prompt.scheduler.stage.start",
            })
            .to_string(),
        );
    }

    async fn on_scheduler_stage_content(
        &self,
        stage_name: &str,
        _stage_index: u32,
        content_delta: &str,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
        let message_id = {
            let guard = self.active_stage_messages.lock().await;
            match guard.last() {
                Some(active) => active.message_id.clone(),
                None => return,
            }
        };

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
            self.emit_stage_block(message);
        }

        self.state.broadcast(
            &serde_json::json!({
                "type": "session.updated",
                "sessionID": &self.session_id,
                "source": "prompt.scheduler.stage.content",
            })
            .to_string(),
        );
    }

    async fn on_scheduler_stage_usage(
        &self,
        _stage_name: &str,
        _stage_index: u32,
        usage: &rocode_orchestrator::runtime::events::StepUsage,
        finalized: bool,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
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
                        "scheduler_stage_total".to_string(),
                        serde_json::json!(stage_total),
                    );
                    message.metadata.remove("scheduler_stage_streaming");
                    message.metadata.insert(
                        "scheduler_stage_status".to_string(),
                        serde_json::json!(if body.starts_with("Stage error:") {
                            "blocked"
                        } else {
                            "done"
                        }),
                    );
                    message.metadata.insert(
                        "scheduler_stage_focus".to_string(),
                        serde_json::json!(scheduler_stage_focus(stage_name)),
                    );
                    message.metadata.insert(
                        "scheduler_stage_last_event".to_string(),
                        serde_json::json!(if body.starts_with("Stage error:") {
                            "Stage failed"
                        } else {
                            "Stage completed"
                        }),
                    );
                    message.metadata.insert(
                        "scheduler_stage_waiting_on".to_string(),
                        serde_json::json!("none"),
                    );
                    if active.step_count > 0 {
                        message.metadata.insert(
                            "scheduler_stage_step".to_string(),
                            serde_json::json!(active.step_count),
                        );
                    }
                    apply_scheduler_decision_metadata(stage_name, message);
                    message_snapshot = Some(message.clone());
                }
                session.touch();
                sessions.update(session);
                drop(sessions);

                if let Some(message) = message_snapshot.as_ref() {
                    self.emit_stage_block(message);
                }

                self.state.broadcast(
                    &serde_json::json!({
                        "type": "session.updated",
                        "sessionID": &self.session_id,
                        "source": "prompt.scheduler.stage",
                    })
                    .to_string(),
                );
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
            .get("scheduler_stage_status")
            .and_then(|value| value.as_str())
            .and_then(runtime_execution_status_from_stage_status),
        waiting_on: message
            .metadata
            .get("scheduler_stage_waiting_on")
            .and_then(|value| value.as_str())
            .filter(|value| *value != "none" && !value.is_empty())
            .map(|value| FieldUpdate::Set(value.to_string()))
            .unwrap_or(FieldUpdate::Clear),
        recent_event: message
            .metadata
            .get("scheduler_stage_last_event")
            .and_then(|value| value.as_str())
            .map(|value| FieldUpdate::Set(value.to_string()))
            .unwrap_or(FieldUpdate::Keep),
        metadata: FieldUpdate::Set(scheduler_stage_runtime_metadata(message)),
        ..ExecutionPatch::default()
    }
}

fn runtime_execution_status_from_stage_status(value: &str) -> Option<ExecutionStatus> {
    match value {
        "running" => Some(ExecutionStatus::Running),
        "waiting" => Some(ExecutionStatus::Waiting),
        "cancelling" => Some(ExecutionStatus::Cancelling),
        "retry" => Some(ExecutionStatus::Retry),
        _ => None,
    }
}

fn scheduler_stage_runtime_metadata(message: &SessionMessage) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    for key in [
        "scheduler_profile",
        "resolved_scheduler_profile",
        "scheduler_stage",
        "scheduler_stage_index",
        "scheduler_stage_total",
        "scheduler_stage_agent",
        "scheduler_stage_step",
        "scheduler_stage_focus",
        "scheduler_stage_projection",
        "scheduler_stage_tool_policy",
        "scheduler_stage_loop_budget",
        "scheduler_stage_activity",
        "scheduler_stage_available_skill_count",
        "scheduler_stage_available_agent_count",
        "scheduler_stage_available_category_count",
        "scheduler_stage_active_skills",
        "scheduler_stage_active_agents",
        "scheduler_stage_active_categories",
        "scheduler_stage_prompt_tokens",
        "scheduler_stage_completion_tokens",
        "scheduler_stage_reasoning_tokens",
        "scheduler_stage_cache_read_tokens",
        "scheduler_stage_cache_write_tokens",
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
        "scheduler_profile".to_string(),
        serde_json::json!(scheduler_profile),
    );
    metadata.insert("scheduler_stage".to_string(), serde_json::json!(stage_name));
    metadata.insert(
        "scheduler_stage_index".to_string(),
        serde_json::json!(stage_index),
    );
    if let Some(stage_total) = stage_total {
        metadata.insert(
            "scheduler_stage_total".to_string(),
            serde_json::json!(stage_total),
        );
    }
    metadata.insert(
        "scheduler_stage_agent".to_string(),
        serde_json::json!(agent_name),
    );
    metadata.insert(
        "scheduler_stage_focus".to_string(),
        serde_json::json!(scheduler_stage_focus(stage_name)),
    );
    serde_json::Value::Object(metadata)
}

fn summarize_tool_activity(tool_name: &str, tool_args: &serde_json::Value) -> Option<String> {
    match tool_name.to_ascii_lowercase().as_str() {
        "question" => summarize_question_args(tool_args),
        "todowrite" | "todo_write" => summarize_todo_args(tool_args),
        "todoread" | "todo_read" => Some("Todo list read".to_string()),
        "task" => summarize_task_args(tool_args),
        "task_flow" => summarize_task_flow_args(tool_args),
        _ => summarize_generic_tool_args(tool_name, tool_args),
    }
}

fn summarize_tool_result_activity(
    tool_name: &str,
    tool_output: &OrchestratorToolOutput,
) -> Option<String> {
    match tool_name.to_ascii_lowercase().as_str() {
        "question" => summarize_question_result(tool_output.metadata.as_ref()),
        "todowrite" | "todo_write" | "todoread" | "todo_read" => {
            summarize_todo_result(tool_output.metadata.as_ref())
        }
        _ => None,
    }
}

fn summarize_question_args(tool_args: &serde_json::Value) -> Option<String> {
    let questions = tool_args.get("questions")?.as_array()?;
    if questions.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Question ({})", questions.len())];
    for question in questions.iter().take(3) {
        let header = question
            .get("header")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("Prompt");
        let text = question
            .get("question")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if !text.is_empty() {
            lines.push(format!("- {header}: {}", collapse_text(text, 96)));
        }
    }
    Some(lines.join("\n"))
}

fn summarize_todo_args(tool_args: &serde_json::Value) -> Option<String> {
    let todos = tool_args.get("todos")?.as_array()?;
    if todos.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Todo list ({})", todos.len())];
    for todo in todos.iter().take(5) {
        let content = todo.get("content").and_then(|value| value.as_str())?;
        let status = todo
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("pending");
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
        if let Some(content) = todo_item.get("content").and_then(|value| value.as_str()) {
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
    matches!(tool_name, "task" | "task_flow")
}

fn tool_supports_stage_capability_activity_output(
    tool_name: &str,
    metadata: &serde_json::Value,
) -> bool {
    matches!(tool_name, "task" | "task_flow")
        || metadata
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
        push_stage_active_value(message, "scheduler_stage_active_agents", &agent);
    }
    for category in evidence.categories {
        push_stage_active_value(message, "scheduler_stage_active_categories", &category);
    }
    for skill in evidence.skills {
        push_stage_active_value(message, "scheduler_stage_active_skills", &skill);
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
    let raw = collapse_text(&tool_args.to_string(), 120);
    Some(format!(
        "{} → {}",
        pretty_scheduler_stage_name(tool_name),
        raw
    ))
}

fn summarize_question_result(metadata: Option<&serde_json::Value>) -> Option<String> {
    let fields = metadata?
        .get("display.fields")
        .and_then(|value| value.as_array())?;
    if fields.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Answered ({})", fields.len())];
    for field in fields.iter().take(3) {
        let key = field
            .get("key")
            .and_then(|value| value.as_str())
            .unwrap_or("Question");
        let value = field
            .get("value")
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
    let todos = metadata?.get("todos").and_then(|value| value.as_array())?;
    if todos.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Todo list ({})", todos.len())];
    for todo in todos.iter().take(5) {
        let content = todo.get("content").and_then(|value| value.as_str())?;
        let status = todo
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("pending");
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

fn apply_scheduler_decision_metadata(stage_name: &str, message: &mut SessionMessage) {
    clear_scheduler_decision_metadata(message);
    let text = message.get_text();
    let body = scheduler_stage_body_text(&text);
    match stage_name {
        "route" => {
            let Some(decision) = parse_route_decision(body) else {
                return;
            };
            write_scheduler_route_metadata(message, &decision);
        }
        "coordination-gate" | "autonomous-gate" => {
            let Some(decision) = parse_execution_gate_decision(body) else {
                return;
            };
            write_scheduler_gate_metadata(message, &decision);
        }
        _ => {}
    }
}

fn clear_scheduler_decision_metadata(message: &mut SessionMessage) {
    for key in [
        "scheduler_decision_kind",
        "scheduler_decision_title",
        "scheduler_decision_fields",
        "scheduler_decision_sections",
        "scheduler_gate_status",
        "scheduler_gate_summary",
        "scheduler_gate_next_input",
        "scheduler_gate_final_response",
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

    write_scheduler_decision_metadata(message, "route", "Decision", fields, sections);
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
    write_scheduler_decision_metadata(message, "gate", "Decision", fields, sections);
    message.metadata.insert(
        "scheduler_gate_status".to_string(),
        serde_json::json!(status),
    );
    if !decision.summary.is_empty() {
        message.metadata.insert(
            "scheduler_gate_summary".to_string(),
            serde_json::json!(decision.summary),
        );
    }
    if let Some(next_input) = decision
        .next_input
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        message.metadata.insert(
            "scheduler_gate_next_input".to_string(),
            serde_json::json!(next_input),
        );
    }
    if let Some(final_response) = decision
        .final_response
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        message.metadata.insert(
            "scheduler_gate_final_response".to_string(),
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
        "scheduler_decision_kind".to_string(),
        serde_json::json!(kind),
    );
    message.metadata.insert(
        "scheduler_decision_title".to_string(),
        serde_json::json!(title),
    );
    message.metadata.insert(
        "scheduler_decision_fields".to_string(),
        serde_json::json!(fields),
    );
    message.metadata.insert(
        "scheduler_decision_sections".to_string(),
        serde_json::json!(sections),
    );
    message.metadata.insert(
        "scheduler_decision_spec".to_string(),
        scheduler_decision_render_spec_json(),
    );
}

fn decision_field(label: &str, value: &str, tone: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "label": label,
        "value": value,
        "tone": tone,
    })
}

fn decision_section(title: &str, body: &str) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "body": body,
    })
}

fn scheduler_decision_render_spec_json() -> serde_json::Value {
    serde_json::json!({
        "version": "decision-card/v1",
        "show_header_divider": true,
        "field_order": "as-provided",
        "field_label_emphasis": "bold",
        "status_palette": "semantic",
        "section_spacing": "loose",
        "update_policy": "stable-shell-live-runtime-append-decision",
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

fn scheduler_stage_body_text(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("## ") {
        if let Some((_, body)) = rest.split_once("\n\n") {
            return body.trim();
        }
        if let Some((_, body)) = rest.split_once('\n') {
            return body.trim();
        }
    }
    trimmed
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
    match stage_name {
        "route" => "Decide the correct workflow and preserve request intent.",
        "interview" => "Clarify scope, requirements, and blocking ambiguities.",
        "plan" => "Draft the executable plan and its guardrails.",
        "review" => "Audit the current artifact for gaps and readiness.",
        "handoff" => "Prepare the next-step handoff for execution or approval.",
        "execution-orchestration" => "Drive the active execution workflow to concrete results.",
        "synthesis" => "Merge stage outputs into a final user-facing delivery.",
        "coordination-verification" => "Verify delegated work against actual evidence.",
        "coordination-gate" => "Decide whether the coordination loop can finish.",
        "coordination-retry" => "Prepare the bounded retry focus for the next round.",
        "autonomous-verification" => "Verify autonomous execution against the task boundary.",
        "autonomous-gate" => "Decide whether autonomous execution is complete.",
        "autonomous-retry" => "Prepare the bounded recovery retry.",
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

pub(crate) async fn emit_scheduler_stage_message(
    state: &Arc<ServerState>,
    session_id: &str,
    scheduler_profile: &str,
    stage_name: &str,
    stage_index: u32,
    stage_total: u32,
    content: &str,
    exec_ctx: &OrchestratorExecutionContext,
) {
    let body = content.trim();
    if body.is_empty() {
        return;
    }

    let mut sessions = state.sessions.lock().await;
    let Some(mut session) = sessions.get(session_id).cloned() else {
        return;
    };

    let message = session.add_assistant_message();
    message.metadata.insert(
        "scheduler_profile".to_string(),
        serde_json::json!(scheduler_profile),
    );
    message.metadata.insert(
        "resolved_scheduler_profile".to_string(),
        serde_json::json!(scheduler_profile),
    );
    message
        .metadata
        .insert("scheduler_stage".to_string(), serde_json::json!(stage_name));
    message.metadata.insert(
        "scheduler_stage_index".to_string(),
        serde_json::json!(stage_index),
    );
    message.metadata.insert(
        "scheduler_stage_total".to_string(),
        serde_json::json!(stage_total),
    );
    message.metadata.insert(
        "scheduler_stage_emitted".to_string(),
        serde_json::json!(true),
    );
    message.metadata.insert(
        "scheduler_stage_agent".to_string(),
        serde_json::json!(exec_ctx.agent_name.clone()),
    );
    message.metadata.insert(
        "scheduler_stage_status".to_string(),
        serde_json::json!(if body.starts_with("Stage error:") {
            "blocked"
        } else {
            "done"
        }),
    );
    message.metadata.insert(
        "scheduler_stage_focus".to_string(),
        serde_json::json!(scheduler_stage_focus(stage_name)),
    );
    message.metadata.insert(
        "scheduler_stage_last_event".to_string(),
        serde_json::json!(if body.starts_with("Stage error:") {
            "Stage failed"
        } else {
            "Stage completed"
        }),
    );
    message.metadata.insert(
        "scheduler_stage_waiting_on".to_string(),
        serde_json::json!("none"),
    );
    if let Some(observability) = scheduler_stage_observability(scheduler_profile, stage_name) {
        message.metadata.insert(
            "scheduler_stage_projection".to_string(),
            serde_json::json!(observability.projection),
        );
        message.metadata.insert(
            "scheduler_stage_tool_policy".to_string(),
            serde_json::json!(observability.tool_policy),
        );
        message.metadata.insert(
            "scheduler_stage_loop_budget".to_string(),
            serde_json::json!(observability.loop_budget),
        );
    }
    message.add_text(body.to_string());
    apply_scheduler_decision_metadata(stage_name, message);
    session.touch();
    sessions.update(session);
    drop(sessions);

    state.broadcast(
        &serde_json::json!({
            "type": "session.updated",
            "sessionID": session_id,
            "source": "prompt.scheduler.stage",
        })
        .to_string(),
    );
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
    let stage = metadata.get("scheduler_stage")?.as_str()?.to_string();
    let text = assistant_visible_text(message);
    let title = scheduler_stage_title_from_text(&text)
        .unwrap_or_else(|| pretty_scheduler_stage_title(metadata, &stage));

    Some(SchedulerStageBlock {
        profile: metadata
            .get("resolved_scheduler_profile")
            .or_else(|| metadata.get("scheduler_profile"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        stage: stage.clone(),
        title,
        text: scheduler_stage_body(&text),
        stage_index: metadata
            .get("scheduler_stage_index")
            .and_then(|value| value.as_u64()),
        stage_total: metadata
            .get("scheduler_stage_total")
            .and_then(|value| value.as_u64()),
        step: metadata
            .get("scheduler_stage_step")
            .and_then(|value| value.as_u64()),
        status: metadata
            .get("scheduler_stage_status")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        focus: metadata
            .get("scheduler_stage_focus")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        last_event: metadata
            .get("scheduler_stage_last_event")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        waiting_on: metadata
            .get("scheduler_stage_waiting_on")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        activity: metadata
            .get("scheduler_stage_activity")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        available_skill_count: metadata
            .get("scheduler_stage_available_skill_count")
            .and_then(|value| value.as_u64()),
        available_agent_count: metadata
            .get("scheduler_stage_available_agent_count")
            .and_then(|value| value.as_u64()),
        available_category_count: metadata
            .get("scheduler_stage_available_category_count")
            .and_then(|value| value.as_u64()),
        active_skills: metadata
            .get("scheduler_stage_active_skills")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        active_agents: metadata
            .get("scheduler_stage_active_agents")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        active_categories: metadata
            .get("scheduler_stage_active_categories")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        prompt_tokens: metadata
            .get("scheduler_stage_prompt_tokens")
            .and_then(|value| value.as_u64()),
        completion_tokens: metadata
            .get("scheduler_stage_completion_tokens")
            .and_then(|value| value.as_u64()),
        reasoning_tokens: metadata
            .get("scheduler_stage_reasoning_tokens")
            .and_then(|value| value.as_u64()),
        cache_read_tokens: metadata
            .get("scheduler_stage_cache_read_tokens")
            .and_then(|value| value.as_u64()),
        cache_write_tokens: metadata
            .get("scheduler_stage_cache_write_tokens")
            .and_then(|value| value.as_u64()),
        decision: scheduler_decision_block(metadata, &stage, &text),
    })
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
        .get("scheduler_decision_kind")
        .and_then(|value| value.as_str())?
        .to_string();
    let title = metadata
        .get("scheduler_decision_title")
        .and_then(|value| value.as_str())
        .unwrap_or("Decision")
        .to_string();
    Some(SchedulerDecisionBlock {
        kind,
        title,
        spec: decision_spec_from_metadata(metadata).unwrap_or_else(default_decision_render_spec),
        fields: metadata
            .get("scheduler_decision_fields")
            .and_then(|value| value.as_array())
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|field| {
                        Some(SchedulerDecisionField {
                            label: field.get("label")?.as_str()?.to_string(),
                            value: field.get("value")?.as_str()?.to_string(),
                            tone: field
                                .get("tone")
                                .and_then(|value| value.as_str())
                                .map(|value| value.to_string()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        sections: metadata
            .get("scheduler_decision_sections")
            .and_then(|value| value.as_array())
            .map(|sections| {
                sections
                    .iter()
                    .filter_map(|section| {
                        Some(SchedulerDecisionSection {
                            title: section.get("title")?.as_str()?.to_string(),
                            body: section.get("body")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn decision_from_stage_text(stage: &str, text: &str) -> Option<SchedulerDecisionBlock> {
    let body = scheduler_stage_body(text);
    match stage {
        "route" => {
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
                kind: "route".to_string(),
                title: "Decision".to_string(),
                spec: default_decision_render_spec(),
                fields,
                sections,
            })
        }
        "coordination-gate" | "autonomous-gate" => {
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
                kind: "gate".to_string(),
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
    let spec = metadata.get("scheduler_decision_spec")?;
    Some(SchedulerDecisionRenderSpec {
        version: spec.get("version")?.as_str()?.to_string(),
        show_header_divider: spec.get("show_header_divider")?.as_bool()?,
        field_order: spec.get("field_order")?.as_str()?.to_string(),
        field_label_emphasis: spec.get("field_label_emphasis")?.as_str()?.to_string(),
        status_palette: spec.get("status_palette")?.as_str()?.to_string(),
        section_spacing: spec.get("section_spacing")?.as_str()?.to_string(),
        update_policy: spec.get("update_policy")?.as_str()?.to_string(),
    })
}

fn default_decision_render_spec() -> SchedulerDecisionRenderSpec {
    SchedulerDecisionRenderSpec {
        version: "decision-card/v1".to_string(),
        show_header_divider: true,
        field_order: "as-provided".to_string(),
        field_label_emphasis: "bold".to_string(),
        status_palette: "semantic".to_string(),
        section_spacing: "loose".to_string(),
        update_policy: "stable-shell-live-runtime-append-decision".to_string(),
    }
}

fn scheduler_stage_title_from_text(text: &str) -> Option<String> {
    text.lines()
        .next()
        .map(str::trim)
        .and_then(|line| line.strip_prefix("## "))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
        .get("resolved_scheduler_profile")
        .or_else(|| metadata.get("scheduler_profile"))
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
        session.set_title(generated_title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use rocode_provider::{
        ChatRequest, ChatResponse, Choice, Content, Message, ModelInfo, Provider, ProviderError,
        Role, StreamResult,
    };
    use std::collections::HashMap;

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

        emit_scheduler_stage_message(
            &state,
            &session_id,
            "prometheus",
            "plan",
            3,
            4,
            "## Plan\n- step",
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert_eq!(message.get_text(), "## Plan\n- step");
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage")
                .and_then(|value| value.as_str()),
            Some("plan")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_projection")
                .and_then(|value| value.as_str()),
            Some("transcript")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_loop_budget")
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

        emit_scheduler_stage_message(
            &state,
            &session_id,
            "atlas",
            "coordination-verification",
            1,
            3,
            "## Coordination Verification\n\nMissing proof for task B.",
            &exec_ctx,
        )
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
                .get("scheduler_stage")
                .and_then(|value| value.as_str()),
            Some("coordination-verification")
        );
        assert!(message.metadata.get("scheduler_stage_projection").is_none());
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
            "question",
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
            "question",
            &OrchestratorToolOutput {
                output: "{}".to_string(),
                is_error: false,
                title: Some("User response received".to_string()),
                metadata: Some(serde_json::json!({
                    "display.fields": [{
                        "key": "Proceed with schema migration?",
                        "value": "Yes"
                    }]
                })),
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
                .get("scheduler_stage_step")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_status")
                .and_then(|value| value.as_str()),
            Some("done")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_focus")
                .and_then(|value| value.as_str()),
            Some("Draft the executable plan and its guardrails.")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_last_event")
                .and_then(|value| value.as_str()),
            Some("Stage completed")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_waiting_on")
                .and_then(|value| value.as_str()),
            Some("none")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_activity")
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
                .get("scheduler_stage_prompt_tokens")
                .and_then(|value| value.as_u64()),
            Some(1300)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_completion_tokens")
                .and_then(|value| value.as_u64()),
            Some(340)
        );
        let usage = message.usage.as_ref().expect("usage should exist");
        assert_eq!(usage.input_tokens, 1300);
        assert_eq!(usage.output_tokens, 340);
        assert_eq!(usage.reasoning_tokens, 40);
        assert_eq!(usage.cache_read_tokens, 2);
        assert_eq!(usage.cache_write_tokens, 1);
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
                .get("scheduler_stage_prompt_tokens")
                .and_then(|value| value.as_u64()),
            Some(1200)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_completion_tokens")
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
            }),
            &exec_ctx,
        )
        .await;
        hook.on_tool_start(
            "atlas",
            "tc_task_flow_1",
            "task_flow",
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
                .get("scheduler_stage_available_skill_count")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_available_agent_count")
                .and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_available_category_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_active_agents")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("build")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_active_skills")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("frontend-ui-ux")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_active_categories")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("frontend")
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
            }),
            &exec_ctx,
        )
        .await;
        hook.on_tool_end(
            "atlas",
            "tc_task_flow_2",
            "task_flow",
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
                .get("scheduler_stage_active_agents")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("build")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage_active_skills")
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
                .get("scheduler_stage_status")
                .and_then(|value| value.as_str()),
            Some("cancelling")
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
                .get("scheduler_stage_status")
                .and_then(|value| value.as_str()),
            Some("cancelled")
        );
        assert!(message.metadata.get("scheduler_stage_streaming").is_none());
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
                .get("scheduler_decision_kind")
                .and_then(|value| value.as_str()),
            Some("route")
        );
        let fields = message
            .metadata
            .get("scheduler_decision_fields")
            .and_then(|value| value.as_array())
            .expect("decision fields should exist");
        assert!(fields.iter().any(|field| {
            field.get("label").and_then(|value| value.as_str()) == Some("Outcome")
                && field.get("value").and_then(|value| value.as_str()) == Some("Orchestrate")
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
                .get("scheduler_gate_status")
                .and_then(|value| value.as_str()),
            Some("continue")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_gate_summary")
                .and_then(|value| value.as_str()),
            Some("Task B still lacks evidence.")
        );
        assert_eq!(
            message
                .metadata
                .get("scheduler_gate_next_input")
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
