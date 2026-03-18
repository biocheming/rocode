//! SSE (Server-Sent Events) subscription for the CLI.
//!
//! Connects to the server's `/event` endpoint and forwards parsed events
//! to the CLI's REPL loop via an unbounded channel.  Auto-reconnects on
//! disconnect with exponential backoff (capped at 5 s).

use std::time::Duration;

use serde::Deserialize;
use tokio::sync::mpsc;

use crate::util::server_url;

// ── Event types ──────────────────────────────────────────────────────

/// Events the CLI cares about, parsed from the SSE stream.
#[derive(Debug, Clone)]
pub enum CliServerEvent {
    /// Global config changed — refetch `/config` for updated preferences.
    ConfigUpdated,
    /// Session state changed — fetch latest session to diff messages.
    SessionUpdated {
        session_id: String,
        source: Option<String>,
    },
    /// Session became busy (prompt running).
    SessionBusy { session_id: String },
    /// Session became idle (prompt finished).
    SessionIdle { session_id: String },
    /// Session is retrying.
    SessionRetrying { session_id: String },
    /// A question was created and needs user interaction.
    QuestionCreated {
        request_id: String,
        session_id: String,
        /// Raw question definitions from the server (Vec<QuestionDef> JSON).
        questions_json: serde_json::Value,
    },
    /// A question was resolved (answered, rejected, or cancelled).
    QuestionResolved { request_id: String },
    /// A permission request was created and needs user interaction.
    PermissionRequested {
        session_id: String,
        permission_id: String,
        info_json: serde_json::Value,
    },
    /// A permission request was resolved.
    PermissionResolved { permission_id: String },
    /// A tool call started.
    ToolCallStarted {
        session_id: String,
        tool_call_id: String,
        tool_name: String,
    },
    /// A tool call completed.
    ToolCallCompleted {
        session_id: String,
        tool_call_id: String,
    },
    /// A child session was attached under a parent session in the active tree.
    ChildSessionAttached { parent_id: String, child_id: String },
    /// A child session was detached from a parent session in the active tree.
    ChildSessionDetached { parent_id: String, child_id: String },
    /// An output block was emitted (message, tool result, etc.).
    OutputBlock {
        session_id: String,
        id: Option<String>,
        block_json: serde_json::Value,
    },
    /// An error event from the server.
    Error {
        session_id: String,
        error: String,
        message_id: Option<String>,
        done: Option<bool>,
    },
    /// Token usage update.
    Usage {
        session_id: String,
        prompt_tokens: u64,
        completion_tokens: u64,
        message_id: Option<String>,
    },
    /// Unknown event type (logged but ignored).
    Unknown {
        event: String,
        data: serde_json::Value,
    },
}

// ── Wire parsing (serde) ──────────────────────────────────────────────

fn default_empty_array() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        _ => None,
    })
}

fn deserialize_u64_lossy<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(value)) => value.as_u64().unwrap_or(0),
        Some(serde_json::Value::String(raw)) => raw.parse::<u64>().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Deserialize, Default)]
struct TypeWire {
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ServerEventWire {
    #[serde(rename = "config.updated")]
    ConfigUpdated,
    #[serde(rename = "session.updated")]
    SessionUpdated {
        #[serde(
            rename = "sessionID",
            alias = "sessionId",
            alias = "session_id",
            default
        )]
        session_id: String,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        source: Option<String>,
    },
    #[serde(rename = "session.status")]
    SessionStatus {
        #[serde(
            rename = "sessionID",
            alias = "sessionId",
            alias = "session_id",
            default
        )]
        session_id: String,
        status: SessionStatusWire,
    },
    #[serde(rename = "question.created")]
    QuestionCreated(QuestionCreatedWire),
    #[serde(rename = "question.resolved")]
    QuestionResolved(QuestionResolvedWire),
    #[serde(rename = "question.replied")]
    QuestionReplied(QuestionResolvedWire),
    #[serde(rename = "question.rejected")]
    QuestionRejected(QuestionResolvedWire),
    #[serde(rename = "permission.requested")]
    PermissionRequested(PermissionRequestedWire),
    #[serde(rename = "permission.resolved")]
    PermissionResolved(PermissionResolvedWire),
    #[serde(rename = "permission.replied")]
    PermissionReplied(PermissionResolvedWire),
    #[serde(rename = "tool_call.lifecycle")]
    ToolCallLifecycle(ToolCallLifecycleWire),
    #[serde(rename = "tool_call.start")]
    ToolCallStart(ToolCallStartWire),
    #[serde(rename = "tool_call.complete")]
    ToolCallComplete(ToolCallCompleteWire),
    #[serde(rename = "child_session.attached")]
    ChildSessionAttached(ChildSessionWire),
    #[serde(rename = "child_session.detached")]
    ChildSessionDetached(ChildSessionWire),
    #[serde(rename = "output_block")]
    OutputBlock(OutputBlockWire),
    #[serde(rename = "error")]
    Error(ErrorWire),
    #[serde(rename = "usage")]
    Usage(UsageWire),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct QuestionCreatedWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(
        rename = "requestID",
        alias = "requestId",
        alias = "request_id",
        alias = "id",
        default
    )]
    request_id: String,
    #[serde(default = "default_empty_array")]
    questions: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct QuestionResolvedWire {
    #[serde(
        rename = "requestID",
        alias = "requestId",
        alias = "request_id",
        alias = "id",
        default
    )]
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct PermissionRequestedWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(
        rename = "permissionID",
        alias = "permissionId",
        alias = "requestID",
        alias = "requestId",
        alias = "id",
        default
    )]
    permission_id: String,
    #[serde(default)]
    info: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PermissionResolvedWire {
    #[serde(
        rename = "permissionID",
        alias = "permissionId",
        alias = "requestID",
        alias = "requestId",
        alias = "id",
        default
    )]
    permission_id: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallLifecycleWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(rename = "toolCallId", alias = "tool_call_id", default)]
    tool_call_id: String,
    #[serde(default)]
    phase: String,
    #[serde(rename = "toolName", alias = "tool_name", default)]
    tool_name: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallStartWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(rename = "toolCallId", alias = "tool_call_id", default)]
    tool_call_id: String,
    #[serde(rename = "toolName", alias = "tool_name", default)]
    tool_name: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallCompleteWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(rename = "toolCallId", alias = "tool_call_id", default)]
    tool_call_id: String,
}

#[derive(Debug, Deserialize)]
struct ChildSessionWire {
    #[serde(rename = "parentID", alias = "parentId", alias = "parent_id", default)]
    parent_id: String,
    #[serde(rename = "childID", alias = "childId", alias = "child_id", default)]
    child_id: String,
}

#[derive(Debug, Deserialize)]
struct OutputBlockWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    id: Option<String>,
    #[serde(default)]
    block: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ErrorWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    error: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    message: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    message_id: Option<String>,
    #[serde(default)]
    done: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UsageWire {
    #[serde(
        rename = "sessionID",
        alias = "sessionId",
        alias = "session_id",
        default
    )]
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_u64_lossy")]
    prompt_tokens: u64,
    #[serde(default, deserialize_with = "deserialize_u64_lossy")]
    completion_tokens: u64,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    message_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SessionStatusWire {
    Simple(String),
    Object(SessionStatusObjectWire),
}

impl SessionStatusWire {
    fn kind(&self) -> Option<&str> {
        match self {
            SessionStatusWire::Simple(kind) => Some(kind.as_str()),
            SessionStatusWire::Object(obj) => obj.kind.as_deref(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct SessionStatusObjectWire {
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    kind: Option<String>,
}

// ── SSE subscriber ───────────────────────────────────────────────────

/// Spawn a background task that subscribes to the server's SSE stream
/// and forwards parsed events to `tx`.
///
/// The task runs until `tx` is closed (all receivers dropped) or the
/// cancellation token is cancelled.
pub fn spawn_sse_subscriber(
    base_url: String,
    session_id: String,
    tx: mpsc::UnboundedSender<CliServerEvent>,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_millis(400);
        let max_backoff = Duration::from_secs(5);

        loop {
            if cancel.is_cancelled() {
                break;
            }

            match connect_and_consume(&base_url, &session_id, &tx, &cancel).await {
                Ok(()) => {
                    // Clean shutdown (cancel requested).
                    break;
                }
                Err(e) => {
                    if cancel.is_cancelled() {
                        break;
                    }
                    tracing::warn!("SSE connection lost: {}. Reconnecting in {:?}…", e, backoff);
                    tokio::select! {
                        _ = tokio::time::sleep(backoff) => {}
                        _ = cancel.cancelled() => break,
                    }
                    backoff = (backoff * 2).min(max_backoff);
                }
            }
        }

        tracing::debug!("SSE subscriber exiting");
    })
}

/// Connect to the SSE endpoint and consume events until disconnect.
async fn connect_and_consume(
    base_url: &str,
    session_id: &str,
    tx: &mpsc::UnboundedSender<CliServerEvent>,
    cancel: &tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    // Subscribe with server-side session filter so we only receive events
    // relevant to our session (plus global events like config.updated).
    // This replaces most client-side is_my_session filtering, though we
    // keep the client-side checks as a defense-in-depth measure.
    let url = format!("{}?session={}", server_url(base_url, "/event"), session_id,);
    let client = reqwest::Client::new();

    let resp = client
        .get(&url)
        .header("Accept", "text/event-stream")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("SSE connect failed: {}", resp.status());
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut current_event = String::new();
    let mut current_data = String::new();

    use futures::StreamExt;

    loop {
        tokio::select! {
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Process complete SSE frames (separated by blank lines).
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            current_event.clear();
                            current_data.clear();

                            for line in frame.lines() {
                                if let Some(value) = line.strip_prefix("event: ") {
                                    current_event = value.trim().to_string();
                                } else if let Some(value) = line.strip_prefix("data: ") {
                                    if !current_data.is_empty() {
                                        current_data.push('\n');
                                    }
                                    current_data.push_str(value);
                                } else if line.starts_with("data:") {
                                    // data: with no space — value is empty
                                    if !current_data.is_empty() {
                                        current_data.push('\n');
                                    }
                                }
                            }

                            if !current_data.is_empty() {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&current_data) {
                                    if let Some(evt) = parse_event(&current_event, &json, session_id) {
                                        if tx.send(evt).is_err() {
                                            // Receiver dropped.
                                            return Ok(());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Err(e.into());
                    }
                    None => {
                        // Stream ended.
                        anyhow::bail!("SSE stream ended");
                    }
                }
            }
            _ = cancel.cancelled() => {
                return Ok(());
            }
        }
    }
}

/// Parse an SSE event JSON payload into a `CliServerEvent`.
///
/// Returns `None` for events that belong to other sessions (session
/// filtering) or events we don't care about.
fn parse_event(
    event_name: &str,
    json: &serde_json::Value,
    my_session_id: &str,
) -> Option<CliServerEvent> {
    let event_type = if !event_name.is_empty() {
        event_name.to_string()
    } else {
        serde_json::from_value::<TypeWire>(json.clone())
            .unwrap_or_default()
            .kind
    };

    let mut value = json.clone();
    if !event_name.is_empty() && !event_type.is_empty() {
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "type".to_string(),
                serde_json::Value::String(event_type.clone()),
            );
        }
    }

    let parsed =
        serde_json::from_value::<ServerEventWire>(value).unwrap_or(ServerEventWire::Unknown);

    match parsed {
        ServerEventWire::ConfigUpdated => Some(CliServerEvent::ConfigUpdated),
        ServerEventWire::SessionUpdated { session_id, source } => {
            if !session_id.is_empty() && session_id != my_session_id {
                return None;
            }
            Some(CliServerEvent::SessionUpdated { session_id, source })
        }
        ServerEventWire::SessionStatus { session_id, status } => {
            if !session_id.is_empty() && session_id != my_session_id {
                return None;
            }
            match status.kind().unwrap_or("") {
                "busy" => Some(CliServerEvent::SessionBusy { session_id }),
                "idle" => Some(CliServerEvent::SessionIdle { session_id }),
                "retry" => Some(CliServerEvent::SessionRetrying { session_id }),
                _ => None,
            }
        }
        ServerEventWire::QuestionCreated(event) => Some(CliServerEvent::QuestionCreated {
            request_id: event.request_id,
            session_id: event.session_id,
            questions_json: event.questions,
        }),
        ServerEventWire::QuestionResolved(event)
        | ServerEventWire::QuestionReplied(event)
        | ServerEventWire::QuestionRejected(event) => Some(CliServerEvent::QuestionResolved {
            request_id: event.request_id,
        }),
        ServerEventWire::PermissionRequested(event) => Some(CliServerEvent::PermissionRequested {
            session_id: event.session_id,
            permission_id: event.permission_id,
            info_json: event.info,
        }),
        ServerEventWire::PermissionResolved(event) | ServerEventWire::PermissionReplied(event) => {
            Some(CliServerEvent::PermissionResolved {
                permission_id: event.permission_id,
            })
        }
        ServerEventWire::ToolCallLifecycle(event) => match event.phase.as_str() {
            "start" => Some(CliServerEvent::ToolCallStarted {
                session_id: event.session_id,
                tool_call_id: event.tool_call_id,
                tool_name: event.tool_name,
            }),
            "complete" => Some(CliServerEvent::ToolCallCompleted {
                session_id: event.session_id,
                tool_call_id: event.tool_call_id,
            }),
            _ => None,
        },
        ServerEventWire::ToolCallStart(event) => Some(CliServerEvent::ToolCallStarted {
            session_id: event.session_id,
            tool_call_id: event.tool_call_id,
            tool_name: event.tool_name,
        }),
        ServerEventWire::ToolCallComplete(event) => Some(CliServerEvent::ToolCallCompleted {
            session_id: event.session_id,
            tool_call_id: event.tool_call_id,
        }),
        ServerEventWire::ChildSessionAttached(event) => {
            Some(CliServerEvent::ChildSessionAttached {
                parent_id: event.parent_id,
                child_id: event.child_id,
            })
        }
        ServerEventWire::ChildSessionDetached(event) => {
            Some(CliServerEvent::ChildSessionDetached {
                parent_id: event.parent_id,
                child_id: event.child_id,
            })
        }
        ServerEventWire::OutputBlock(event) => Some(CliServerEvent::OutputBlock {
            session_id: event.session_id,
            id: event.id,
            block_json: event.block.unwrap_or_else(|| json.clone()),
        }),
        ServerEventWire::Error(event) => Some(CliServerEvent::Error {
            session_id: event.session_id,
            error: event
                .error
                .or(event.message)
                .unwrap_or_else(|| "unknown error".to_string()),
            message_id: event.message_id,
            done: event.done,
        }),
        ServerEventWire::Usage(event) => Some(CliServerEvent::Usage {
            session_id: event.session_id,
            prompt_tokens: event.prompt_tokens,
            completion_tokens: event.completion_tokens,
            message_id: event.message_id,
        }),
        ServerEventWire::Unknown => {
            tracing::trace!("Unknown SSE event: {}", event_type);
            Some(CliServerEvent::Unknown {
                event: event_type,
                data: json.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_event, CliServerEvent};

    #[test]
    fn output_block_is_filtered_to_current_session() {
        let mine = serde_json::json!({
            "type": "output_block",
            "sessionID": "session-1",
            "block": {
                "kind": "message",
                "phase": "delta",
                "role": "assistant",
                "text": "hi"
            }
        });
        let other = serde_json::json!({
            "type": "output_block",
            "sessionID": "session-2",
            "block": {
                "kind": "message",
                "phase": "delta",
                "role": "assistant",
                "text": "nope"
            }
        });

        let mine_event = parse_event("", &mine, "session-1");
        let other_event = parse_event("", &other, "session-1");

        assert!(matches!(
            mine_event,
            Some(CliServerEvent::OutputBlock { session_id, .. }) if session_id == "session-1"
        ));
        assert!(matches!(
            other_event,
            Some(CliServerEvent::OutputBlock { session_id, .. }) if session_id == "session-2"
        ));
    }

    #[test]
    fn config_updated_is_parsed_as_global_event() {
        let payload = serde_json::json!({
            "type": "config.updated"
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(event, Some(CliServerEvent::ConfigUpdated)));
    }

    #[test]
    fn question_created_from_child_session_is_not_filtered() {
        let payload = serde_json::json!({
            "type": "question.created",
            "sessionID": "child-session-1",
            "requestID": "question-1",
            "questions": [{
                "header": "Scope",
                "question": "Proceed?",
                "options": [{"label": "Yes"}]
            }]
        });

        let event = parse_event("", &payload, "parent-session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::QuestionCreated { session_id, request_id, .. })
                if session_id == "child-session-1" && request_id == "question-1"
        ));
    }

    #[test]
    fn child_session_attach_event_is_parsed() {
        let payload = serde_json::json!({
            "type": "child_session.attached",
            "parentID": "parent-session-1",
            "childID": "child-session-1"
        });

        let event = parse_event("", &payload, "parent-session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::ChildSessionAttached { parent_id, child_id })
                if parent_id == "parent-session-1" && child_id == "child-session-1"
        ));
    }

    #[test]
    fn canonical_question_resolved_event_is_parsed() {
        let payload = serde_json::json!({
            "type": "question.resolved",
            "sessionID": "session-1",
            "requestID": "question-1",
            "resolution": "answered",
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::QuestionResolved { request_id }) if request_id == "question-1"
        ));
    }

    #[test]
    fn canonical_tool_call_lifecycle_complete_event_is_parsed() {
        let payload = serde_json::json!({
            "type": "tool_call.lifecycle",
            "sessionID": "session-1",
            "toolCallId": "tool-1",
            "phase": "complete",
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::ToolCallCompleted { session_id, tool_call_id })
                if session_id == "session-1" && tool_call_id == "tool-1"
        ));
    }

    #[test]
    fn canonical_permission_requested_event_is_parsed() {
        let payload = serde_json::json!({
            "type": "permission.requested",
            "sessionID": "session-1",
            "permissionID": "permission-1",
            "info": {
                "id": "permission-1",
                "session_id": "session-1",
                "tool": "bash",
                "input": {"permission": "bash", "patterns": ["cargo test"]},
                "message": "Permission required"
            }
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::PermissionRequested { session_id, permission_id, .. })
                if session_id == "session-1" && permission_id == "permission-1"
        ));
    }

    #[test]
    fn session_status_idle_tagged_object_is_parsed() {
        // The server serializes SessionRunStatus with #[serde(tag = "type")]
        // so idle becomes {"type": "idle"} rather than a plain string.
        let payload = serde_json::json!({
            "type": "session.status",
            "sessionID": "session-1",
            "status": {"type": "idle"}
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::SessionIdle { session_id }) if session_id == "session-1"
        ));
    }

    #[test]
    fn session_status_busy_tagged_object_is_parsed() {
        let payload = serde_json::json!({
            "type": "session.status",
            "sessionID": "session-1",
            "status": {"type": "busy"}
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::SessionBusy { session_id }) if session_id == "session-1"
        ));
    }

    #[test]
    fn session_status_plain_string_is_parsed() {
        // Forward-compatible: also accept plain string format.
        let payload = serde_json::json!({
            "type": "session.status",
            "sessionID": "session-1",
            "status": "idle"
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::SessionIdle { session_id }) if session_id == "session-1"
        ));
    }
}
