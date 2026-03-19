use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

use rocode_permission::{PermissionReply, PermissionReplyRequest, PermissionRequestInfo};

use crate::session_runtime::events::{broadcast_server_event, ServerEvent};
use crate::{ApiError, Result, ServerState};

pub(crate) fn permission_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_permissions))
        .route("/{id}/reply", post(reply_permission))
}

pub(crate) static PERMISSION_REQUESTS: Lazy<RwLock<HashMap<String, PermissionRequestInfo>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static PERMISSION_WAITERS: Lazy<Mutex<HashMap<String, oneshot::Sender<PermissionResolution>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug)]
struct PermissionResolution {
    reply: PermissionReply,
    message: Option<String>,
}

pub(crate) async fn request_permission(
    state: Arc<ServerState>,
    session_id: String,
    request: rocode_tool::PermissionRequest,
) -> std::result::Result<(), rocode_tool::ToolError> {
    let permission_id = format!("permission_{}", uuid::Uuid::new_v4().simple());
    let info =
        PermissionRequestInfo::from_request(permission_id.clone(), session_id.clone(), &request);
    let (tx, rx) = oneshot::channel();

    PERMISSION_REQUESTS
        .write()
        .await
        .insert(permission_id.clone(), info.clone());
    PERMISSION_WAITERS
        .lock()
        .await
        .insert(permission_id.clone(), tx);

    broadcast_server_event(
        state.as_ref(),
        &ServerEvent::PermissionRequested {
            session_id: session_id.clone(),
            permission_id: permission_id.clone(),
            info: info.clone(),
        },
    );

    // Update aggregated runtime state: pending permission.
    state
        .runtime_state
        .permission_requested(
            &session_id,
            &permission_id,
            serde_json::to_value(&info).unwrap_or(serde_json::Value::Null),
        )
        .await;

    let wait_result = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;
    PERMISSION_WAITERS.lock().await.remove(&permission_id);

    // Clear pending permission from aggregated runtime state.
    state.runtime_state.permission_resolved(&session_id).await;

    match wait_result {
        Ok(Ok(PermissionResolution { reply, message })) => match reply {
            PermissionReply::Once | PermissionReply::Always => Ok(()),
            PermissionReply::Reject => Err(rocode_tool::ToolError::PermissionDenied(
                message
                    .unwrap_or_else(|| format!("Permission rejected for {}", request.permission)),
            )),
        },
        Ok(Err(_)) => {
            PERMISSION_REQUESTS.write().await.remove(&permission_id);
            Err(rocode_tool::ToolError::ExecutionError(
                "Permission response channel closed".to_string(),
            ))
        }
        Err(_) => {
            PERMISSION_REQUESTS.write().await.remove(&permission_id);
            Err(rocode_tool::ToolError::PermissionDenied(
                "Permission request timed out".to_string(),
            ))
        }
    }
}

async fn list_permissions() -> Json<Vec<PermissionRequestInfo>> {
    let pending = PERMISSION_REQUESTS.read().await;
    let mut result: Vec<_> = pending.values().cloned().collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Json(result)
}

async fn reply_permission(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<PermissionReplyRequest>,
) -> Result<Json<bool>> {
    let mut pending = PERMISSION_REQUESTS.write().await;
    let permission = pending
        .remove(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Permission request not found: {}", id)))?;
    drop(pending);

    if let Some(waiter) = PERMISSION_WAITERS.lock().await.remove(&id) {
        let _ = waiter.send(PermissionResolution {
            reply: req.reply,
            message: req.message.clone(),
        });
    }

    broadcast_server_event(
        state.as_ref(),
        &ServerEvent::PermissionResolved {
            session_id: permission.session_id,
            permission_id: id,
            reply: req.reply,
            message: req.message,
        },
    );
    Ok(Json(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Path;
    use axum::extract::State;
    use axum::Json;

    #[tokio::test]
    async fn request_permission_emits_requested_and_resolved_events() {
        let state = Arc::new(ServerState::new());
        let mut rx = state.event_bus.subscribe();

        let state_for_request = state.clone();
        let request_task = tokio::spawn(async move {
            request_permission(
                state_for_request,
                "session-1".to_string(),
                rocode_tool::PermissionRequest::new("bash")
                    .with_pattern("cargo test")
                    .with_metadata("command", serde_json::json!("cargo test")),
            )
            .await
        });

        let permission_id = loop {
            let pending = PERMISSION_REQUESTS.read().await;
            if let Some(id) = pending.keys().next().cloned() {
                break id;
            }
            drop(pending);
            tokio::task::yield_now().await;
        };

        let requested = rx.recv().await.expect("requested event");
        let requested_json: serde_json::Value =
            serde_json::from_str(&requested).expect("requested json");
        assert_eq!(requested_json["type"], "permission.requested");
        assert_eq!(requested_json["permissionID"], permission_id);
        assert_eq!(requested_json["sessionID"], "session-1");

        let reply = PermissionReplyRequest {
            reply: PermissionReply::Once,
            message: Some("approved".to_string()),
        };
        let _ = reply_permission(
            State(state.clone()),
            Path(permission_id.clone()),
            Json(reply),
        )
        .await
        .expect("reply should succeed");

        let resolved = rx.recv().await.expect("resolved event");
        let resolved_json: serde_json::Value =
            serde_json::from_str(&resolved).expect("resolved json");
        assert_eq!(resolved_json["type"], "permission.resolved");
        assert_eq!(resolved_json["permissionID"], permission_id);
        assert_eq!(resolved_json["reply"], "once");

        request_task
            .await
            .expect("request task join")
            .expect("permission allowed");
    }
}
