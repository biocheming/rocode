use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
    Json,
};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use crate::session_runtime::events::{
    broadcast_session_updated, send_sse_server_event, sse_output_block_hook, ServerEvent,
};
use crate::{ApiError, ServerState};
use rocode_agent::{AgentInfo, AgentRegistry};
use rocode_provider::ToolDefinition;
use rocode_session::{MessageRole as SessionMessageRole, MessageUsage, Session};

use super::permission::request_permission;
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

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    })
}

#[derive(Debug, Default, Deserialize)]
struct StreamSessionMetadataWire {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_variant: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamEventTypeWire {
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    event_type: Option<String>,
}

fn stream_session_metadata_wire(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
) -> StreamSessionMetadataWire {
    serde_json::from_value::<StreamSessionMetadataWire>(serde_json::Value::Object(
        metadata.clone().into_iter().collect(),
    ))
    .unwrap_or_default()
}

fn stream_event_name(payload: &serde_json::Value) -> String {
    serde_json::from_value::<StreamEventTypeWire>(payload.clone())
        .unwrap_or_default()
        .event_type
        .unwrap_or_else(|| "question.event".to_string())
}

async fn emit_latest_assistant_usage(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    session_id: &str,
    session: &Session,
) {
    let latest_usage = session
        .messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, SessionMessageRole::Assistant))
        .and_then(|message| {
            message
                .usage
                .as_ref()
                .map(|usage| (message.id.clone(), usage.clone()))
        });

    let Some((message_id, usage)) = latest_usage else {
        return;
    };

    let MessageUsage {
        input_tokens,
        output_tokens,
        ..
    } = usage;

    send_stream_usage_event(
        tx,
        Some(session_id.to_string()),
        Some(message_id),
        input_tokens,
        output_tokens,
    )
    .await;
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
        let metadata_wire = stream_session_metadata_wire(&session.metadata);
        req.variant.clone().or_else(|| {
            metadata_wire
                .model_variant
                .as_ref()
                .map(ToString::to_string)
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
        let mut update_task = tokio::spawn(async move {
            while let Some(snapshot) = update_rx.recv().await {
                {
                    let mut sessions = update_state.sessions.lock().await;
                    sessions.update(snapshot.clone());
                }
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
                    let event_name = stream_event_name(&payload);
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
        let ask_permission_hook: Option<rocode_session::prompt::AskPermissionHook> = {
            let state = stream_state.clone();
            Some(Arc::new(move |session_id, request| {
                let state = state.clone();
                Box::pin(async move { request_permission(state, session_id, request).await })
            }))
        };

        let event_broadcast: Option<rocode_session::prompt::EventBroadcastHook> = {
            let state = stream_state.clone();
            Some(Arc::new(move |event| {
                if let Ok(server_event) = serde_json::from_value::<ServerEvent>(event) {
                    if let Some(payload) = server_event.to_json_string() {
                        state.broadcast(&payload);
                    } else {
                        tracing::warn!(
                            "failed to serialize ServerEvent from stream event_broadcast"
                        );
                    }
                } else {
                    tracing::warn!("ignored non-ServerEvent payload in stream event_broadcast");
                }
            }))
        };
        let output_block_hook: Option<rocode_session::prompt::OutputBlockHook> =
            { Some(sse_output_block_hook(stream_tx.clone())) };

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
                        output_block_hook,
                        agent_lookup,
                        ask_question_hook,
                        ask_permission_hook,
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
            sessions.update(session.clone());
        }
        emit_latest_assistant_usage(&stream_tx, &stream_session_id, &session).await;
        broadcast_session_updated(
            stream_state.as_ref(),
            stream_session_id.clone(),
            "stream.final",
        );

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
    use crate::session_runtime::scheduler_stage_block_from_message;
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
