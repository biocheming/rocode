//! SSE (Server-Sent Events) subscription for the CLI.
//!
//! Connects to the server's `/event` endpoint and forwards parsed events
//! to the CLI's REPL loop via an unbounded channel.  Auto-reconnects on
//! disconnect with exponential backoff (capped at 5 s).

use std::time::Duration;

use tokio::sync::mpsc;

use crate::util::server_url;
use rocode_core::contracts::events::{ServerEventType, SessionRunStatusType, ToolCallPhase};
use rocode_core::contracts::wire::{
    fields as wire_fields, keys as wire_keys, keysets as wire_keysets, selectors as wire_selectors,
};

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
        payload: serde_json::Value,
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
    // Helper to extract session_id from various field names.
    let event_session_id =
        wire_selectors::first_str(json, wire_keysets::SESSION_ID_ANY).unwrap_or("");

    // Determine event type from SSE event name or payload's "type" field.
    let event_type = if !event_name.is_empty() {
        event_name.to_string()
    } else {
        json.get(wire_keys::TYPE)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let parsed_event_type = ServerEventType::parse(&event_type);

    // Global events (no session filter).
    if parsed_event_type == Some(ServerEventType::ConfigUpdated) {
        return Some(CliServerEvent::ConfigUpdated);
    }

    // Session-scoped events — filter by session_id.
    let is_my_session = event_session_id.is_empty() || event_session_id == my_session_id;

    match parsed_event_type {
        Some(ServerEventType::SessionUpdated) => {
            if !is_my_session {
                return None;
            }
            let source = json
                .get(wire_fields::SOURCE)
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(CliServerEvent::SessionUpdated {
                session_id: event_session_id.to_string(),
                source,
            })
        }
        Some(ServerEventType::SessionStatus) => {
            if !is_my_session {
                return None;
            }
            // Support both plain string "idle" and tagged object {"type": "idle"}
            // formats. The server serializes SessionRunStatus with
            // #[serde(tag = "type")] so the value is an object, but we also
            // accept a plain string for forward compatibility.
            let status = json
                .get(wire_fields::STATUS)
                .and_then(|v| {
                    v.as_str()
                        .or_else(|| v.get(wire_keys::TYPE).and_then(|t| t.as_str()))
                })
                .unwrap_or("");
            match SessionRunStatusType::parse(status) {
                Some(SessionRunStatusType::Busy) => Some(CliServerEvent::SessionBusy {
                    session_id: event_session_id.to_string(),
                }),
                Some(SessionRunStatusType::Idle) => Some(CliServerEvent::SessionIdle {
                    session_id: event_session_id.to_string(),
                }),
                Some(SessionRunStatusType::Retry) => Some(CliServerEvent::SessionRetrying {
                    session_id: event_session_id.to_string(),
                }),
                _ => None,
            }
        }
        Some(ServerEventType::QuestionCreated) => {
            // Questions may come from child/subsessions — always handle them
            // so the CLI user can answer regardless of which session asked.
            let request_id = wire_selectors::first_str(json, wire_keysets::REQUEST_ID_ANY)
                .unwrap_or("")
                .to_string();
            let questions_json = json
                .get(wire_fields::QUESTIONS)
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![]));
            Some(CliServerEvent::QuestionCreated {
                request_id,
                session_id: event_session_id.to_string(),
                questions_json,
            })
        }
        Some(ServerEventType::QuestionResolved) => {
            let request_id = wire_selectors::first_str(json, wire_keysets::REQUEST_ID_ANY)
                .unwrap_or("")
                .to_string();
            Some(CliServerEvent::QuestionResolved { request_id })
        }
        Some(ServerEventType::PermissionRequested) => {
            let permission_id = wire_selectors::first_str(json, wire_keysets::PERMISSION_ID_ANY)
                .unwrap_or("")
                .to_string();
            let info_json = json
                .get(wire_fields::INFO)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Some(CliServerEvent::PermissionRequested {
                session_id: event_session_id.to_string(),
                permission_id,
                info_json,
            })
        }
        Some(ServerEventType::PermissionResolved) => {
            let permission_id = wire_selectors::first_str(json, wire_keysets::PERMISSION_ID_ANY)
                .unwrap_or("")
                .to_string();
            Some(CliServerEvent::PermissionResolved { permission_id })
        }
        Some(ServerEventType::ToolCallLifecycle) => {
            let tool_call_id = wire_selectors::first_str(json, wire_keysets::TOOL_CALL_ID_ANY)
                .unwrap_or("")
                .to_string();
            match ToolCallPhase::parse(
                json.get(wire_fields::PHASE)
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            ) {
                Some(ToolCallPhase::Start) => {
                    let tool_name = wire_selectors::first_str(
                        json,
                        &[wire_fields::TOOL_NAME, wire_fields::TOOL_NAME_SNAKE],
                    )
                    .unwrap_or("")
                    .to_string();
                    Some(CliServerEvent::ToolCallStarted {
                        session_id: event_session_id.to_string(),
                        tool_call_id,
                        tool_name,
                    })
                }
                Some(ToolCallPhase::Complete) => Some(CliServerEvent::ToolCallCompleted {
                    session_id: event_session_id.to_string(),
                    tool_call_id,
                }),
                _ => None,
            }
        }
        Some(ServerEventType::ToolCallStart) => {
            let tool_call_id = wire_selectors::first_str(json, wire_keysets::TOOL_CALL_ID_ANY)
                .unwrap_or("")
                .to_string();
            let tool_name = wire_selectors::first_str(
                json,
                &[wire_fields::TOOL_NAME, wire_fields::TOOL_NAME_SNAKE],
            )
            .unwrap_or("")
            .to_string();
            Some(CliServerEvent::ToolCallStarted {
                session_id: event_session_id.to_string(),
                tool_call_id,
                tool_name,
            })
        }
        Some(ServerEventType::ToolCallComplete) => {
            let tool_call_id = wire_selectors::first_str(json, wire_keysets::TOOL_CALL_ID_ANY)
                .unwrap_or("")
                .to_string();
            Some(CliServerEvent::ToolCallCompleted {
                session_id: event_session_id.to_string(),
                tool_call_id,
            })
        }
        Some(ServerEventType::ChildSessionAttached) => {
            let parent_id = wire_selectors::first_str(json, wire_keysets::PARENT_ID_ANY)
                .unwrap_or("")
                .to_string();
            let child_id = wire_selectors::first_str(json, wire_keysets::CHILD_ID_ANY)
                .unwrap_or("")
                .to_string();
            Some(CliServerEvent::ChildSessionAttached {
                parent_id,
                child_id,
            })
        }
        Some(ServerEventType::ChildSessionDetached) => {
            let parent_id = wire_selectors::first_str(json, wire_keysets::PARENT_ID_ANY)
                .unwrap_or("")
                .to_string();
            let child_id = wire_selectors::first_str(json, wire_keysets::CHILD_ID_ANY)
                .unwrap_or("")
                .to_string();
            Some(CliServerEvent::ChildSessionDetached {
                parent_id,
                child_id,
            })
        }
        Some(ServerEventType::OutputBlock) => {
            // Output blocks may or may not carry a session_id.
            let id = json
                .get(wire_fields::ID)
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(CliServerEvent::OutputBlock {
                session_id: event_session_id.to_string(),
                id,
                payload: json.clone(),
            })
        }
        Some(ServerEventType::Error) => {
            let error = json
                .get(wire_keys::ERROR)
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            let message_id =
                wire_selectors::first_str(json, wire_keysets::MESSAGE_ID_ANY).map(String::from);
            let done = json.get(wire_fields::DONE).and_then(|v| v.as_bool());
            Some(CliServerEvent::Error {
                session_id: event_session_id.to_string(),
                error,
                message_id,
                done,
            })
        }
        Some(ServerEventType::Usage) => {
            let prompt_tokens = json
                .get(wire_fields::PROMPT_TOKENS)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let completion_tokens = json
                .get(wire_fields::COMPLETION_TOKENS)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let message_id =
                wire_selectors::first_str(json, wire_keysets::MESSAGE_ID_ANY).map(String::from);
            Some(CliServerEvent::Usage {
                session_id: event_session_id.to_string(),
                prompt_tokens,
                completion_tokens,
                message_id,
            })
        }
        _ => {
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
    use rocode_core::contracts::events::{
        QuestionResolutionKind, ServerEventType, SessionRunStatusType, ToolCallPhase,
    };
    use rocode_core::contracts::output_blocks::{
        MessagePhaseWire, MessageRoleWire, OutputBlockKind,
    };
    use rocode_core::contracts::tools::BuiltinToolName;

    #[test]
    fn output_block_is_filtered_to_current_session() {
        let mine = serde_json::json!({
            "type": ServerEventType::OutputBlock.as_str(),
            "sessionID": "session-1",
            "block": {
                "kind": OutputBlockKind::Message.as_str(),
                "phase": MessagePhaseWire::Delta.as_str(),
                "role": MessageRoleWire::Assistant.as_str(),
                "text": "hi"
            }
        });
        let other = serde_json::json!({
            "type": ServerEventType::OutputBlock.as_str(),
            "sessionID": "session-2",
            "block": {
                "kind": OutputBlockKind::Message.as_str(),
                "phase": MessagePhaseWire::Delta.as_str(),
                "role": MessageRoleWire::Assistant.as_str(),
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
            "type": ServerEventType::ConfigUpdated.as_str()
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(event, Some(CliServerEvent::ConfigUpdated)));
    }

    #[test]
    fn question_created_from_child_session_is_not_filtered() {
        let payload = serde_json::json!({
            "type": ServerEventType::QuestionCreated.as_str(),
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
            "type": ServerEventType::ChildSessionAttached.as_str(),
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
            "type": ServerEventType::QuestionResolved.as_str(),
            "sessionID": "session-1",
            "requestID": "question-1",
            "resolution": QuestionResolutionKind::Answered.as_str(),
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
            "type": ServerEventType::ToolCallLifecycle.as_str(),
            "sessionID": "session-1",
            "toolCallId": "tool-1",
            "phase": ToolCallPhase::Complete.as_str(),
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
            "type": ServerEventType::PermissionRequested.as_str(),
            "sessionID": "session-1",
            "permissionID": "permission-1",
            "info": {
                "id": "permission-1",
                "session_id": "session-1",
                "tool": BuiltinToolName::Bash.as_str(),
                "input": {"permission": BuiltinToolName::Bash.as_str(), "patterns": ["cargo test"]},
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
            "type": ServerEventType::SessionStatus.as_str(),
            "sessionID": "session-1",
            "status": {"type": SessionRunStatusType::Idle.as_str()}
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
            "type": ServerEventType::SessionStatus.as_str(),
            "sessionID": "session-1",
            "status": {"type": SessionRunStatusType::Busy.as_str()}
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
            "type": ServerEventType::SessionStatus.as_str(),
            "sessionID": "session-1",
            "status": SessionRunStatusType::Idle.as_str()
        });

        let event = parse_event("", &payload, "session-1");

        assert!(matches!(
            event,
            Some(CliServerEvent::SessionIdle { session_id }) if session_id == "session-1"
        ));
    }
}
