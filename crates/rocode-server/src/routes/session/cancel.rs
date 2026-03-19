use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};

use crate::session_runtime::request_active_scheduler_stage_abort;
use crate::{ApiError, Result, ServerState};
use rocode_orchestrator::OrchestratorError;
use serde::Serialize;

use super::super::tui::cancel_questions_for_session;

#[derive(Debug, Serialize)]
pub(super) struct CancelSessionResponse {
    aborted: bool,
    target: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scheduler_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage_index: Option<u32>,
}

pub(super) async fn abort_prompt(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    ensure_session_exists(&state, &id).await?;
    let response = abort_session_execution(&state, &id, false).await;
    Ok(Json(response))
}

pub(super) async fn abort_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    ensure_session_exists(&state, &id).await?;
    let response = abort_session_execution(&state, &id, false).await;
    Ok(Json(response))
}

pub(super) async fn abort_scheduler_stage(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    ensure_session_exists(&state, &id).await?;
    let response = abort_session_execution(&state, &id, true).await;
    Ok(Json(response))
}

pub(super) async fn ensure_session_exists(
    state: &Arc<ServerState>,
    session_id: &str,
) -> Result<()> {
    let sessions = state.sessions.lock().await;
    if sessions.get(session_id).is_none() {
        return Err(ApiError::SessionNotFound(session_id.to_string()));
    }
    Ok(())
}

pub(super) async fn abort_session_execution(
    state: &Arc<ServerState>,
    session_id: &str,
    scheduler_stage_only: bool,
) -> serde_json::Value {
    let mut prompt_running = false;
    let scheduler_running = state
        .runtime_control
        .request_scheduler_cancel(session_id)
        .await;

    if !scheduler_stage_only && state.runtime_control.has_prompt_run(session_id).await {
        prompt_running = true;
        state.prompt_runner.cancel(session_id).await;
    }

    let scheduler_abort_info = if scheduler_running {
        let info = request_active_scheduler_stage_abort(state, session_id).await;
        let _ = cancel_questions_for_session(state.clone(), session_id).await;
        info
    } else {
        None
    };

    if prompt_running {
        let _ = cancel_questions_for_session(state.clone(), session_id).await;
    }

    match scheduler_abort_info {
        Some(info) => serde_json::to_value(CancelSessionResponse {
            aborted: true,
            target: Some("stage"),
            scheduler_profile: info.scheduler_profile,
            stage: info.stage_name,
            stage_index: info.stage_index,
        })
        .unwrap_or(serde_json::Value::Null),
        None if prompt_running || scheduler_running => {
            serde_json::to_value(CancelSessionResponse {
                aborted: true,
                target: Some("run"),
                scheduler_profile: None,
                stage: None,
                stage_index: None,
            })
            .unwrap_or(serde_json::Value::Null)
        }
        None => serde_json::to_value(CancelSessionResponse {
            aborted: false,
            target: None,
            scheduler_profile: None,
            stage: None,
            stage_index: None,
        })
        .unwrap_or(serde_json::Value::Null),
    }
}

pub(super) fn is_scheduler_cancellation_error(error: &OrchestratorError) -> bool {
    match error {
        OrchestratorError::Other(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("cancelled") || lower.contains("canceled") || lower.contains("aborted")
        }
        OrchestratorError::ToolError { error, .. } => {
            let lower = error.to_ascii_lowercase();
            lower.contains("cancelled") || lower.contains("canceled") || lower.contains("aborted")
        }
        _ => false,
    }
}
