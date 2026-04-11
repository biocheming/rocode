use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use rocode_command::stage_protocol::StageSummary;
use rocode_session::{
    load_session_telemetry_snapshot, persist_session_telemetry_snapshot,
    session_last_run_status_label, Session, SessionTelemetrySnapshot as PersistedTelemetrySnapshot,
    SessionUsage,
};
use serde::Serialize;

use crate::runtime_control::SessionExecutionTopology;
use crate::session_runtime::state::SessionRuntimeState;
use crate::{Result, ServerState};

use super::cancel::ensure_session_exists;
use super::executions::build_session_execution_topology_snapshot;
use super::session_crud::runtime_snapshot_or_default;

#[derive(Debug, Clone, Serialize)]
pub struct SessionTelemetrySnapshot {
    pub runtime: SessionRuntimeState,
    pub stages: Vec<StageSummary>,
    pub topology: SessionExecutionTopology,
    pub usage: SessionUsage,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInsightsResponse {
    pub id: String,
    pub title: String,
    pub directory: String,
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<PersistedTelemetrySnapshot>,
}

pub(super) async fn get_session_telemetry(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionTelemetrySnapshot>> {
    ensure_session_exists(&state, &session_id).await?;
    let session = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .cloned()
            .expect("session existence checked before telemetry load")
    };

    Ok(Json(
        build_session_telemetry_snapshot(&state, &session_id, &session).await?,
    ))
}

pub(super) async fn get_session_insights(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionInsightsResponse>> {
    let session = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| crate::ApiError::SessionNotFound(session_id.clone()))?
    };

    let session_record = session.record();
    Ok(Json(SessionInsightsResponse {
        id: session_record.id.clone(),
        title: session_record.title.clone(),
        directory: session_record.directory.clone(),
        updated: session_record.time.updated,
        telemetry: load_session_telemetry_snapshot(&session),
    }))
}

pub(super) async fn build_session_telemetry_snapshot(
    state: &Arc<ServerState>,
    session_id: &str,
    session: &Session,
) -> Result<SessionTelemetrySnapshot> {
    let mut runtime = runtime_snapshot_or_default(state, session_id).await?;
    let usage = runtime.usage.clone().unwrap_or_else(|| session.get_usage());
    runtime.usage = Some(usage.clone());

    let stages = state
        .runtime_telemetry
        .list_stage_summaries(session_id)
        .await;
    let topology = build_session_execution_topology_snapshot(state, session_id, session).await;

    Ok(SessionTelemetrySnapshot {
        runtime,
        stages,
        topology,
        usage,
    })
}

pub(super) async fn persist_session_telemetry_metadata(
    state: &Arc<ServerState>,
    session: &mut Session,
) {
    let usage = session.get_usage();
    let last_run_status = session_last_run_status_label(session);
    let session_id = session.record().id.clone();
    let Some(snapshot) = state
        .runtime_telemetry
        .build_persisted_snapshot(&session_id, usage, last_run_status)
        .await
    else {
        return;
    };

    if let Err(error) = persist_session_telemetry_snapshot(session, &snapshot) {
        tracing::warn!(
            session_id = %session.id,
            %error,
            "failed to persist telemetry snapshot into session metadata"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_control::SessionExecutionTopology;
    use crate::session_runtime::state::SessionRuntimeState;
    use crate::ServerState;
    use rocode_command::stage_protocol::{StageStatus, StageSummary};
    use rocode_session::{persist_session_telemetry_snapshot, SessionTelemetrySnapshotVersion};
    use std::sync::Arc;

    #[test]
    fn telemetry_snapshot_syncs_runtime_usage_from_session_when_missing() {
        let mut session = Session::new("session-1".to_string(), ".".to_string());
        let assistant = session.add_assistant_message();
        assistant.usage = Some(rocode_session::MessageUsage {
            input_tokens: 12,
            output_tokens: 8,
            reasoning_tokens: 3,
            cache_write_tokens: 2,
            cache_read_tokens: 1,
            total_cost: 0.42,
        });

        let mut runtime = SessionRuntimeState::new("session-1");
        let usage = runtime.usage.clone().unwrap_or_else(|| session.get_usage());
        runtime.usage = Some(usage.clone());

        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 8);
        assert_eq!(runtime.usage.as_ref().map(|v| v.total_cost), Some(0.42));
    }

    #[test]
    fn telemetry_snapshot_serializes_authority_contract_fields() {
        let mut runtime = SessionRuntimeState::new("session-1");
        runtime.active_stage_id = Some("stage-1".to_string());
        runtime.active_stage_count = 1;
        runtime.usage = Some(SessionUsage {
            input_tokens: 10,
            output_tokens: 20,
            reasoning_tokens: 3,
            cache_write_tokens: 4,
            cache_read_tokens: 5,
            total_cost: 0.12,
        });

        let snapshot = SessionTelemetrySnapshot {
            runtime,
            stages: vec![StageSummary {
                stage_id: "stage-1".to_string(),
                stage_name: "Plan".to_string(),
                index: Some(1),
                total: Some(2),
                step: Some(1),
                step_total: Some(3),
                status: StageStatus::Waiting,
                prompt_tokens: Some(11),
                completion_tokens: Some(7),
                reasoning_tokens: Some(5),
                cache_read_tokens: Some(2),
                cache_write_tokens: Some(1),
                focus: Some("inspect scheduler".to_string()),
                last_event: Some("scheduler.stage.waiting".to_string()),
                waiting_on: Some("tool".to_string()),
                estimated_context_tokens: Some(99),
                skill_tree_budget: Some(512),
                skill_tree_truncation_strategy: Some("head".to_string()),
                skill_tree_truncated: Some(true),
                retry_attempt: Some(2),
                active_agent_count: 1,
                active_tool_count: 2,
                child_session_count: 0,
                primary_child_session_id: None,
            }],
            topology: SessionExecutionTopology {
                session_id: "session-1".to_string(),
                active_count: 1,
                done_count: 0,
                running_count: 0,
                waiting_count: 1,
                cancelling_count: 0,
                retry_count: 0,
                updated_at: Some(123),
                roots: Vec::new(),
            },
            usage: SessionUsage {
                input_tokens: 10,
                output_tokens: 20,
                reasoning_tokens: 3,
                cache_write_tokens: 4,
                cache_read_tokens: 5,
                total_cost: 0.12,
            },
        };

        let value = serde_json::to_value(&snapshot).expect("snapshot should serialize");

        assert!(value.get("runtime").is_some());
        assert!(value.get("stages").is_some());
        assert!(value.get("topology").is_some());
        assert!(value.get("usage").is_some());
        assert_eq!(value["runtime"]["active_stage_id"], "stage-1");
        assert_eq!(value["stages"][0]["status"], "waiting");
        assert_eq!(value["stages"][0]["skill_tree_truncated"], true);
        assert_eq!(value["topology"]["waiting_count"], 1);
        assert_eq!(value["usage"]["total_cost"], 0.12);
    }

    #[test]
    fn persisted_telemetry_snapshot_defaults_version_when_missing() {
        let value = serde_json::json!({
            "usage": {
                "input_tokens": 1,
                "output_tokens": 2,
                "reasoning_tokens": 3,
                "cache_write_tokens": 4,
                "cache_read_tokens": 5,
                "total_cost": 0.5
            },
            "stage_summaries": [],
            "last_run_status": "completed",
            "updated_at": 123
        });

        let parsed = serde_json::from_value::<rocode_session::SessionTelemetrySnapshot>(value)
            .expect("snapshot should deserialize with default version");

        assert_eq!(
            parsed.version,
            rocode_session::SessionTelemetrySnapshotVersion::V1
        );
    }

    #[tokio::test]
    async fn session_insights_returns_persisted_snapshot() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            let mut session = sessions.create("project", "/tmp/project");
            session.set_title("Telemetry Session");
            persist_session_telemetry_snapshot(
                &mut session,
                &rocode_session::SessionTelemetrySnapshot {
                    version: SessionTelemetrySnapshotVersion::V1,
                    usage: rocode_types::SessionUsage {
                        input_tokens: 10,
                        output_tokens: 20,
                        reasoning_tokens: 3,
                        cache_write_tokens: 4,
                        cache_read_tokens: 5,
                        total_cost: 0.25,
                    },
                    stage_summaries: vec![],
                    last_run_status: "completed".to_string(),
                    updated_at: 123,
                },
            )
            .expect("snapshot should persist");
            let id = session.id.clone();
            sessions.update(session);
            id
        };

        let Json(response) = get_session_insights(State(state), Path(session_id.clone()))
            .await
            .expect("insights route should succeed");

        assert_eq!(response.id, session_id);
        assert_eq!(response.title, "Telemetry Session");
        assert_eq!(response.directory, "/tmp/project");
        assert_eq!(
            response
                .telemetry
                .as_ref()
                .map(|snapshot| snapshot.last_run_status.as_str()),
            Some("completed")
        );
    }
}
