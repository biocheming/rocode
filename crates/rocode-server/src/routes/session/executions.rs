use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;

use rocode_core::agent_task_registry::{global_task_registry, AgentTask, AgentTaskStatus};
use rocode_session::{PartType, Session, ToolCallStatus};
use serde::{Deserialize, Serialize};

use crate::runtime_control::SessionExecutionTopology;
use crate::{ApiError, Result, ServerState};

use super::cancel::ensure_session_exists;

#[derive(Debug, Serialize)]
pub(super) struct AllExecutionsResponse {
    active_count: usize,
    active_session_ids: Vec<String>,
    executions: Vec<crate::runtime_control::ExecutionRecord>,
}

#[derive(Debug, Serialize)]
pub(super) struct CancelExecutionResponse {
    cancelled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<crate::runtime_control::ExecutionKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolExecutionMetadata<'a> {
    tool_call_id: &'a str,
    tool_name: &'a str,
    input: &'a serde_json::Value,
    message_id: &'a str,
    status: &'a str,
}

#[derive(Debug, Serialize)]
struct AgentTaskExecutionMetadata {
    task_id: String,
    agent_name: String,
    prompt: String,
    max_steps: Option<u32>,
    step: Option<u32>,
    output_tail: Vec<String>,
}

pub(super) async fn get_session_executions(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionExecutionTopology>> {
    ensure_session_exists(&state, &session_id).await?;
    if !state
        .ensure_session_hydrated(&session_id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(session_id));
    }
    let session = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?
    };
    let mut records = state
        .runtime_control
        .list_session_execution_records(&session_id)
        .await;
    let tool_records = collect_active_tool_execution_records(&session, &records);
    records.extend(tool_records);
    records.extend(collect_active_agent_task_execution_records(
        &session_id,
        &records,
    ));
    Ok(Json(
        crate::runtime_control::build_session_execution_topology(session_id, records),
    ))
}

/// Global enumeration: list all active execution records across all sessions.
pub(super) async fn list_all_executions(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<AllExecutionsResponse>> {
    let records = state.runtime_control.list_all_executions().await;
    let session_ids = state.runtime_control.list_active_session_ids().await;
    Ok(Json(AllExecutionsResponse {
        active_count: records.len(),
        active_session_ids: session_ids,
        executions: records,
    }))
}

pub(super) async fn cancel_session_execution(
    State(state): State<Arc<ServerState>>,
    Path((_session_id, execution_id)): Path<(String, String)>,
) -> Result<Json<CancelExecutionResponse>> {
    let result = state.runtime_control.cancel_execution(&execution_id).await;
    match result {
        Some(kind) => {
            // For AgentTask, also cancel via the global task registry.
            if matches!(kind, crate::runtime_control::ExecutionKind::AgentTask) {
                if let Some(task_id) = execution_id.strip_prefix("agent_task:") {
                    let _ = global_task_registry().cancel(task_id);
                }
            }
            Ok(Json(CancelExecutionResponse {
                cancelled: true,
                kind: Some(kind),
                error: None,
            }))
        }
        None => Ok(Json(CancelExecutionResponse {
            cancelled: false,
            kind: None,
            error: Some("execution not found".to_string()),
        })),
    }
}

pub(super) fn collect_active_tool_execution_records(
    session: &Session,
    existing_records: &[crate::runtime_control::ExecutionRecord],
) -> Vec<crate::runtime_control::ExecutionRecord> {
    let parent_id = select_active_tool_parent_id(existing_records);
    // Resolve stage_id from the parent record.
    let stage_id = parent_id.as_ref().and_then(|pid| {
        existing_records
            .iter()
            .find(|r| r.id == *pid)
            .and_then(|r| r.stage_id.clone())
    });

    // Build a set of tool_call IDs already present in the registry to avoid
    // double-counting when the lifecycle hook has already registered them.
    let registered_ids: std::collections::HashSet<&str> = existing_records
        .iter()
        .filter(|r| matches!(r.kind, crate::runtime_control::ExecutionKind::ToolCall))
        .map(|r| r.id.as_str())
        .collect();

    let mut records = Vec::new();

    for message in &session.messages {
        for part in &message.parts {
            let PartType::ToolCall {
                id,
                name,
                input,
                status,
                ..
            } = &part.part_type
            else {
                continue;
            };

            if !matches!(status, ToolCallStatus::Pending | ToolCallStatus::Running) {
                continue;
            }

            // Skip if this tool call is already registered via the lifecycle hook.
            let candidate_id = format!("tool_call:{id}");
            if registered_ids.contains(candidate_id.as_str()) {
                continue;
            }

            let execution_status = match status {
                ToolCallStatus::Pending => crate::runtime_control::ExecutionStatus::Waiting,
                ToolCallStatus::Running => crate::runtime_control::ExecutionStatus::Running,
                ToolCallStatus::Completed | ToolCallStatus::Error => continue,
            };

            let metadata = ToolExecutionMetadata {
                tool_call_id: id,
                tool_name: name,
                input,
                message_id: &message.id,
                status: match status {
                    ToolCallStatus::Pending => "pending",
                    ToolCallStatus::Running => "running",
                    ToolCallStatus::Completed => "completed",
                    ToolCallStatus::Error => "error",
                },
            };

            records.push(crate::runtime_control::ExecutionRecord {
                id: format!("tool_call:{id}"),
                session_id: session.id.clone(),
                kind: crate::runtime_control::ExecutionKind::ToolCall,
                status: execution_status,
                label: Some(format!("Tool: {name}")),
                parent_id: parent_id.clone(),
                stage_id: stage_id.clone(),
                waiting_on: Some(match status {
                    ToolCallStatus::Pending => "dispatch".to_string(),
                    ToolCallStatus::Running => "tool".to_string(),
                    ToolCallStatus::Completed | ToolCallStatus::Error => unreachable!(),
                }),
                recent_event: Some(match status {
                    ToolCallStatus::Pending => format!("{name} queued"),
                    ToolCallStatus::Running => format!("{name} running"),
                    ToolCallStatus::Completed | ToolCallStatus::Error => unreachable!(),
                }),
                started_at: part.created_at.timestamp_millis(),
                updated_at: part.created_at.timestamp_millis(),
                metadata: serde_json::to_value(metadata).ok(),
            });
        }
    }

    records
}

pub(super) fn collect_active_agent_task_execution_records(
    session_id: &str,
    existing_records: &[crate::runtime_control::ExecutionRecord],
) -> Vec<crate::runtime_control::ExecutionRecord> {
    let parent_id = select_active_agent_task_parent_id(existing_records);
    let stage_id = parent_id.as_ref().and_then(|pid| {
        existing_records
            .iter()
            .find(|r| r.id == *pid)
            .and_then(|r| r.stage_id.clone())
    });
    global_task_registry()
        .list()
        .into_iter()
        .filter(|task| task.session_id.as_deref() == Some(session_id))
        .filter(|task| !task.status.is_terminal())
        .map(|task| {
            agent_task_execution_record(task, session_id, parent_id.clone(), stage_id.clone())
        })
        .collect()
}

fn agent_task_execution_record(
    task: AgentTask,
    session_id: &str,
    parent_id: Option<String>,
    stage_id: Option<String>,
) -> crate::runtime_control::ExecutionRecord {
    let (status, waiting_on, recent_event, step) = match &task.status {
        AgentTaskStatus::Pending => (
            crate::runtime_control::ExecutionStatus::Waiting,
            Some("agent".to_string()),
            Some("Agent task queued".to_string()),
            None,
        ),
        AgentTaskStatus::Running { step } => (
            crate::runtime_control::ExecutionStatus::Running,
            Some("agent".to_string()),
            Some(match task.max_steps {
                Some(max_steps) => format!("Step {} / {}", step, max_steps),
                None => format!("Step {}", step),
            }),
            Some(*step),
        ),
        AgentTaskStatus::Completed { .. }
        | AgentTaskStatus::Cancelled
        | AgentTaskStatus::Failed { .. } => (
            crate::runtime_control::ExecutionStatus::Running,
            None,
            None,
            None,
        ),
    };

    let metadata = AgentTaskExecutionMetadata {
        task_id: task.id.clone(),
        agent_name: task.agent_name.clone(),
        prompt: task.prompt.clone(),
        max_steps: task.max_steps,
        step,
        output_tail: task.output_tail.iter().cloned().collect(),
    };

    crate::runtime_control::ExecutionRecord {
        id: format!("agent_task:{}", task.id),
        session_id: session_id.to_string(),
        kind: crate::runtime_control::ExecutionKind::AgentTask,
        status,
        label: Some(format!("Agent task: {}", task.agent_name)),
        parent_id,
        stage_id,
        waiting_on,
        recent_event,
        started_at: task.started_at.saturating_mul(1000),
        updated_at: chrono::Utc::now().timestamp_millis(),
        metadata: serde_json::to_value(metadata).ok(),
    }
}

fn select_active_tool_parent_id(
    records: &[crate::runtime_control::ExecutionRecord],
) -> Option<String> {
    select_preferred_execution_parent_id(records)
}

fn select_active_agent_task_parent_id(
    records: &[crate::runtime_control::ExecutionRecord],
) -> Option<String> {
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
    struct ToolCallMetadataWire {
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        tool_name: Option<String>,
    }

    fn tool_call_name(metadata: Option<&serde_json::Value>) -> Option<String> {
        let Some(metadata) = metadata else {
            return None;
        };
        serde_json::from_value::<ToolCallMetadataWire>(metadata.clone())
            .ok()
            .and_then(|wire| wire.tool_name)
    }

    records
        .iter()
        .filter(|record| matches!(record.kind, crate::runtime_control::ExecutionKind::ToolCall))
        .filter(|record| {
            tool_call_name(record.metadata.as_ref())
                .as_deref()
                .map(|name| matches!(name, "task" | "task_flow"))
                .unwrap_or(false)
        })
        .max_by_key(|record| record.updated_at)
        .map(|record| record.id.clone())
        .or_else(|| select_preferred_execution_parent_id(records))
}

fn select_preferred_execution_parent_id(
    records: &[crate::runtime_control::ExecutionRecord],
) -> Option<String> {
    records
        .iter()
        .filter(|record| {
            matches!(
                record.kind,
                crate::runtime_control::ExecutionKind::PromptRun
                    | crate::runtime_control::ExecutionKind::SchedulerRun
                    | crate::runtime_control::ExecutionKind::SchedulerStage
            )
        })
        .max_by_key(|record| {
            (
                execution_parent_rank(&record.kind),
                record.updated_at,
                record.started_at,
            )
        })
        .map(|record| record.id.clone())
}

fn execution_parent_rank(kind: &crate::runtime_control::ExecutionKind) -> u8 {
    match kind {
        crate::runtime_control::ExecutionKind::PromptRun => 0,
        crate::runtime_control::ExecutionKind::SchedulerRun => 1,
        crate::runtime_control::ExecutionKind::SchedulerStage => 2,
        crate::runtime_control::ExecutionKind::ToolCall
        | crate::runtime_control::ExecutionKind::AgentTask
        | crate::runtime_control::ExecutionKind::Question => 0,
    }
}
