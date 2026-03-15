use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::session_runtime::events::{broadcast_server_event, ServerEvent};
use crate::{ApiError, Result, ServerState};

pub(crate) fn permission_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_permissions))
        .route("/{id}/reply", post(reply_permission))
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionRequestInfo {
    pub id: String,
    pub session_id: String,
    pub tool: String,
    pub input: serde_json::Value,
    pub message: String,
}

pub(crate) static PERMISSION_REQUESTS: Lazy<RwLock<HashMap<String, PermissionRequestInfo>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

async fn list_permissions() -> Json<Vec<PermissionRequestInfo>> {
    let pending = PERMISSION_REQUESTS.read().await;
    let mut result: Vec<_> = pending.values().cloned().collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Json(result)
}

#[derive(Debug, Deserialize)]
pub struct ReplyPermissionRequest {
    pub reply: String,
    pub message: Option<String>,
}

async fn reply_permission(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ReplyPermissionRequest>,
) -> Result<Json<bool>> {
    match req.reply.as_str() {
        "once" | "always" | "reject" => {}
        _ => {
            return Err(ApiError::BadRequest(
                "Invalid reply; expected `once`, `always`, or `reject`".to_string(),
            ));
        }
    }

    let mut pending = PERMISSION_REQUESTS.write().await;
    let permission = pending
        .remove(&id)
        .ok_or_else(|| ApiError::NotFound(format!("Permission request not found: {}", id)))?;

    if req.reply == "reject" {
        pending.retain(|_, item| item.session_id != permission.session_id);
    }

    broadcast_server_event(
        state.as_ref(),
        &ServerEvent::PermissionReplied {
            session_id: permission.session_id,
            request_id: id,
            reply: req.reply,
            message: req.message,
        },
    );
    Ok(Json(true))
}
