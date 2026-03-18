use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::session_runtime::{assistant_visible_text, decision_from_stage_text};
use crate::{ApiError, Result, ServerState};
use rocode_command::agent_presenter::{
    history_session_event_to_web, history_tool_call_to_web, history_tool_result_to_web,
};

use super::session_crud::persist_sessions_if_enabled;

#[derive(Debug, Deserialize)]
pub(crate) struct SendMessageRequest {
    pub content: String,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub variant: Option<String>,
    #[allow(dead_code)]
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct MessageInfo {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub parts: Vec<PartInfo>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub finish: Option<String>,
    pub error: Option<String>,
    pub cost: f64,
    pub tokens: MessageTokensInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Default)]
pub(super) struct MessageTokensInfo {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct PartInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
    pub file: Option<MessageFileInfo>,
    pub tool_call: Option<ToolCallInfo>,
    pub tool_result: Option<ToolResultInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_block: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct MessageFileInfo {
    pub url: String,
    pub filename: String,
    pub mime: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolResultInfo {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<serde_json::Value>>,
}

pub(super) fn message_role_name(role: &rocode_session::MessageRole) -> &'static str {
    match role {
        rocode_session::MessageRole::User => "user",
        rocode_session::MessageRole::Assistant => "assistant",
        rocode_session::MessageRole::System => "system",
        rocode_session::MessageRole::Tool => "tool",
    }
}

fn part_type_name(part_type: &rocode_session::PartType) -> &'static str {
    match part_type {
        rocode_session::PartType::Text { .. } => "text",
        rocode_session::PartType::ToolCall { .. } => "tool_call",
        rocode_session::PartType::ToolResult { .. } => "tool_result",
        rocode_session::PartType::Reasoning { .. } => "reasoning",
        rocode_session::PartType::File { .. } => "file",
        rocode_session::PartType::StepStart { .. } => "step_start",
        rocode_session::PartType::StepFinish { .. } => "step_finish",
        rocode_session::PartType::Snapshot { .. } => "snapshot",
        rocode_session::PartType::Patch { .. } => "patch",
        rocode_session::PartType::Agent { .. } => "agent",
        rocode_session::PartType::Subtask { .. } => "subtask",
        rocode_session::PartType::Retry { .. } => "retry",
        rocode_session::PartType::Compaction { .. } => "compaction",
    }
}

fn part_to_info(
    part: &rocode_session::MessagePart,
    tool_names: &HashMap<String, String>,
    pending_questions: &mut Vec<super::super::tui::QuestionInfo>,
) -> PartInfo {
    let (synthetic, ignored) = match &part.part_type {
        rocode_session::PartType::Text {
            synthetic, ignored, ..
        } => (*synthetic, *ignored),
        _ => (None, None),
    };
    let tool_call = if let rocode_session::PartType::ToolCall {
        id,
        name,
        input,
        status,
        raw,
        state,
        ..
    } = &part.part_type
    {
        Some(ToolCallInfo {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
            status: Some(
                match status {
                    rocode_session::ToolCallStatus::Pending => "pending",
                    rocode_session::ToolCallStatus::Running => "running",
                    rocode_session::ToolCallStatus::Completed => "completed",
                    rocode_session::ToolCallStatus::Error => "error",
                }
                .to_string(),
            ),
            raw: raw.clone(),
            state: state.as_ref().and_then(|s| serde_json::to_value(s).ok()),
        })
    } else {
        None
    };
    let tool_result = if let rocode_session::PartType::ToolResult {
        tool_call_id,
        content,
        is_error,
        title,
        metadata,
        attachments,
    } = &part.part_type
    {
        Some(ToolResultInfo {
            tool_call_id: tool_call_id.clone(),
            content: content.clone(),
            is_error: *is_error,
            title: title.clone(),
            metadata: metadata.clone(),
            attachments: attachments.clone(),
        })
    } else {
        None
    };
    let mut output_block = if let Some(tool_call) = tool_call.as_ref() {
        Some(history_tool_call_to_web(
            &tool_call.id,
            &tool_call.name,
            &tool_call.input,
            tool_call.status.as_deref(),
            tool_call.raw.as_deref(),
        ))
    } else if let Some(tool_result) = tool_result.as_ref() {
        let tool_name = tool_names
            .get(&tool_result.tool_call_id)
            .cloned()
            .unwrap_or_else(|| tool_result.tool_call_id.clone());
        let empty_meta = HashMap::new();
        Some(history_tool_result_to_web(
            &tool_result.tool_call_id,
            &tool_name,
            tool_result.title.as_deref(),
            &tool_result.content,
            tool_result.is_error,
            tool_result.metadata.as_ref().unwrap_or(&empty_meta),
        ))
    } else if let rocode_session::PartType::Agent { name, status } = &part.part_type {
        Some(history_session_event_to_web(
            "agent",
            format!("Agent · {name}"),
            Some(status.as_str()),
            Some(format!("Agent `{name}` entered `{status}` state.")),
            vec![("Agent".to_string(), name.clone(), None)],
            None,
        ))
    } else if let rocode_session::PartType::Subtask {
        id,
        description,
        status,
    } = &part.part_type
    {
        Some(history_session_event_to_web(
            "subtask",
            if description.trim().is_empty() {
                "Subtask".to_string()
            } else {
                format!("Subtask · {description}")
            },
            Some(status.as_str()),
            Some(format!("Subtask `{id}` is `{status}`.")),
            vec![
                ("ID".to_string(), id.clone(), None),
                (
                    "Description".to_string(),
                    if description.trim().is_empty() {
                        "—".to_string()
                    } else {
                        description.clone()
                    },
                    None,
                ),
            ],
            None,
        ))
    } else if let rocode_session::PartType::Retry { count, reason } = &part.part_type {
        Some(history_session_event_to_web(
            "retry",
            "Retry",
            Some("running"),
            Some(format!("Retry attempt {}", count)),
            vec![(
                "Attempt".to_string(),
                count.to_string(),
                Some("status".to_string()),
            )],
            Some(reason.clone()),
        ))
    } else if let rocode_session::PartType::StepStart { id, name } = &part.part_type {
        Some(history_session_event_to_web(
            "step",
            format!("Step · {name}"),
            Some("running"),
            Some("Step started".to_string()),
            vec![("ID".to_string(), id.clone(), None)],
            None,
        ))
    } else if let rocode_session::PartType::StepFinish { id, output } = &part.part_type {
        Some(history_session_event_to_web(
            "step",
            "Step complete",
            Some("completed"),
            Some("Step finished".to_string()),
            vec![("ID".to_string(), id.clone(), None)],
            output.clone(),
        ))
    } else {
        None
    };
    if let Some(serde_json::Value::Object(map)) = output_block.as_mut() {
        map.insert(
            "ts".to_string(),
            serde_json::Value::Number(part.created_at.timestamp_millis().into()),
        );
        if let Some(tool_call) = tool_call.as_ref() {
            if tool_call.name.eq_ignore_ascii_case("question") {
                if let Some(question_info) =
                    match_pending_question_request(&tool_call.input, pending_questions)
                {
                    map.insert(
                        "interaction".to_string(),
                        question_pending_interaction_json(question_info, &tool_call.input),
                    );
                }
            }
        }
    }
    PartInfo {
        id: part.id.clone(),
        part_type: part_type_name(&part.part_type).to_string(),
        text: match &part.part_type {
            rocode_session::PartType::Text { text, .. } => {
                Some(rocode_session::sanitize_display_text(text))
            }
            rocode_session::PartType::Reasoning { text } => Some(text.clone()),
            rocode_session::PartType::Compaction { summary } => {
                Some(rocode_session::sanitize_display_text(summary))
            }
            _ => None,
        },
        file: if let rocode_session::PartType::File {
            url,
            filename,
            mime,
        } = &part.part_type
        {
            Some(MessageFileInfo {
                url: url.clone(),
                filename: filename.clone(),
                mime: mime.clone(),
            })
        } else {
            None
        },
        tool_call,
        tool_result,
        output_block,
        synthetic,
        ignored,
    }
}

fn message_to_info(
    session_id: &str,
    message: &rocode_session::SessionMessage,
    tool_names: &HashMap<String, String>,
    pending_questions: &mut Vec<super::super::tui::QuestionInfo>,
) -> MessageInfo {
    fn deserialize_opt_string_lossy<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match value {
            Some(serde_json::Value::String(value)) => Some(value),
            _ => None,
        })
    }

    fn deserialize_opt_i64_lossy<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<i64>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match value {
            Some(serde_json::Value::Number(value)) => value.as_i64(),
            Some(serde_json::Value::String(value)) => value.parse::<i64>().ok(),
            _ => None,
        })
    }

    fn deserialize_opt_f64_lossy<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<f64>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match value {
            Some(serde_json::Value::Number(value)) => value.as_f64(),
            Some(serde_json::Value::String(value)) => value.parse::<f64>().ok(),
            _ => None,
        })
    }

    #[derive(Debug, Default, Deserialize)]
    struct MessageMetadataWire {
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        model_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        model_provider: Option<String>,
        #[serde(default, deserialize_with = "deserialize_opt_f64_lossy")]
        cost: Option<f64>,
        #[serde(default, deserialize_with = "deserialize_opt_i64_lossy")]
        completed_at: Option<i64>,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        agent: Option<String>,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        mode: Option<String>,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        finish_reason: Option<String>,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        error: Option<String>,
    }

    fn message_metadata_wire(metadata: &HashMap<String, serde_json::Value>) -> MessageMetadataWire {
        let Ok(value) = serde_json::to_value(metadata) else {
            return MessageMetadataWire::default();
        };
        serde_json::from_value::<MessageMetadataWire>(value).unwrap_or_default()
    }

    let wire = message_metadata_wire(&message.metadata);
    let mut metadata = message.metadata.clone();
    augment_scheduler_decision_metadata_for_response(&mut metadata, message);
    let usage = message.usage.clone().unwrap_or_default();
    let model_id = wire.model_id.clone();
    let model_provider = wire.model_provider.clone();
    let model = match (model_provider.as_deref(), model_id.as_deref()) {
        (Some(provider), Some(model)) => Some(format!("{}/{}", provider, model)),
        (None, Some(model)) => Some(model.to_string()),
        _ => None,
    };
    let cost = if usage.total_cost > 0.0 {
        usage.total_cost
    } else {
        wire.cost.unwrap_or(0.0)
    };

    MessageInfo {
        id: message.id.clone(),
        session_id: session_id.to_string(),
        role: message_role_name(&message.role).to_string(),
        parts: message
            .parts
            .iter()
            .map(|part| part_to_info(part, tool_names, pending_questions))
            .collect(),
        created_at: message.created_at.timestamp_millis(),
        completed_at: wire.completed_at,
        agent: wire.agent.clone(),
        model,
        mode: wire.mode.clone(),
        finish: message.finish.clone().or(wire.finish_reason.clone()),
        error: wire.error.clone(),
        cost,
        tokens: MessageTokensInfo {
            input: usage.input_tokens,
            output: usage.output_tokens,
            reasoning: usage.reasoning_tokens,
            cache_read: usage.cache_read_tokens,
            cache_write: usage.cache_write_tokens,
        },
        metadata: (!metadata.is_empty()).then_some(metadata),
    }
}

fn collect_tool_names(session: &rocode_session::Session) -> HashMap<String, String> {
    let mut tool_names = HashMap::new();
    for message in &session.messages {
        for part in &message.parts {
            if let rocode_session::PartType::ToolCall { id, name, .. } = &part.part_type {
                if !name.trim().is_empty() {
                    tool_names.insert(id.clone(), name.clone());
                }
            }
        }
    }
    tool_names
}

fn augment_scheduler_decision_metadata_for_response(
    metadata: &mut HashMap<String, serde_json::Value>,
    message: &rocode_session::SessionMessage,
) {
    if metadata.contains_key("scheduler_decision_title") {
        return;
    }
    fn deserialize_opt_string_lossy<'de, D>(
        deserializer: D,
    ) -> std::result::Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        Ok(match value {
            Some(serde_json::Value::String(value)) => Some(value),
            _ => None,
        })
    }

    #[derive(Debug, Default, Deserialize)]
    struct SchedulerStageWire {
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        scheduler_stage: Option<String>,
    }

    let stage = serde_json::to_value(&*metadata)
        .ok()
        .and_then(|value| serde_json::from_value::<SchedulerStageWire>(value).ok())
        .and_then(|wire| wire.scheduler_stage);
    let Some(stage) = stage.as_deref() else {
        return;
    };
    let text = assistant_visible_text(message);
    let Some(decision) = decision_from_stage_text(stage, &text) else {
        return;
    };

    metadata.insert(
        "scheduler_decision_kind".to_string(),
        serde_json::json!(decision.kind),
    );
    metadata.insert(
        "scheduler_decision_title".to_string(),
        serde_json::json!(decision.title),
    );
    metadata.insert(
        "scheduler_decision_spec".to_string(),
        serde_json::json!({
            "version": decision.spec.version,
            "show_header_divider": decision.spec.show_header_divider,
            "field_order": decision.spec.field_order,
            "field_label_emphasis": decision.spec.field_label_emphasis,
            "status_palette": decision.spec.status_palette,
            "section_spacing": decision.spec.section_spacing,
            "update_policy": decision.spec.update_policy,
        }),
    );
    metadata.insert(
        "scheduler_decision_fields".to_string(),
        serde_json::Value::Array(
            decision
                .fields
                .iter()
                .map(|field| {
                    serde_json::json!({
                        "label": field.label,
                        "value": field.value,
                        "tone": field.tone,
                    })
                })
                .collect(),
        ),
    );
    metadata.insert(
        "scheduler_decision_sections".to_string(),
        serde_json::Value::Array(
            decision
                .sections
                .iter()
                .map(|section| {
                    serde_json::json!({
                        "title": section.title,
                        "body": section.body,
                    })
                })
                .collect(),
        ),
    );
}

pub(super) async fn resolve_provider_and_model(
    state: &ServerState,
    request_model: Option<&str>,
    config_model: Option<&str>,
    config_provider: Option<&str>,
) -> Result<(Arc<dyn rocode_provider::Provider>, String, String)> {
    let providers = state.providers.read().await;
    let resolve_from_model = |model: &str| -> Result<(String, String)> {
        providers
            .parse_model_string(model)
            .ok_or_else(|| ApiError::BadRequest(format!("Model not found: {}", model)))
    };

    let (provider_id, model_id) = if let Some(model) = request_model {
        resolve_from_model(model)?
    } else if let Some(model) = config_model {
        if let Some(provider_id) = config_provider {
            (provider_id.to_string(), model.to_string())
        } else {
            resolve_from_model(model)?
        }
    } else {
        let first = providers
            .list_models()
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::BadRequest("No providers configured".to_string()))?;
        (first.provider, first.id)
    };

    let provider = providers
        .get_provider(&provider_id)
        .map_err(|e| ApiError::ProviderError(e.to_string()))?;
    if provider.get_model(&model_id).is_none() {
        return Err(ApiError::BadRequest(format!(
            "Model `{}` not found for provider `{}`",
            model_id, provider_id
        )));
    }

    Ok((provider, provider_id, model_id))
}

pub(super) async fn send_message(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<MessageInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    session.add_user_message(&req.content);
    if let Some(variant) = req.variant.as_deref() {
        session
            .metadata
            .insert("model_variant".to_string(), serde_json::json!(variant));
    }
    let tool_names = collect_tool_names(session);
    let assistant_msg = session.add_assistant_message();
    let mut pending_questions = Vec::new();
    let info = message_to_info(
        &session_id,
        assistant_msg,
        &tool_names,
        &mut pending_questions,
    );
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn list_messages(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageInfo>>> {
    state
        .api_perf
        .list_messages_calls
        .fetch_add(1, Ordering::Relaxed);
    if query.after.is_some() {
        state
            .api_perf
            .list_messages_incremental_calls
            .fetch_add(1, Ordering::Relaxed);
    } else {
        state
            .api_perf
            .list_messages_full_calls
            .fetch_add(1, Ordering::Relaxed);
    }

    let mut pending_questions =
        super::super::tui::list_questions_for_session(&state, &session_id).await;
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let tool_names = collect_tool_names(session);
    let limit = query.limit.filter(|value| *value > 0);
    let mut started = query.after.is_none();
    let mut messages = Vec::new();
    for message in &session.messages {
        if !started {
            if query.after.as_deref() == Some(message.id.as_str()) {
                started = true;
            }
            continue;
        }
        messages.push(message_to_info(
            &session_id,
            message,
            &tool_names,
            &mut pending_questions,
        ));
        if let Some(limit) = limit {
            if messages.len() >= limit {
                break;
            }
        }
    }

    // If the anchor message is unknown, fall back to a full list so clients can recover.
    if query.after.is_some() && !started {
        messages.clear();
        for message in &session.messages {
            messages.push(message_to_info(
                &session_id,
                message,
                &tool_names,
                &mut pending_questions,
            ));
            if let Some(limit) = limit {
                if messages.len() >= limit {
                    break;
                }
            }
        }
    }

    Ok(Json(messages))
}

fn match_pending_question_request(
    input: &serde_json::Value,
    pending_questions: &mut Vec<super::super::tui::QuestionInfo>,
) -> Option<super::super::tui::QuestionInfo> {
    #[derive(Debug, Deserialize, Default)]
    struct QuestionToolInput {
        #[serde(default)]
        questions: Vec<QuestionToolQuestion>,
    }

    #[derive(Debug, Deserialize, Default)]
    struct QuestionToolQuestion {
        #[serde(default)]
        question: Option<String>,
    }

    let parsed: QuestionToolInput = serde_json::from_value(input.clone()).ok()?;
    let normalized_input = parsed
        .questions
        .iter()
        .filter_map(|question| question.question.as_deref().map(normalize_question_text))
        .collect::<Vec<_>>();
    if normalized_input.is_empty() {
        return None;
    }
    let index = pending_questions.iter().position(|pending| {
        let normalized_pending = pending
            .questions
            .iter()
            .map(|question| normalize_question_text(question))
            .collect::<Vec<_>>();
        normalized_pending == normalized_input
    })?;
    Some(pending_questions.remove(index))
}

fn normalize_question_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn question_pending_interaction_json(
    question_info: super::super::tui::QuestionInfo,
    input: &serde_json::Value,
) -> serde_json::Value {
    #[derive(Debug, Deserialize, Default)]
    struct QuestionToolInput {
        #[serde(default)]
        questions: Vec<QuestionToolQuestion>,
    }

    #[derive(Debug, Deserialize, Default)]
    struct QuestionToolQuestion {
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        header: Option<String>,
        #[serde(default)]
        multiple: bool,
        #[serde(default)]
        options: Option<Vec<QuestionToolOption>>,
    }

    #[derive(Debug, Deserialize, Default)]
    struct QuestionToolOption {
        #[serde(default)]
        label: Option<String>,
    }

    let parsed: QuestionToolInput = serde_json::from_value(input.clone()).unwrap_or_default();
    let questions = parsed
        .questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let parsed_labels = question.options.as_ref().map(|values| {
                values
                    .iter()
                    .filter_map(|option| option.label.clone())
                    .collect::<Vec<_>>()
            });
            let options = parsed_labels
                .or_else(|| {
                    question_info
                        .options
                        .as_ref()
                        .and_then(|options| options.get(index).cloned())
                })
                .unwrap_or_default();
            serde_json::json!({
                "question": question.question.as_deref().unwrap_or_default(),
                "header": question.header.as_deref(),
                "multiple": question.multiple,
                "options": options,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "type": "question",
        "status": "pending",
        "request_id": question_info.id,
        "can_reply": true,
        "can_reject": true,
        "questions": questions,
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct ListMessagesQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

pub(super) async fn delete_message(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    session.remove_message(&msg_id);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Deserialize)]
pub(super) struct AddPartRequest {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_status: Option<String>,
    pub tool_raw_input: Option<String>,
    pub content: Option<String>,
    pub is_error: Option<bool>,
    pub title: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub attachments: Option<Vec<serde_json::Value>>,
}

fn build_message_part(req: AddPartRequest, msg_id: &str) -> Result<rocode_session::MessagePart> {
    let part_type = match req.part_type.as_str() {
        "text" => rocode_session::PartType::Text {
            text: req.text.ok_or_else(|| {
                ApiError::BadRequest("Field `text` is required for text parts".to_string())
            })?,
            synthetic: None,
            ignored: None,
        },
        "reasoning" => rocode_session::PartType::Reasoning {
            text: req.text.ok_or_else(|| {
                ApiError::BadRequest("Field `text` is required for reasoning parts".to_string())
            })?,
        },
        "tool_call" => rocode_session::PartType::ToolCall {
            id: req.tool_call_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_call_id` is required for tool_call parts".to_string(),
                )
            })?,
            name: req.tool_name.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_name` is required for tool_call parts".to_string(),
                )
            })?,
            input: req.tool_input.unwrap_or_else(|| serde_json::json!({})),
            status: match req
                .tool_status
                .as_deref()
                .unwrap_or("pending")
                .to_ascii_lowercase()
                .as_str()
            {
                "running" => rocode_session::ToolCallStatus::Running,
                "completed" => rocode_session::ToolCallStatus::Completed,
                "error" => rocode_session::ToolCallStatus::Error,
                _ => rocode_session::ToolCallStatus::Pending,
            },
            raw: req.tool_raw_input,
            state: None,
        },
        "tool_result" => rocode_session::PartType::ToolResult {
            tool_call_id: req.tool_call_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_call_id` is required for tool_result parts".to_string(),
                )
            })?,
            content: req.content.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `content` is required for tool_result parts".to_string(),
                )
            })?,
            is_error: req.is_error.unwrap_or(false),
            title: req.title,
            metadata: req.metadata,
            attachments: req.attachments,
        },
        unsupported => {
            return Err(ApiError::BadRequest(format!(
                "Unsupported part type: {}",
                unsupported
            )));
        }
    };

    Ok(rocode_session::MessagePart {
        id: format!("prt_{}", uuid::Uuid::new_v4().simple()),
        part_type,
        created_at: chrono::Utc::now(),
        message_id: Some(msg_id.to_string()),
    })
}

pub(super) async fn add_message_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
    Json(req): Json<AddPartRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let part = build_message_part(req, &msg_id)?;
    let part_id = part.id.clone();
    message.parts.push(part);
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "added": true,
        "session_id": session_id,
        "message_id": msg_id,
        "part_id": part_id,
    })))
}

pub(super) async fn delete_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id, part_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let before = message.parts.len();
    message.parts.retain(|part| part.id != part_id);
    if message.parts.len() == before {
        return Err(ApiError::NotFound(format!("Part not found: {}", part_id)));
    }
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "deleted": true,
        "session_id": session_id,
        "message_id": msg_id,
        "part_id": part_id,
    })))
}
