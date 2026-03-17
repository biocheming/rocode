use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::runtime_control::SessionRunStatus;
use crate::session_runtime::events::broadcast_session_updated;
use crate::{ApiError, Result, ServerState};

// ─── Request / Response structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub directory: Option<String>,
    pub roots: Option<bool>,
    pub start: Option<i64>,
    pub search: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    pub project_id: String,
    pub directory: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub version: String,
    pub time: SessionTimeInfo,
    pub summary: Option<SessionSummaryInfo>,
    pub share: Option<SessionShareInfo>,
    pub revert: Option<SessionRevertInfo>,
    pub permission: Option<PermissionRulesetInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize)]
pub struct SessionTimeInfo {
    pub created: i64,
    pub updated: i64,
    pub compacting: Option<i64>,
    pub archived: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryInfo {
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionShareInfo {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct SessionRevertInfo {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionRulesetInfo {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionStatusInfo {
    pub status: String,
    pub idle: bool,
    pub busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TodoInfo {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Serialize)]
pub struct FileDiffInfo {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub parent_id: Option<String>,
    pub scheduler_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PermissionRulesetInput {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionTimeRequest {
    pub archived: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub time: Option<UpdateSessionTimeRequest>,
}

#[derive(Debug, Deserialize)]
pub struct ForkSessionRequest {
    pub message_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveSessionRequest {
    pub archive: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetTitleRequest {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct SetSummaryRequest {
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub files: Option<u64>,
    pub diffs: Option<Vec<SetSummaryFileDiff>>,
}

#[derive(Debug, Deserialize)]
pub struct SetSummaryFileDiff {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Deserialize)]
pub struct RevertRequest {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePartRequest {
    pub part: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteShellRequest {
    pub command: String,
    pub workdir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    pub command: String,
    pub arguments: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
}

// ─── Helpers ──────────────────────────────────────────────────────────

pub(super) fn session_to_info(session: &rocode_session::Session) -> SessionInfo {
    SessionInfo {
        id: session.id.clone(),
        slug: session.slug.clone(),
        project_id: session.project_id.clone(),
        directory: session.directory.clone(),
        parent_id: session.parent_id.clone(),
        title: session.title.clone(),
        version: session.version.clone(),
        time: SessionTimeInfo {
            created: session.time.created,
            updated: session.time.updated,
            compacting: session.time.compacting,
            archived: session.time.archived,
        },
        summary: session.summary.as_ref().map(|s| SessionSummaryInfo {
            additions: s.additions,
            deletions: s.deletions,
            files: s.files,
        }),
        share: session
            .share
            .as_ref()
            .map(|s| SessionShareInfo { url: s.url.clone() }),
        revert: session.revert.as_ref().map(|r| SessionRevertInfo {
            message_id: r.message_id.clone(),
            part_id: r.part_id.clone(),
            snapshot: r.snapshot.clone(),
            diff: r.diff.clone(),
        }),
        permission: session.permission.as_ref().map(|p| PermissionRulesetInfo {
            allow: p.allow.clone(),
            deny: p.deny.clone(),
            mode: p.mode.clone(),
        }),
        metadata: if session.metadata.is_empty() {
            None
        } else {
            Some(session.metadata.clone())
        },
    }
}

pub(super) async fn persist_sessions_if_enabled(state: &Arc<ServerState>) {
    if let Err(err) = state.sync_sessions_to_storage().await {
        tracing::error!("failed to sync sessions to storage: {}", err);
    }
}

pub(crate) fn resolved_session_directory(raw: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let trimmed = raw.trim();
    let candidate = if trimmed.is_empty() || trimmed == "." {
        cwd
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    candidate
        .canonicalize()
        .unwrap_or(candidate)
        .to_string_lossy()
        .to_string()
}

pub(super) fn session_model_override(session: &rocode_session::Session) -> Option<String> {
    session
        .metadata
        .get("model_provider")
        .and_then(|value| value.as_str())
        .zip(
            session
                .metadata
                .get("model_id")
                .and_then(|value| value.as_str()),
        )
        .map(|(provider, model)| format!("{provider}/{model}"))
}

pub(super) fn session_variant_override(session: &rocode_session::Session) -> Option<String> {
    session
        .metadata
        .get("model_variant")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

pub(super) fn session_agent_override(session: &rocode_session::Session) -> Option<String> {
    session
        .metadata
        .get("agent")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

pub(super) fn session_scheduler_profile_override(
    session: &rocode_session::Session,
) -> Option<String> {
    session
        .metadata
        .get("scheduler_profile")
        .or_else(|| session.metadata.get("resolved_scheduler_profile"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

pub(super) async fn set_session_run_status(
    state: &Arc<ServerState>,
    session_id: &str,
    status: SessionRunStatus,
) {
    state
        .runtime_control
        .set_session_run_status(session_id, status.clone())
        .await;

    // Mirror run-status transition into the aggregated SessionRuntimeState.
    match &status {
        SessionRunStatus::Busy => {
            state.runtime_state.mark_running(session_id, None).await;
        }
        SessionRunStatus::Idle => {
            state.runtime_state.mark_idle(session_id).await;
        }
        SessionRunStatus::Retry { .. } => {
            // Retry is still a "running" variant from the runtime state
            // perspective — the session is not idle.
            state.runtime_state.mark_running(session_id, None).await;
        }
    }

    state.broadcast(
        &serde_json::json!({
            "type": "session.status",
            "sessionID": session_id,
            "status": status,
        })
        .to_string(),
    );
}

/// Drop guard that sets session status to idle when the prompt task exits.
/// Mirrors the TS `defer(() => cancel(sessionID))` pattern to guarantee
/// the spinner stops even if the spawned task panics.
pub(super) struct IdleGuard {
    pub state: Arc<ServerState>,
    pub session_id: Option<String>,
}

impl IdleGuard {
    /// Defuse the guard — the caller will handle cleanup explicitly.
    pub fn defuse(&mut self) {
        self.session_id = None;
    }
}

impl Drop for IdleGuard {
    fn drop(&mut self) {
        let Some(sid) = self.session_id.take() else {
            return;
        };
        let state = self.state.clone();
        tokio::spawn(async move {
            set_session_run_status(&state, &sid, SessionRunStatus::Idle).await;
        });
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────

pub(super) async fn list_sessions(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<Vec<SessionInfo>>> {
    let filter = rocode_session::SessionFilter {
        directory: query.directory,
        roots: query.roots.unwrap_or(false),
        start: query.start,
        search: query.search,
        limit: query.limit,
    };
    let manager = state.sessions.lock().await;
    let sessions = manager.list_filtered(filter);
    let infos: Vec<SessionInfo> = sessions.into_iter().map(session_to_info).collect();
    Ok(Json(infos))
}

pub(super) async fn session_status(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<HashMap<String, SessionStatusInfo>>> {
    let run_status = state.runtime_control.session_run_statuses().await;
    let manager = state.sessions.lock().await;
    let sessions = manager.list();
    let status: HashMap<String, SessionStatusInfo> = sessions
        .into_iter()
        .map(|s| {
            let lifecycle_status = match s.status {
                rocode_session::SessionStatus::Active => "active",
                rocode_session::SessionStatus::Completed => "completed",
                rocode_session::SessionStatus::Archived => "archived",
                rocode_session::SessionStatus::Compacting => "compacting",
            };
            let run = run_status.get(&s.id).cloned().unwrap_or_default();
            let (status, idle, busy, attempt, message, next) = match run {
                SessionRunStatus::Idle => {
                    (lifecycle_status.to_string(), true, false, None, None, None)
                }
                SessionRunStatus::Busy => ("busy".to_string(), false, true, None, None, None),
                SessionRunStatus::Retry {
                    attempt,
                    message,
                    next,
                } => (
                    "retry".to_string(),
                    false,
                    true,
                    Some(attempt),
                    Some(message),
                    Some(next),
                ),
            };
            (
                s.id.clone(),
                SessionStatusInfo {
                    status,
                    idle,
                    busy,
                    attempt,
                    message,
                    next,
                },
            )
        })
        .collect();
    Ok(Json(status))
}

pub(super) async fn create_session(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let mut session = if let Some(parent_id) = &req.parent_id {
        sessions
            .create_child(parent_id)
            .ok_or_else(|| ApiError::SessionNotFound(parent_id.clone()))?
    } else {
        sessions.create("default", resolved_session_directory("."))
    };
    let normalized_directory = resolved_session_directory(&session.directory);
    if session.directory != normalized_directory {
        session.directory = normalized_directory;
        sessions.update(session.clone());
    }
    if let Some(profile) = req
        .scheduler_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        session
            .metadata
            .insert("scheduler_profile".to_string(), serde_json::json!(profile));
        session
            .metadata
            .insert("scheduler_applied".to_string(), serde_json::json!(true));
        sessions.update(session.clone());
    }
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(session_to_info(&session)))
}

pub(super) async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&id).ok_or(ApiError::SessionNotFound(id))?;
    Ok(Json(session_to_info(session)))
}

pub(super) async fn update_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;

    if let Some(title) = req.title {
        session.set_title(title);
    }
    if let Some(time) = req.time {
        if let Some(archived) = time.archived {
            session.set_archived(Some(archived));
        }
    }
    let info = session_to_info(session);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn delete_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .sessions
        .lock()
        .await
        .delete(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    state
        .runtime_control
        .set_session_run_status(&id, SessionRunStatus::Idle)
        .await;
    state.runtime_state.remove(&id).await;
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// `GET /session/{id}/runtime` — aggregated runtime state snapshot for a session.
pub(super) async fn get_session_runtime(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::session_runtime::state::SessionRuntimeState>> {
    match state.runtime_state.get(&id).await {
        Some(runtime) => Ok(Json(runtime)),
        None => {
            // Session may exist but never had a prompt run. Return a default idle state.
            let sessions = state.sessions.lock().await;
            if sessions.get(&id).is_some() {
                drop(sessions);
                Ok(Json(
                    crate::session_runtime::state::SessionRuntimeState::new(id),
                ))
            } else {
                Err(ApiError::SessionNotFound(id))
            }
        }
    }
}

pub(super) async fn get_session_children(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionInfo>>> {
    let manager = state.sessions.lock().await;
    let children = manager.children(&id);
    Ok(Json(children.into_iter().map(session_to_info).collect()))
}

pub(super) async fn get_session_todos(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TodoInfo>>> {
    let sessions = state.sessions.lock().await;
    if sessions.get(&id).is_none() {
        return Err(ApiError::SessionNotFound(id));
    }
    drop(sessions);

    let todos = state.todo_manager.get(&id).await;
    let items = todos
        .into_iter()
        .enumerate()
        .map(|(idx, todo)| TodoInfo {
            id: format!("{}_{}", id, idx),
            content: todo.content,
            status: todo.status,
            priority: todo.priority,
        })
        .collect();
    Ok(Json(items))
}

pub(super) async fn fork_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ForkSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let forked = state
        .sessions
        .lock()
        .await
        .fork(&id, req.message_id.as_deref())
        .ok_or(ApiError::SessionNotFound(id))?;
    persist_sessions_if_enabled(&state).await;
    Ok(Json(session_to_info(&forked)))
}

pub(super) async fn share_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionShareInfo>> {
    let mut sessions = state.sessions.lock().await;
    let share_url = format!("https://share.opencode.ai/{}", id);
    sessions
        .share(&id, share_url.clone())
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(SessionShareInfo { url: share_url }))
}

pub(super) async fn unshare_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    sessions
        .unshare(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "unshared": true })))
}

pub(super) async fn archive_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ArchiveSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let info = if req.archive.unwrap_or(true) {
        let updated = sessions
            .set_archived(&id, None)
            .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
        session_to_info(&updated)
    } else {
        let session = sessions
            .get(&id)
            .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
        session_to_info(session)
    };
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn set_session_title(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<SetTitleRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.set_title(&req.title);
    let updated = session.clone();
    sessions.update(updated.clone());
    let info = session_to_info(&updated);
    drop(sessions);
    broadcast_session_updated(state.as_ref(), id, "session.title.set");
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn set_session_permission(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<PermissionRulesetInput>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_permission(
            &id,
            rocode_session::PermissionRuleset {
                allow: req.allow.unwrap_or_default(),
                deny: req.deny.unwrap_or_default(),
                mode: req.mode,
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn get_session_summary(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Option<SessionSummaryInfo>>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    Ok(Json(session.summary.as_ref().map(|s| SessionSummaryInfo {
        additions: s.additions,
        deletions: s.deletions,
        files: s.files,
    })))
}

pub(super) async fn set_session_summary(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<SetSummaryRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_summary(
            &id,
            rocode_session::SessionSummary {
                additions: req.additions.unwrap_or(0),
                deletions: req.deletions.unwrap_or(0),
                files: req.files.unwrap_or(0),
                diffs: req.diffs.map(|diffs| {
                    diffs
                        .into_iter()
                        .map(|d| rocode_session::FileDiff {
                            path: d.path,
                            additions: d.additions,
                            deletions: d.deletions,
                        })
                        .collect()
                }),
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn session_revert(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_revert(
            &id,
            rocode_session::SessionRevert {
                message_id: req.message_id,
                part_id: req.part_id,
                snapshot: req.snapshot,
                diff: req.diff,
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn clear_session_revert(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .clear_revert(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn start_compaction(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.start_compacting();
    let info = session_to_info(session);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn get_message(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let info = serde_json::json!({
        "id": message.id,
        "sessionID": session_id,
        "role": super::messages::message_role_name(&message.role),
        "createdAt": message.created_at.timestamp_millis(),
    });
    Ok(Json(serde_json::json!({
        "info": info,
        "parts": message.parts.clone(),
    })))
}

pub(super) async fn update_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id, part_id)): Path<(String, String, String)>,
    Json(req): Json<UpdatePartRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let mut part: rocode_session::MessagePart = serde_json::from_value(req.part)
        .map_err(|e| ApiError::BadRequest(format!("Invalid part payload: {}", e)))?;
    if part.id != part_id {
        return Err(ApiError::BadRequest(format!(
            "Part id mismatch: body has {}, path has {}",
            part.id, part_id
        )));
    }
    part.message_id = Some(msg_id.clone());

    let updated_part = {
        let target = message
            .parts
            .iter_mut()
            .find(|existing| existing.id == part_id)
            .ok_or_else(|| ApiError::NotFound(format!("Part not found: {}", part_id)))?;
        *target = part.clone();
        target.clone()
    };
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "updated": true,
        "part": updated_part,
    })))
}

pub(super) async fn execute_shell(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteShellRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.add_user_message(format!("$ {}", req.command));
    let assistant = session.add_assistant_message();
    assistant.add_text(format!("Shell command queued: {}", req.command));
    let assistant_id = assistant.id.clone();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "executed": true,
        "command": req.command,
        "workdir": req.workdir,
        "message_id": assistant_id,
    })))
}

pub(super) async fn session_unrevert(Path(_id): Path<String>) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({ "unreverted": true, "message": "Session unreverted successfully" }),
    ))
}

pub(super) async fn execute_command(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let text = req
        .arguments
        .as_deref()
        .map(|args| format!("/{cmd} {args}", cmd = req.command))
        .unwrap_or_else(|| format!("/{}", req.command));
    session.add_user_message(text);
    let assistant = session.add_assistant_message();
    assistant.add_text(format!("Command queued: {}", req.command));
    let assistant_id = assistant.id.clone();
    let arguments = req
        .arguments
        .as_deref()
        .map(|value| {
            value
                .split_whitespace()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    sessions.publish_command_executed(&req.command, &id, arguments, &assistant_id);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "executed": true,
        "command": req.command,
        "arguments": req.arguments,
        "model": req.model,
        "agent": req.agent,
        "message_id": assistant_id,
    })))
}

pub(super) async fn get_session_diff(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<FileDiffInfo>>> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&id).ok_or(ApiError::SessionNotFound(id))?;
    let diffs = session
        .summary
        .as_ref()
        .and_then(|summary| summary.diffs.as_ref())
        .map(|items| {
            items
                .iter()
                .map(|diff| FileDiffInfo {
                    path: diff.path.clone(),
                    additions: diff.additions,
                    deletions: diff.deletions,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Json(diffs))
}

pub(super) async fn cancel_tool_call(
    State(state): State<Arc<ServerState>>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    // Verify the tool call exists in the session (hold lock briefly).
    {
        let sessions = state.sessions.lock().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;

        let found = session.messages.iter().any(|msg| {
            msg.parts.iter().any(|part| {
                matches!(
                    &part.part_type,
                    rocode_session::PartType::ToolCall { id, .. } if id == &tool_call_id
                )
            })
        });

        if !found {
            return Err(ApiError::NotFound(format!(
                "Tool call {} not found in session {}",
                tool_call_id, session_id
            )));
        }
    }

    // Look up the plugin request mapping from global tracking
    if let Some(tracking) = rocode_plugin::subprocess::get_tool_call_tracking(&tool_call_id).await {
        // Get the plugin loader and cancel the request
        if let Some(loader) = super::super::get_plugin_loader() {
            let clients = loader.clients().await;
            if let Some(plugin) = clients
                .iter()
                .find(|c| c.plugin_id() == tracking.plugin_name)
            {
                if let Err(e) = plugin.cancel_request(tracking.request_id).await {
                    tracing::warn!(
                        tool_call_id = %tool_call_id,
                        plugin_name = %tracking.plugin_name,
                        request_id = %tracking.request_id,
                        error = %e,
                        "Failed to send cancel request to plugin"
                    );
                    return Ok(Json(serde_json::json!({
                        "cancelled": false,
                        "message": format!("Failed to cancel: {}", e)
                    })));
                }

                // Remove from tracking
                rocode_plugin::subprocess::remove_tool_call_tracking(&tool_call_id).await;

                return Ok(Json(serde_json::json!({
                    "cancelled": true,
                    "message": "Cancel request sent to plugin"
                })));
            }
        }

        return Ok(Json(serde_json::json!({
            "cancelled": false,
            "message": "Plugin not found or not loaded"
        })));
    }

    Ok(Json(serde_json::json!({
        "cancelled": false,
        "message": "Tool call is not currently executing or not tracked"
    })))
}

#[derive(Debug, Deserialize)]
pub struct PromptAsyncRequest {
    pub message: Option<String>,
    pub model: Option<String>,
}

pub(super) async fn prompt_async(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<PromptAsyncRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let text = req
        .message
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("Field `message` is required".to_string()))?;
    session.add_user_message(text);
    let assistant = session.add_assistant_message();
    let assistant_id = assistant.id.clone();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "status": "queued",
        "message_id": assistant_id,
        "model": req.model,
    })))
}
