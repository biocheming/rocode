use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
    Json,
};
use futures::stream::Stream;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use crate::session_runtime::{
    assistant_reasoning_text, assistant_visible_text, scheduler_stage_block_from_message,
};
use crate::session_runtime::events::{
    broadcast_session_updated, send_sse_server_event, ServerEvent,
};
use crate::{ApiError, ServerState};
use rocode_agent::{AgentInfo, AgentRegistry};
use rocode_command::agent_presenter::output_block_to_web;
use rocode_command::output_blocks::{
    MessageBlock, MessageRole, OutputBlock, ReasoningBlock, ToolBlock,
};
use rocode_provider::ToolDefinition;
use rocode_session::{MessageRole as SessionMessageRole, PartType, Session, SessionMessage};

use super::session::{
    resolve_prompt_request_config, resolved_session_directory, to_task_agent_info,
    SendMessageRequest,
};
use super::tui::{request_question_answers_with_hook, QuestionEventHook};

pub(crate) async fn send_stream_error_event(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    session_id: Option<String>,
    message_id: Option<String>,
    done: Option<bool>,
    error: String,
) {
    let event = ServerEvent::Error {
        session_id,
        error,
        message_id,
        done,
    };
    send_sse_server_event(tx, &event).await;
}

pub(crate) async fn send_stream_usage_event(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    session_id: Option<String>,
    message_id: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
) {
    let event = ServerEvent::Usage {
        session_id,
        prompt_tokens,
        completion_tokens,
        message_id,
    };
    send_sse_server_event(tx, &event).await;
}

#[derive(Default)]
struct AssistantEmitState {
    started: bool,
    emitted_text: String,
    /// Tracks the reasoning text emitted so far for delta computation.
    emitted_reasoning: String,
    /// Whether we emitted a reasoning-start block for the current message.
    reasoning_started: bool,
    ended: bool,
    usage: Option<(u64, u64)>,
}

#[derive(Default)]
struct ToolCallEmitState {
    started: bool,
    detail: Option<String>,
}

#[derive(Default)]
struct StreamSnapshotEmitter {
    assistants: HashMap<String, AssistantEmitState>,
    tool_calls: HashMap<String, ToolCallEmitState>,
    emitted_tool_result_parts: HashSet<String>,
    scheduler_stages: HashMap<String, String>,
}

impl StreamSnapshotEmitter {
    async fn emit_snapshot(
        &mut self,
        tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
        snapshot: &Session,
    ) {
        let mut tool_names = HashMap::new();

        for message in &snapshot.messages {
            match message.role {
                SessionMessageRole::Assistant => {
                    self.emit_assistant_message(tx, &snapshot.id, message, &mut tool_names)
                        .await;
                }
                SessionMessageRole::Tool => {
                    self.emit_tool_results(tx, &snapshot.id, message, &tool_names)
                        .await;
                }
                _ => {}
            }
        }
    }

    async fn emit_assistant_message(
        &mut self,
        tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
        session_id: &str,
        message: &SessionMessage,
        tool_names: &mut HashMap<String, String>,
    ) {
        if self.emit_scheduler_stage(tx, session_id, message).await {
            return;
        }

        let state = self.assistants.entry(message.id.clone()).or_default();
        if !state.started {
            emit_output_block(
                tx,
                session_id,
                OutputBlock::Message(MessageBlock::start(MessageRole::Assistant)),
                Some(message.id.as_str()),
            )
            .await;
            state.started = true;
        }

        // ── Reasoning (thinking) blocks ──
        let reasoning = assistant_reasoning_text(message);
        tracing::debug!(
            message_id = %message.id,
            reasoning_len = reasoning.len(),
            "emit_assistant_message: checking reasoning"
        );
        if !reasoning.is_empty() {
            tracing::info!(
                message_id = %message.id,
                reasoning_len = reasoning.len(),
                reasoning_preview = %reasoning.chars().take(100).collect::<String>(),
                "emit_assistant_message: emitting reasoning block"
            );
            if !state.reasoning_started {
                emit_output_block(
                    tx,
                    session_id,
                    OutputBlock::Reasoning(ReasoningBlock::start()),
                    Some(message.id.as_str()),
                )
                .await;
                state.reasoning_started = true;
            }
            let reasoning_delta = if reasoning.starts_with(&state.emitted_reasoning) {
                reasoning[state.emitted_reasoning.len()..].to_string()
            } else {
                reasoning.clone()
            };
            if !reasoning_delta.is_empty() {
                emit_output_block(
                    tx,
                    session_id,
                    OutputBlock::Reasoning(ReasoningBlock::delta(reasoning_delta)),
                    Some(message.id.as_str()),
                )
                .await;
                state.emitted_reasoning = reasoning;
            }
        }

        // ── Text blocks ──
        let text = assistant_visible_text(message);
        let delta = if text.starts_with(&state.emitted_text) {
            text[state.emitted_text.len()..].to_string()
        } else {
            text.clone()
        };
        if !delta.is_empty() {
            emit_output_block(
                tx,
                session_id,
                OutputBlock::Message(MessageBlock::delta(MessageRole::Assistant, delta)),
                Some(message.id.as_str()),
            )
            .await;
            state.emitted_text = text;
        }

        for part in &message.parts {
            let PartType::ToolCall {
                id,
                name,
                input,
                status,
                raw,
                ..
            } = &part.part_type
            else {
                continue;
            };

            let trimmed_name = name.trim();
            if trimmed_name.is_empty() {
                continue;
            }

            tool_names.insert(id.clone(), trimmed_name.to_string());
            let call_state = self.tool_calls.entry(id.clone()).or_default();
            if !call_state.started {
                emit_output_block(
                    tx,
                    session_id,
                    OutputBlock::Tool(ToolBlock::start(trimmed_name.to_string())),
                    Some(id.as_str()),
                )
                .await;
                call_state.started = true;
            }

            let detail = tool_progress_detail(input, raw.as_deref(), status);
            if detail.is_some() && detail != call_state.detail {
                emit_output_block(
                    tx,
                    session_id,
                    OutputBlock::Tool(ToolBlock::running(
                        trimmed_name.to_string(),
                        detail.clone().unwrap_or_default(),
                    )),
                    Some(id.as_str()),
                )
                .await;
                call_state.detail = detail;
            }
        }

        if let Some(usage) = message.usage.as_ref() {
            let current = (usage.input_tokens, usage.output_tokens);
            if state.usage != Some(current) {
                send_stream_usage_event(
                    tx,
                    Some(session_id.to_string()),
                    Some(message.id.clone()),
                    current.0,
                    current.1,
                )
                .await;
                state.usage = Some(current);
            }
        }

        if !state.ended && assistant_finished(message) {
            if state.reasoning_started {
                emit_output_block(
                    tx,
                    session_id,
                    OutputBlock::Reasoning(ReasoningBlock::end()),
                    Some(message.id.as_str()),
                )
                .await;
            }
            emit_output_block(
                tx,
                session_id,
                OutputBlock::Message(MessageBlock::end(MessageRole::Assistant)),
                Some(message.id.as_str()),
            )
            .await;
            state.ended = true;
        }
    }

    async fn emit_scheduler_stage(
        &mut self,
        tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
        session_id: &str,
        message: &SessionMessage,
    ) -> bool {
        let Some(block) = scheduler_stage_block_from_message(message) else {
            return false;
        };

        let signature =
            output_block_to_web(&OutputBlock::SchedulerStage(Box::new(block.clone()))).to_string();
        if self.scheduler_stages.get(&message.id) == Some(&signature) {
            return true;
        }
        self.scheduler_stages.insert(message.id.clone(), signature);

        emit_output_block(
            tx,
            session_id,
            OutputBlock::SchedulerStage(Box::new(block)),
            Some(message.id.as_str()),
        )
        .await;
        true
    }

    async fn emit_tool_results(
        &mut self,
        tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
        session_id: &str,
        message: &SessionMessage,
        tool_names: &HashMap<String, String>,
    ) {
        for part in &message.parts {
            let PartType::ToolResult {
                tool_call_id,
                content,
                is_error,
                title,
                ..
            } = &part.part_type
            else {
                continue;
            };

            if !self.emitted_tool_result_parts.insert(part.id.clone()) {
                continue;
            }

            let tool_name = tool_names
                .get(tool_call_id)
                .cloned()
                .unwrap_or_else(|| tool_call_id.clone());
            let detail = tool_result_detail(title.as_deref(), content);
            let block = if *is_error {
                OutputBlock::Tool(ToolBlock::error(
                    tool_name,
                    detail.unwrap_or_else(|| content.clone()),
                ))
            } else {
                OutputBlock::Tool(ToolBlock::done(tool_name, detail))
            };
            emit_output_block(tx, session_id, block, Some(tool_call_id.as_str())).await;
        }
    }
}

async fn emit_output_block(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    session_id: &str,
    block: OutputBlock,
    id: Option<&str>,
) {
    let event = ServerEvent::output_block(session_id.to_string(), &block, id);
    send_sse_server_event(tx, &event).await;
}

fn assistant_finished(message: &SessionMessage) -> bool {
    message.finish.is_some()
        || message.metadata.contains_key("completed_at")
        || message.metadata.contains_key("finish_reason")
}

fn tool_progress_detail(
    input: &serde_json::Value,
    raw: Option<&str>,
    status: &rocode_session::ToolCallStatus,
) -> Option<String> {
    if let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(raw.to_string());
    }

    match status {
        rocode_session::ToolCallStatus::Pending | rocode_session::ToolCallStatus::Running => {
            if input.is_null() {
                return None;
            }
            if let Some(obj) = input.as_object() {
                if obj.is_empty() {
                    return None;
                }
            }
            if let Some(arr) = input.as_array() {
                if arr.is_empty() {
                    return None;
                }
            }
            if let Some(text) = input.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            Some(input.to_string())
        }
        rocode_session::ToolCallStatus::Completed | rocode_session::ToolCallStatus::Error => None,
    }
}

fn tool_result_detail(title: Option<&str>, content: &str) -> Option<String> {
    match title.map(str::trim).filter(|value| !value.is_empty()) {
        Some(title) => Some(format!("{title}: {content}")),
        None if content.trim().is_empty() => None,
        None => Some(content.to_string()),
    }
}

fn filtered_tool_definitions(
    mut tool_defs: Vec<ToolDefinition>,
    agent: Option<&AgentInfo>,
) -> Vec<ToolDefinition> {
    if let Some(agent) = agent {
        tool_defs.retain(|tool| agent.is_tool_allowed(&tool.name));
    }
    rocode_session::prioritize_tool_definitions(&mut tool_defs);
    tool_defs
}

pub(crate) async fn stream_message(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> std::result::Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>, ApiError>
{
    if req.agent.is_some() && req.scheduler_profile.is_some() {
        return Err(ApiError::BadRequest(
            "`agent` and `scheduler_profile` are mutually exclusive".to_string(),
        ));
    }

    let request_variant = {
        let sessions = state.sessions.lock().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
        req.variant.clone().or_else(|| {
            session
                .metadata
                .get("model_variant")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        })
    };

    let config = state.config_store.config();
    let request_config = resolve_prompt_request_config(super::session::PromptRequestConfigInput {
        state: &state,
        config: &config,
        session_id: &session_id,
        requested_agent: req.agent.as_deref(),
        requested_scheduler_profile: req.scheduler_profile.as_deref(),
        request_model: req.model.as_deref(),
        request_variant: request_variant.as_deref(),
        route: "stream",
    })
    .await?;
    let scheduler_applied = request_config.scheduler_applied;
    let scheduler_profile_name = request_config.scheduler_profile_name.clone();
    let scheduler_root_agent = request_config.scheduler_root_agent.clone();
    let scheduler_skill_tree_applied = request_config.scheduler_skill_tree_applied;
    let resolved_agent = request_config.resolved_agent.clone();
    let provider = request_config.provider.clone();
    let provider_id = request_config.provider_id.clone();
    let model_id = request_config.model_id.clone();
    let agent_system_prompt = request_config.agent_system_prompt.clone();
    let compiled_request = request_config.compiled_request.clone();

    let (selected_variant, stream_session) = {
        let mut sessions = state.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;

        let normalized_directory = resolved_session_directory(&session.directory);
        if session.directory != normalized_directory {
            session.directory = normalized_directory;
        }

        let selected_variant = request_variant.clone();
        if let Some(variant) = selected_variant.as_deref() {
            session
                .metadata
                .insert("model_variant".to_string(), serde_json::json!(variant));
        } else {
            session.metadata.remove("model_variant");
        }
        session.metadata.insert(
            "model_provider".to_string(),
            serde_json::json!(provider_id.clone()),
        );
        session
            .metadata
            .insert("model_id".to_string(), serde_json::json!(model_id.clone()));
        if let Some(agent) = resolved_agent.as_ref().map(|agent| agent.name.as_str()) {
            session
                .metadata
                .insert("agent".to_string(), serde_json::json!(agent));
        } else {
            session.metadata.remove("agent");
        }
        session.metadata.insert(
            "scheduler_applied".to_string(),
            serde_json::json!(scheduler_applied),
        );
        session.metadata.insert(
            "scheduler_skill_tree_applied".to_string(),
            serde_json::json!(scheduler_skill_tree_applied),
        );
        if let Some(profile) = scheduler_profile_name.as_deref() {
            session
                .metadata
                .insert("scheduler_profile".to_string(), serde_json::json!(profile));
        } else {
            session.metadata.remove("scheduler_profile");
        }
        if let Some(root_agent) = scheduler_root_agent.as_deref() {
            session.metadata.insert(
                "scheduler_root_agent".to_string(),
                serde_json::json!(root_agent),
            );
        } else {
            session.metadata.remove("scheduler_root_agent");
        }
        session.touch();

        (selected_variant, session.clone())
    };

    broadcast_session_updated(state.as_ref(), session_id.clone(), "stream.request");

    let tool_defs = filtered_tool_definitions(
        rocode_session::resolve_tools(state.tool_registry.as_ref()).await,
        resolved_agent.as_ref(),
    );

    let (tx, rx) = mpsc::channel::<std::result::Result<Event, Infallible>>(128);
    let stream_state = state.clone();
    let stream_session_id = session_id.clone();
    let stream_config = config.clone();
    let stream_session = stream_session.clone();
    let stream_content = req.content.clone();
    let stream_variant = selected_variant.clone();
    let stream_provider = provider.clone();
    let stream_provider_id = provider_id.clone();
    let stream_model_id = model_id.clone();
    let stream_agent = resolved_agent.clone();
    let stream_system_prompt = agent_system_prompt.clone();

    tokio::spawn(async move {
        let stream_tx = tx;
        let (update_tx, mut update_rx) = tokio::sync::mpsc::unbounded_channel::<Session>();
        let update_state = stream_state.clone();
        let update_sse_tx = stream_tx.clone();
        let mut update_task = tokio::spawn(async move {
            let mut emitter = StreamSnapshotEmitter::default();
            while let Some(snapshot) = update_rx.recv().await {
                let snapshot_id = snapshot.id.clone();
                {
                    let mut sessions = update_state.sessions.lock().await;
                    sessions.update(snapshot.clone());
                }
                broadcast_session_updated(update_state.as_ref(), snapshot_id, "stream.prompt");
                emitter.emit_snapshot(&update_sse_tx, &snapshot).await;
            }
        });

        let update_hook_tx = update_tx.clone();
        let update_hook: rocode_session::SessionUpdateHook = Arc::new(move |snapshot| {
            let _ = update_hook_tx.send(snapshot.clone());
        });

        let prompt_runner = rocode_session::SessionPrompt::new(Arc::new(RwLock::new(
            rocode_session::SessionStateManager::new(),
        )))
        .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(&stream_config));
        let mut session = stream_session;
        let input = rocode_session::PromptInput {
            session_id: stream_session_id.clone(),
            message_id: None,
            model: Some(rocode_session::prompt::ModelRef {
                provider_id: stream_provider_id.clone(),
                model_id: stream_model_id.clone(),
            }),
            agent: stream_agent.as_ref().map(|agent| agent.name.clone()),
            no_reply: false,
            system: None,
            variant: stream_variant.clone(),
            parts: vec![rocode_session::PartInput::Text {
                text: stream_content.clone(),
            }],
            tools: None,
        };

        let agent_registry = AgentRegistry::from_config(&stream_config);
        let agent_lookup: Option<rocode_session::prompt::AgentLookup> =
            Some(Arc::new(move |name: &str| {
                agent_registry.get(name).map(to_task_agent_info)
            }));

        let ask_question_hook: Option<rocode_session::prompt::AskQuestionHook> = {
            let state = stream_state.clone();
            let sse_tx = stream_tx.clone();
            Some(Arc::new(move |session_id, questions| {
                let state = state.clone();
                let sse_tx = sse_tx.clone();
                let event_hook: QuestionEventHook = Arc::new(move |payload| {
                    let sse_tx = sse_tx.clone();
                    let event_name = payload
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("question.event")
                        .to_string();
                    tokio::spawn(async move {
                        if let Ok(event) = Event::default().event(&event_name).json_data(payload) {
                            let _ = sse_tx.send(Ok(event)).await;
                        }
                    });
                });
                Box::pin(async move {
                    request_question_answers_with_hook(
                        state,
                        session_id,
                        questions,
                        Some(event_hook),
                    )
                    .await
                })
            }))
        };

        let event_broadcast: Option<rocode_session::prompt::EventBroadcastHook> = {
            let state = stream_state.clone();
            Some(Arc::new(move |event| {
                if let Ok(server_event) = serde_json::from_value::<ServerEvent>(event) {
                    if let Some(payload) = server_event.to_json_string() {
                        state.broadcast(&payload);
                    } else {
                        tracing::warn!("failed to serialize ServerEvent from stream event_broadcast");
                    }
                } else {
                    tracing::warn!("ignored non-ServerEvent payload in stream event_broadcast");
                }
            }))
        };

        if let Err(error) = prompt_runner
            .prompt_with_update_hook(
                input,
                &mut session,
                rocode_session::prompt::PromptRequestContext {
                    provider: stream_provider,
                    system_prompt: stream_system_prompt.clone(),
                    tools: tool_defs,
                    compiled_request: compiled_request.clone(),
                    hooks: rocode_session::prompt::PromptHooks {
                        update_hook: Some(update_hook),
                        event_broadcast,
                        agent_lookup,
                        ask_question_hook,
                        publish_bus_hook: None,
                    },
                },
            )
            .await
        {
            tracing::warn!(
                session_id = %stream_session_id,
                provider_id = %stream_provider_id,
                model_id = %stream_model_id,
                %error,
                "web stream prompt failed"
            );
            let assistant = session.add_assistant_message();
            assistant.finish = Some("error".to_string());
            assistant
                .metadata
                .insert("error".to_string(), serde_json::json!(error.to_string()));
            assistant
                .metadata
                .insert("finish_reason".to_string(), serde_json::json!("error"));
            assistant.metadata.insert(
                "model_provider".to_string(),
                serde_json::json!(&stream_provider_id),
            );
            assistant
                .metadata
                .insert("model_id".to_string(), serde_json::json!(&stream_model_id));
            if let Some(agent) = stream_agent.as_ref().map(|agent| agent.name.as_str()) {
                assistant
                    .metadata
                    .insert("agent".to_string(), serde_json::json!(agent));
            }
            assistant.add_text(format!("Provider error: {}", error));
            let _ = update_tx.send(session.clone());
            let message_id = session
                .messages
                .iter()
                .rev()
                .find(|message| matches!(message.role, SessionMessageRole::Assistant))
                .map(|message| message.id.clone());
            send_stream_error_event(
                &stream_tx,
                Some(stream_session_id.clone()),
                message_id,
                Some(true),
                error.to_string(),
            )
            .await;
        }

        drop(update_tx);
        match tokio::time::timeout(std::time::Duration::from_secs(1), &mut update_task).await {
            Ok(joined) => {
                let _ = joined;
            }
            Err(_) => {
                update_task.abort();
                tracing::warn!(
                    session_id = %stream_session_id,
                    "timed out waiting for web stream update task shutdown; aborted task"
                );
            }
        }

        {
            let mut sessions = stream_state.sessions.lock().await;
            sessions.update(session);
        }
        broadcast_session_updated(stream_state.as_ref(), stream_session_id.clone(), "stream.final");

        if let Err(err) = stream_state
            .flush_session_to_storage(&stream_session_id)
            .await
        {
            tracing::error!(
                session_id = %stream_session_id,
                %err,
                "failed to flush session to storage after stream"
            );
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

#[cfg(test)]
mod tests {
    use super::scheduler_stage_block_from_message;
    use chrono::Utc;
    use rocode_command::governance_fixtures::canonical_scheduler_stage_fixture;
    use rocode_session::{MessagePart, MessageRole, PartType, SessionMessage};

    #[test]
    fn scheduler_stage_message_projects_canonical_governance_block() {
        let fixture = canonical_scheduler_stage_fixture();
        let message = SessionMessage {
            id: "msg-stage".to_string(),
            session_id: "session-1".to_string(),
            role: MessageRole::Assistant,
            parts: vec![MessagePart {
                id: "part-text".to_string(),
                part_type: PartType::Text {
                    text: fixture.message_text.clone(),
                    synthetic: None,
                    ignored: None,
                },
                created_at: Utc::now(),
                message_id: Some("msg-stage".to_string()),
            }],
            created_at: Utc::now(),
            metadata: fixture.metadata,
            usage: None,
            finish: None,
        };

        let block = scheduler_stage_block_from_message(&message).expect("scheduler stage block");
        assert_eq!(block, fixture.block);
    }

    #[test]
    fn scheduler_stage_message_falls_back_to_metadata_title_when_heading_missing() {
        let fixture = canonical_scheduler_stage_fixture();
        let message = SessionMessage {
            id: "msg-stage".to_string(),
            session_id: "session-1".to_string(),
            role: MessageRole::Assistant,
            parts: vec![MessagePart {
                id: "part-text".to_string(),
                part_type: PartType::Text {
                    text: fixture.block.text.clone(),
                    synthetic: None,
                    ignored: None,
                },
                created_at: Utc::now(),
                message_id: Some("msg-stage".to_string()),
            }],
            created_at: Utc::now(),
            metadata: fixture.metadata,
            usage: None,
            finish: None,
        };

        let block = scheduler_stage_block_from_message(&message).expect("scheduler stage block");
        assert_eq!(block.title, "Atlas · Coordination Gate");
        assert_eq!(
            block.text,
            "Decision pending on the unresolved task ledger."
        );
    }
}
