use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::runtime_control::SessionRunStatus;
use crate::session_runtime::events::{
    broadcast_server_event, broadcast_session_updated, ServerEvent,
};
use crate::{ApiError, Result, ServerState};

fn map_storage_err(err: rocode_storage::DatabaseError) -> ApiError {
    ApiError::InternalError(format!("storage error: {}", err))
}

// ─── Request / Response structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub directory: Option<String>,
    pub roots: Option<bool>,
    pub start: Option<i64>,
    pub search: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
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
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryInfo {
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
}

#[derive(Debug, Serialize)]
pub(super) struct DeletedResponse {
    deleted: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct UnsharedResponse {
    unshared: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct MessageInfoResponse {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    role: rocode_session::Role,
    #[serde(rename = "createdAt")]
    created_at: i64,
}

#[derive(Debug, Serialize)]
pub(super) struct MessageDetailResponse {
    info: MessageInfoResponse,
    parts: Vec<rocode_session::MessagePart>,
}

#[derive(Debug, Serialize)]
pub(super) struct UpdatePartResponse {
    updated: bool,
    part: rocode_session::MessagePart,
}

#[derive(Debug, Serialize)]
pub(super) struct ExecuteShellResponse {
    executed: bool,
    command: String,
    workdir: Option<String>,
    message_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct UnrevertResponse {
    unreverted: bool,
    message: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ExecuteCommandResponse {
    executed: bool,
    command: String,
    arguments: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    message_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct CancelToolCallResponse {
    cancelled: bool,
    message: String,
}

#[derive(Debug, Serialize)]
pub(super) struct PromptAsyncResponse {
    status: &'static str,
    message_id: String,
    model: Option<String>,
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

pub type PermissionRulesetInfo = rocode_session::PermissionRuleset;

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

pub type PermissionRulesetInput = rocode_session::PermissionRuleset;

#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForkSessionRequest {
    pub message_id: Option<String>,
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
        directory: session.directory.clone(),
        parent_id: session.parent_id.clone(),
        title: session.title.clone(),
        version: session.version.clone(),
        time: SessionTimeInfo {
            created: session.time.created,
            updated: session.time.updated,
        },
        summary: session.summary.as_ref().map(|s| SessionSummaryInfo {
            additions: s.additions,
            deletions: s.deletions,
            files: s.files,
        }),
        share: session
            .share
            .as_ref()
            .map(|url| SessionShareInfo { url: url.clone() }),
        revert: session.revert.as_ref().map(|r| SessionRevertInfo {
            message_id: r.message_id.clone(),
            part_id: r.part_id.clone(),
            snapshot: r.snapshot.clone(),
            diff: r.diff.clone(),
        }),
        permission: session.permission.clone(),
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
struct SessionMetadataWire {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_provider: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_variant: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    agent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    scheduler_profile: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    resolved_scheduler_profile: Option<String>,
}

fn session_metadata_wire(metadata: &HashMap<String, serde_json::Value>) -> SessionMetadataWire {
    let Ok(value) = serde_json::to_value(metadata) else {
        return SessionMetadataWire::default();
    };
    serde_json::from_value::<SessionMetadataWire>(value).unwrap_or_default()
}

pub(super) fn session_model_override(session: &rocode_session::Session) -> Option<String> {
    let wire = session_metadata_wire(&session.metadata);
    match (wire.model_provider.as_deref(), wire.model_id.as_deref()) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        _ => None,
    }
}

pub(super) fn session_variant_override(session: &rocode_session::Session) -> Option<String> {
    session_metadata_wire(&session.metadata).model_variant
}

pub(super) fn session_agent_override(session: &rocode_session::Session) -> Option<String> {
    session_metadata_wire(&session.metadata).agent
}

pub(super) fn session_scheduler_profile_override(
    session: &rocode_session::Session,
) -> Option<String> {
    let wire = session_metadata_wire(&session.metadata);
    wire.scheduler_profile.or(wire.resolved_scheduler_profile)
}

pub(super) async fn set_session_run_status(
    state: &Arc<ServerState>,
    session_id: &str,
    status: SessionRunStatus,
) {
    let is_running = !matches!(status, SessionRunStatus::Idle);
    {
        let mut sessions = state.sessions.lock().await;
        let should_update = sessions
            .get(session_id)
            .map(|session| session.active != is_running)
            .unwrap_or(false);
        if should_update {
            let _ = sessions.mutate_session(session_id, |session| {
                session.set_active(is_running);
            });
        }
    }
    persist_sessions_if_enabled(state).await;

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

    broadcast_server_event(
        state.as_ref(),
        &ServerEvent::SessionStatus {
            session_id: session_id.to_string(),
            status: rocode_types::SessionRunStatusWire::Tagged(match status {
                SessionRunStatus::Idle => rocode_types::SessionRunStatus::Idle,
                SessionRunStatus::Busy => rocode_types::SessionRunStatus::Busy,
                SessionRunStatus::Retry {
                    attempt,
                    message,
                    next,
                } => rocode_types::SessionRunStatus::Retry {
                    attempt,
                    message,
                    next,
                },
            }),
        },
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
) -> Result<(HeaderMap, Json<Vec<SessionInfo>>)> {
    let ListSessionsQuery {
        directory,
        roots,
        start,
        search,
        limit,
        offset,
    } = query;

    // Normalize directory filtering to match how sessions are stored (canonical absolute paths).
    // This lets clients pass "." or relative paths and still get consistent results.
    let directory = directory.map(|raw| resolved_session_directory(&raw));

    let filter = rocode_session::SessionFilter {
        directory,
        roots: roots.unwrap_or(false),
        start,
        search,
        limit,
        offset,
    };
    let manager = state.sessions.lock().await;
    let (total, sessions) = manager.list_filtered_with_total(filter);
    let infos: Vec<SessionInfo> = sessions.into_iter().map(session_to_info).collect();

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Total-Count",
        HeaderValue::from_str(&total.to_string()).unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    headers.insert(
        "X-Returned-Count",
        HeaderValue::from_str(&infos.len().to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    headers.insert(
        "X-Offset",
        HeaderValue::from_str(&offset.unwrap_or(0).to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    if let Some(limit) = limit {
        headers.insert(
            "X-Limit",
            HeaderValue::from_str(&limit.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
    }

    Ok((headers, Json(infos)))
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
            let run = run_status.get(&s.id).cloned().unwrap_or_default();
            let (status, idle, busy, attempt, message, next) = match run {
                SessionRunStatus::Idle => {
                    if s.active {
                        ("busy".to_string(), false, true, None, None, None)
                    } else {
                        ("idle".to_string(), true, false, None, None, None)
                    }
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
        sessions.create(resolved_session_directory("."))
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
    let info = sessions
        .mutate_session(&id, |session| {
            if let Some(title) = req.title {
                session.set_title(title);
            }
            session_to_info(session)
        })
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

pub(super) async fn delete_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<DeletedResponse>> {
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
    state.clear_session_cache(&id).await;
    persist_sessions_if_enabled(&state).await;
    Ok(Json(DeletedResponse { deleted: true }))
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
    if !state
        .ensure_session_hydrated(&id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(id));
    }

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
) -> Result<Json<UnsharedResponse>> {
    let mut sessions = state.sessions.lock().await;
    sessions
        .unshare(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(UnsharedResponse { unshared: true }))
}

pub(super) async fn set_session_title(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<SetTitleRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .mutate_session(&id, |session| {
            session.set_title(&req.title);
            session.clone()
        })
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
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
        .set_permission(&id, req)
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

pub(super) async fn get_message(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
) -> Result<Json<MessageDetailResponse>> {
    let session_exists = state
        .ensure_session_loaded(&session_id)
        .await
        .map_err(|err| {
            ApiError::InternalError(format!("failed to load session metadata: {}", err))
        })?;
    if !session_exists {
        return Err(ApiError::SessionNotFound(session_id));
    }

    if let Some(message_repo) = state.message_repo.as_ref() {
        if let Some(message) = message_repo.get(&msg_id).await.map_err(map_storage_err)? {
            if message.session_id != session_id {
                return Err(ApiError::NotFound(format!("Message not found: {}", msg_id)));
            }
            let info = MessageInfoResponse {
                id: message.id.clone(),
                session_id,
                role: message.role,
                created_at: message.created_at.timestamp_millis(),
            };
            return Ok(Json(MessageDetailResponse {
                info,
                parts: message.parts,
            }));
        }
        return Err(ApiError::NotFound(format!("Message not found: {}", msg_id)));
    }

    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;
    let info = MessageInfoResponse {
        id: message.id.clone(),
        session_id,
        role: message.role,
        created_at: message.created_at.timestamp_millis(),
    };
    Ok(Json(MessageDetailResponse {
        info,
        parts: message.parts.clone(),
    }))
}

pub(super) async fn update_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id, part_id)): Path<(String, String, String)>,
    Json(req): Json<UpdatePartRequest>,
) -> Result<Json<UpdatePartResponse>> {
    if !state
        .ensure_session_hydrated(&session_id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(session_id));
    }

    let mut part: rocode_session::MessagePart = serde_json::from_value(req.part)
        .map_err(|e| ApiError::BadRequest(format!("Invalid part payload: {}", e)))?;
    if part.id != part_id {
        return Err(ApiError::BadRequest(format!(
            "Part id mismatch: body has {}, path has {}",
            part.id, part_id
        )));
    }
    part.message_id = Some(msg_id.clone());

    let mut sessions = state.sessions.lock().await;
    let updated_part = sessions
        .mutate_session(&session_id, |session| {
            let message = session
                .get_message_mut(&msg_id)
                .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;
            let target = message
                .parts
                .iter_mut()
                .find(|existing| existing.id == part_id)
                .ok_or_else(|| ApiError::NotFound(format!("Part not found: {}", part_id)))?;
            *target = part.clone();
            let updated_part = target.clone();
            session.touch();
            Ok::<rocode_session::MessagePart, ApiError>(updated_part)
        })
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))??;
    drop(sessions);
    state.touch_session_cache(&session_id).await;
    persist_sessions_if_enabled(&state).await;

    Ok(Json(UpdatePartResponse {
        updated: true,
        part: updated_part,
    }))
}

pub(super) async fn execute_shell(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteShellRequest>,
) -> Result<Json<ExecuteShellResponse>> {
    if !state
        .ensure_session_hydrated(&id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(id));
    }

    let mut sessions = state.sessions.lock().await;
    let assistant_id = sessions
        .mutate_session(&id, |session| {
            session.add_user_message(format!("$ {}", req.command));
            let assistant = session.add_assistant_message();
            assistant.add_text(format!("Shell command queued: {}", req.command));
            assistant.id.clone()
        })
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    state.touch_session_cache(&id).await;
    persist_sessions_if_enabled(&state).await;

    Ok(Json(ExecuteShellResponse {
        executed: true,
        command: req.command,
        workdir: req.workdir,
        message_id: assistant_id,
    }))
}

pub(super) async fn session_unrevert(Path(_id): Path<String>) -> Result<Json<UnrevertResponse>> {
    Ok(Json(UnrevertResponse {
        unreverted: true,
        message: "Session unreverted successfully",
    }))
}

pub(super) async fn execute_command(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> Result<Json<ExecuteCommandResponse>> {
    if !state
        .ensure_session_hydrated(&id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(id));
    }

    let mut sessions = state.sessions.lock().await;
    let text = req
        .arguments
        .as_deref()
        .map(|args| format!("/{cmd} {args}", cmd = req.command))
        .unwrap_or_else(|| format!("/{}", req.command));
    let assistant_id = sessions
        .mutate_session(&id, |session| {
            session.add_user_message(text);
            let assistant = session.add_assistant_message();
            assistant.add_text(format!("Command queued: {}", req.command));
            assistant.id.clone()
        })
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
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
    state.touch_session_cache(&id).await;
    persist_sessions_if_enabled(&state).await;

    Ok(Json(ExecuteCommandResponse {
        executed: true,
        command: req.command,
        arguments: req.arguments,
        model: req.model,
        agent: req.agent,
        message_id: assistant_id,
    }))
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
) -> Result<Json<CancelToolCallResponse>> {
    if !state
        .ensure_session_hydrated(&session_id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(session_id));
    }

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
                    return Ok(Json(CancelToolCallResponse {
                        cancelled: false,
                        message: format!("Failed to cancel: {}", e),
                    }));
                }

                // Remove from tracking
                rocode_plugin::subprocess::remove_tool_call_tracking(&tool_call_id).await;

                return Ok(Json(CancelToolCallResponse {
                    cancelled: true,
                    message: "Cancel request sent to plugin".to_string(),
                }));
            }
        }

        return Ok(Json(CancelToolCallResponse {
            cancelled: false,
            message: "Plugin not found or not loaded".to_string(),
        }));
    }

    Ok(Json(CancelToolCallResponse {
        cancelled: false,
        message: "Tool call is not currently executing or not tracked".to_string(),
    }))
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
) -> Result<Json<PromptAsyncResponse>> {
    if !state
        .ensure_session_hydrated(&id)
        .await
        .map_err(|err| ApiError::InternalError(format!("failed to hydrate session: {}", err)))?
    {
        return Err(ApiError::SessionNotFound(id));
    }

    let mut sessions = state.sessions.lock().await;
    let text = req
        .message
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("Field `message` is required".to_string()))?;
    let assistant_id = sessions
        .mutate_session(&id, |session| {
            session.add_user_message(text);
            session.add_assistant_message().id.clone()
        })
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    state.touch_session_cache(&id).await;
    persist_sessions_if_enabled(&state).await;

    Ok(Json(PromptAsyncResponse {
        status: "queued",
        message_id: assistant_id,
        model: req.model,
    }))
}
