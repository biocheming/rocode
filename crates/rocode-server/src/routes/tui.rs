use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify};

pub(crate) use crate::runtime_control::QuestionInfo;
use crate::runtime_control::QuestionReply;
use crate::{ApiError, Result, ServerState};

pub(crate) fn question_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_questions))
        .route("/{id}/reply", post(reply_question))
        .route("/{id}/reject", post(reject_question))
}

pub(crate) fn tui_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/append-prompt", post(append_prompt))
        .route("/set-prompt", post(set_prompt))
        .route("/submit-prompt", post(submit_prompt))
        .route("/clear-prompt", post(clear_prompt))
        .route("/open-help", post(open_help))
        .route("/open-sessions", post(open_sessions))
        .route("/open-themes", post(open_themes))
        .route("/open-models", post(open_models))
        .route("/execute-command", post(execute_tui_command))
        .route("/show-toast", post(show_toast))
        .route("/publish", post(publish_tui_event))
        .route("/select-session", post(select_session))
        .route("/control/next", get(get_next_tui_request))
        .route("/control/response", post(submit_tui_response))
}

pub(crate) type QuestionEventHook = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

pub(crate) async fn request_question_answers(
    state: Arc<ServerState>,
    session_id: String,
    questions: Vec<rocode_tool::QuestionDef>,
) -> std::result::Result<Vec<Vec<String>>, rocode_tool::ToolError> {
    request_question_answers_with_hook(state, session_id, questions, None).await
}

pub(crate) async fn request_question_answers_with_hook(
    state: Arc<ServerState>,
    session_id: String,
    questions: Vec<rocode_tool::QuestionDef>,
    event_hook: Option<QuestionEventHook>,
) -> std::result::Result<Vec<Vec<String>>, rocode_tool::ToolError> {
    if questions.is_empty() {
        return Ok(Vec::new());
    }

    let (question_info, rx) = state
        .runtime_control
        .register_question(session_id.clone(), questions.clone())
        .await;
    let request_id = question_info.id.clone();

    let created_event = serde_json::json!({
        "type": "question.created",
        "requestID": request_id,
        "sessionID": session_id,
        "questions": questions,
    });
    state.broadcast(&created_event.to_string());
    if let Some(hook) = event_hook.as_ref() {
        hook(created_event);
    }

    let wait_result = tokio::time::timeout(Duration::from_secs(300), rx).await;

    state.runtime_control.drop_question(&question_info.id).await;

    match wait_result {
        Ok(Ok(QuestionReply::Answers(answers))) => {
            if let Some(hook) = event_hook.as_ref() {
                hook(serde_json::json!({
                    "type": "question.replied",
                    "requestID": question_info.id,
                    "sessionID": question_info.session_id,
                    "answers": answers,
                }));
            }
            Ok(answers)
        }
        Ok(Ok(QuestionReply::Rejected)) => {
            if let Some(hook) = event_hook.as_ref() {
                hook(serde_json::json!({
                    "type": "question.rejected",
                    "requestID": question_info.id,
                    "sessionID": question_info.session_id,
                }));
            }
            Err(rocode_tool::ToolError::QuestionRejected(
                "User rejected question request".to_string(),
            ))
        }
        Ok(Ok(QuestionReply::Cancelled)) => {
            if let Some(hook) = event_hook.as_ref() {
                hook(serde_json::json!({
                    "type": "question.rejected",
                    "requestID": question_info.id,
                    "sessionID": question_info.session_id,
                    "reason": "cancelled",
                }));
            }
            Err(rocode_tool::ToolError::Cancelled)
        }
        Ok(Err(_)) => Err(rocode_tool::ToolError::ExecutionError(
            "Question response channel closed".to_string(),
        )),
        Err(_) => Err(rocode_tool::ToolError::ExecutionError(
            "Timed out waiting for question response".to_string(),
        )),
    }
}

pub(crate) async fn cancel_questions_for_session(
    state: Arc<ServerState>,
    session_id: &str,
) -> usize {
    let cancelled = state
        .runtime_control
        .cancel_questions_for_session(session_id)
        .await;
    if cancelled.is_empty() {
        return 0;
    }

    for question in &cancelled {
        state.broadcast(
            &serde_json::json!({
                "type": "question.rejected",
                "requestID": question.id,
                "sessionID": question.session_id,
                "reason": "cancelled",
            })
            .to_string(),
        );
    }

    cancelled.len()
}

async fn list_questions(State(state): State<Arc<ServerState>>) -> Json<Vec<QuestionInfo>> {
    Json(state.runtime_control.list_questions().await)
}

pub(crate) async fn list_questions_for_session(
    state: &Arc<ServerState>,
    session_id: &str,
) -> Vec<QuestionInfo> {
    state
        .runtime_control
        .list_questions_for_session(session_id)
        .await
}

#[derive(Debug, Deserialize)]
pub struct ReplyQuestionRequest {
    pub answers: Vec<Vec<String>>,
}

async fn reply_question(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ReplyQuestionRequest>,
) -> Result<Json<bool>> {
    let question = state
        .runtime_control
        .answer_question(&id, req.answers.clone())
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Question request not found: {}", id)))?;

    state.broadcast(
        &serde_json::json!({
            "type": "question.replied",
            "requestID": id,
            "sessionID": question.session_id,
            "answers": req.answers,
        })
        .to_string(),
    );
    Ok(Json(true))
}

async fn reject_question(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<bool>> {
    let question = state
        .runtime_control
        .reject_question(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Question request not found: {}", id)))?;

    state.broadcast(
        &serde_json::json!({
            "type": "question.rejected",
            "requestID": id,
            "sessionID": question.session_id,
        })
        .to_string(),
    );
    Ok(Json(true))
}

/// TUI communication routes.
///
/// Architecture note: In the TypeScript version the TUI (Ink/React) runs as a
/// separate process and communicates with the backend exclusively over HTTP.
/// The Rust TUI (`opencode-tui`) uses ratatui/crossterm and runs in its own
/// binary, but still talks to the server through an HTTP `ApiClient`.  These
/// endpoints therefore remain necessary -- they bridge external TUI requests
/// into an internal queue that the TUI polls via `/control/next` and answers
/// via `/control/response`.
///
/// The `/set-prompt` endpoint is a Rust-only addition (not present in the TS
/// codebase) that allows overwriting the prompt text rather than appending.

#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    pub text: String,
}

static TUI_REQUEST_QUEUE: Lazy<Mutex<VecDeque<TuiRequest>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static TUI_RESPONSE_QUEUE: Lazy<Mutex<VecDeque<serde_json::Value>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static TUI_REQUEST_NOTIFY: Lazy<Notify> = Lazy::new(Notify::new);
static TUI_RESPONSE_NOTIFY: Lazy<Notify> = Lazy::new(Notify::new);

async fn enqueue_tui_request(state: &Arc<ServerState>, path: &str, body: serde_json::Value) {
    let mut queue = TUI_REQUEST_QUEUE.lock().await;
    queue.push_back(TuiRequest {
        path: path.to_string(),
        body: body.clone(),
    });
    drop(queue);
    TUI_REQUEST_NOTIFY.notify_one();

    state.broadcast(
        &serde_json::json!({
            "type": "tui.request",
            "path": path,
            "body": body,
        })
        .to_string(),
    );
}

async fn append_prompt(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PromptRequest>,
) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/append-prompt",
        serde_json::json!({ "text": req.text }),
    )
    .await;
    Ok(Json(true))
}

async fn set_prompt(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PromptRequest>,
) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/set-prompt",
        serde_json::json!({ "text": req.text }),
    )
    .await;
    Ok(Json(true))
}

async fn submit_prompt(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PromptRequest>,
) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/submit-prompt",
        serde_json::json!({ "text": req.text }),
    )
    .await;
    Ok(Json(true))
}

async fn clear_prompt(State(state): State<Arc<ServerState>>) -> Result<Json<bool>> {
    enqueue_tui_request(&state, "/tui/clear-prompt", serde_json::json!({})).await;
    Ok(Json(true))
}

async fn open_help(State(state): State<Arc<ServerState>>) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/open-help",
        serde_json::json!({ "command": "help.show" }),
    )
    .await;
    Ok(Json(true))
}

async fn open_sessions(State(state): State<Arc<ServerState>>) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/open-sessions",
        serde_json::json!({ "command": "session.list" }),
    )
    .await;
    Ok(Json(true))
}

async fn open_themes(State(state): State<Arc<ServerState>>) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/open-themes",
        serde_json::json!({ "command": "theme.list" }),
    )
    .await;
    Ok(Json(true))
}

async fn open_models(State(state): State<Arc<ServerState>>) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/open-models",
        serde_json::json!({ "command": "model.list" }),
    )
    .await;
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct TuiCommandRequest {
    pub command: String,
    pub arguments: Option<serde_json::Value>,
}

fn map_tui_command(command: &str) -> &str {
    match command {
        "session_new" => "session.new",
        "session_share" => "session.share",
        "session_interrupt" => "session.interrupt",
        "session_compact" => "session.compact",
        "messages_page_up" => "session.page.up",
        "messages_page_down" => "session.page.down",
        "messages_line_up" => "session.line.up",
        "messages_line_down" => "session.line.down",
        "messages_half_page_up" => "session.half.page.up",
        "messages_half_page_down" => "session.half.page.down",
        "messages_first" => "session.first",
        "messages_last" => "session.last",
        "agent_cycle" => "agent.cycle",
        other => other,
    }
}

async fn execute_tui_command(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<TuiCommandRequest>,
) -> Result<Json<bool>> {
    let mapped = map_tui_command(&req.command);
    enqueue_tui_request(
        &state,
        "/tui/execute-command",
        serde_json::json!({
            "command": mapped,
            "arguments": req.arguments,
        }),
    )
    .await;
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct ToastRequest {
    pub message: String,
    pub level: Option<String>,
}

async fn show_toast(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ToastRequest>,
) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/show-toast",
        serde_json::json!({
            "message": req.message,
            "level": req.level,
        }),
    )
    .await;
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct PublishEventRequest {
    pub event: String,
    pub data: Option<serde_json::Value>,
}

async fn publish_tui_event(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PublishEventRequest>,
) -> Result<Json<bool>> {
    enqueue_tui_request(
        &state,
        "/tui/publish",
        serde_json::json!({
            "event": req.event,
            "data": req.data,
        }),
    )
    .await;
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct SelectSessionRequest {
    pub session_id: String,
}

async fn select_session(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<SelectSessionRequest>,
) -> Result<Json<bool>> {
    let sessions = state.sessions.lock().await;
    if sessions.get(&req.session_id).is_none() {
        return Err(ApiError::SessionNotFound(req.session_id));
    }
    drop(sessions);

    enqueue_tui_request(
        &state,
        "/tui/select-session",
        serde_json::json!({ "sessionID": req.session_id }),
    )
    .await;
    Ok(Json(true))
}

#[derive(Debug, Serialize)]
pub struct TuiRequest {
    pub path: String,
    pub body: serde_json::Value,
}

async fn get_next_tui_request() -> Json<Option<TuiRequest>> {
    loop {
        let mut queue = TUI_REQUEST_QUEUE.lock().await;
        if let Some(next) = queue.pop_front() {
            return Json(Some(next));
        }
        drop(queue);
        TUI_REQUEST_NOTIFY.notified().await;
    }
}

async fn submit_tui_response(Json(body): Json<serde_json::Value>) -> Json<bool> {
    let mut queue = TUI_RESPONSE_QUEUE.lock().await;
    queue.push_back(body);
    drop(queue);
    TUI_RESPONSE_NOTIFY.notify_one();
    Json(true)
}
