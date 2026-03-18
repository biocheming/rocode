use super::*;
use serde::Deserialize;
use std::sync::{Arc, Mutex as StdMutex};

pub(super) fn env_var_enabled(name: &str) -> bool {
    let Ok(value) = std::env::var(name) else {
        return false;
    };
    let normalized = value.trim().to_ascii_lowercase();
    !normalized.is_empty() && !matches!(normalized.as_str(), "0" | "false" | "no" | "off")
}

pub(super) fn env_var_with_fallback(primary: &str, fallback: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .or_else(|| std::env::var(fallback).ok())
}

pub(super) fn resolve_tui_base_url() -> String {
    if let Some(value) = env_var_with_fallback("ROCODE_TUI_BASE_URL", "OPENCODE_TUI_BASE_URL") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    // Prefer a live backend endpoint over a hardcoded default. This avoids
    // accidental 404s when localhost:3000 is occupied by a non-opencode service.
    let candidates = [
        "http://127.0.0.1:3000",
        "http://localhost:3000",
        "http://127.0.0.1:4096",
        "http://localhost:4096",
    ];
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
    {
        Ok(client) => client,
        Err(_) => return "http://localhost:3000".to_string(),
    };

    for base in candidates {
        let health_url = format!("{}/health", base);
        if let Ok(response) = client.get(&health_url).send() {
            if response.status().is_success() {
                return base.to_string();
            }
        }
    }

    "http://localhost:3000".to_string()
}

/// Shared session filter. Updated by the app when the active session changes.
/// The SSE listener thread reads this on each reconnect to build the URL.
pub(super) type SessionFilter = Arc<StdMutex<Option<String>>>;

pub(super) fn spawn_server_event_listener(
    event_tx: Sender<Event>,
    base_url: String,
    session_filter: SessionFilter,
) {
    thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!(%err, "failed to initialize server event stream client");
                return;
            }
        };

        let base_event_url = format!("{}/event", base_url.trim_end_matches('/'));
        loop {
            // Read current session filter and build the SSE URL.
            let current_filter = session_filter.lock().ok().and_then(|guard| guard.clone());
            let event_url = match &current_filter {
                Some(sid) => format!("{}?session={}", base_event_url, sid),
                None => base_event_url.clone(),
            };

            match client
                .get(&event_url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
            {
                Ok(response) if response.status().is_success() => {
                    consume_server_event_stream(
                        response,
                        &event_tx,
                        &session_filter,
                        &current_filter,
                    );
                    // consume returned — either stream ended or filter changed.
                    // Loop around to reconnect with potentially new filter.
                }
                Ok(response) => {
                    tracing::debug!(
                        url = %event_url,
                        status = %response.status(),
                        "server event stream subscription rejected"
                    );
                    thread::sleep(Duration::from_millis(400));
                }
                Err(err) => {
                    tracing::debug!(
                        url = %event_url,
                        %err,
                        "server event stream disconnected"
                    );
                    thread::sleep(Duration::from_millis(400));
                }
            }
        }
    });
}

fn consume_server_event_stream(
    response: reqwest::blocking::Response,
    event_tx: &Sender<Event>,
    session_filter: &SessionFilter,
    connected_filter: &Option<String>,
) {
    let mut reader = BufReader::new(response);
    let mut line = String::new();
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                forward_server_event(&data_lines, event_tx);
                break;
            }
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    forward_server_event(&data_lines, event_tx);
                    data_lines.clear();

                    // After each complete event frame, check whether the
                    // session filter has changed. If so, break out to
                    // trigger a reconnect with the updated URL.
                    let current = session_filter.lock().ok().and_then(|guard| guard.clone());
                    if current != *connected_filter {
                        tracing::debug!(
                            old = ?connected_filter,
                            new = ?current,
                            "session filter changed, reconnecting SSE"
                        );
                        break;
                    }

                    continue;
                }
                if trimmed.starts_with(':') {
                    continue;
                }
                if let Some(payload) = trimmed.strip_prefix("data:") {
                    data_lines.push(payload.trim_start().to_string());
                }
            }
            Err(err) => {
                tracing::debug!(%err, "error while reading server event stream");
                break;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ServerEvent {
    #[serde(rename = "session.updated")]
    SessionUpdated {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        source: Option<String>,
    },
    #[serde(rename = "config.updated")]
    ConfigUpdated,
    #[serde(rename = "session.status")]
    SessionStatus {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
        status: SessionStatus,
    },
    #[serde(rename = "question.created")]
    QuestionCreated(QuestionEvent),
    #[serde(rename = "question.resolved")]
    QuestionResolved(QuestionEvent),
    #[serde(rename = "question.replied")]
    QuestionReplied(QuestionEvent),
    #[serde(rename = "question.rejected")]
    QuestionRejected(QuestionEvent),
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
        info: crate::api::PermissionRequestInfo,
    },
    #[serde(rename = "permission.resolved")]
    PermissionResolved(PermissionResolvedEvent),
    #[serde(rename = "permission.replied")]
    PermissionReplied(PermissionResolvedEvent),
    #[serde(rename = "tool_call.lifecycle")]
    ToolCallLifecycle(ToolCallLifecycleEvent),
    #[serde(rename = "tool_call.start")]
    ToolCallStart(ToolCallStartEvent),
    #[serde(rename = "tool_call.complete")]
    ToolCallComplete(ToolCallCompleteEvent),
    #[serde(rename = "execution.topology.changed")]
    TopologyChanged {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
    },
    #[serde(rename = "diff.updated")]
    DiffUpdated(DiffEvent),
    #[serde(rename = "session.diff")]
    SessionDiff(DiffEvent),
    #[serde(rename = "output_block")]
    OutputBlock(OutputBlockEvent),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct QuestionEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(rename = "requestID", alias = "requestId")]
    request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PermissionResolvedEvent {
    PermissionId {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
        #[serde(rename = "permissionID", alias = "permissionId")]
        permission_id: String,
    },
    RequestId {
        #[serde(rename = "sessionID", alias = "sessionId")]
        session_id: String,
        #[serde(rename = "requestID", alias = "requestId")]
        permission_id: String,
    },
}

impl PermissionResolvedEvent {
    fn session_id(&self) -> &str {
        match self {
            PermissionResolvedEvent::PermissionId { session_id, .. }
            | PermissionResolvedEvent::RequestId { session_id, .. } => session_id,
        }
    }

    fn permission_id(&self) -> &str {
        match self {
            PermissionResolvedEvent::PermissionId { permission_id, .. }
            | PermissionResolvedEvent::RequestId { permission_id, .. } => permission_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SessionStatus {
    Simple(String),
    Object(SessionStatusObject),
}

impl SessionStatus {
    fn kind(&self) -> Option<&str> {
        match self {
            SessionStatus::Simple(kind) => Some(kind.as_str()),
            SessionStatus::Object(obj) => obj.kind.as_deref(),
        }
    }

    fn retry_attempt(&self) -> u32 {
        match self {
            SessionStatus::Object(obj) => obj.attempt.unwrap_or(0),
            _ => 0,
        }
    }

    fn retry_message(&self) -> String {
        match self {
            SessionStatus::Object(obj) => obj.message.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn retry_next(&self) -> i64 {
        match self {
            SessionStatus::Object(obj) => obj.next.unwrap_or_default(),
            _ => 0,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct SessionStatusObject {
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    kind: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_u32_lossy")]
    attempt: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    message: Option<String>,
    #[serde(default)]
    next: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ToolCallLifecycleEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(rename = "toolCallId")]
    tool_call_id: Option<String>,
    phase: Option<String>,
    #[serde(rename = "toolName")]
    tool_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallStartEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(rename = "toolCallId")]
    tool_call_id: Option<String>,
    #[serde(rename = "toolName")]
    tool_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallCompleteEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(rename = "toolCallId")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiffEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_diff_entries_lossy")]
    diff: Vec<DiffEntryWire>,
}

#[derive(Debug, Deserialize)]
struct DiffEntryWire {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_u32_lossy")]
    additions: u32,
    #[serde(default, deserialize_with = "deserialize_u32_lossy")]
    deletions: u32,
}

#[derive(Debug, Deserialize)]
struct OutputBlockEvent {
    #[serde(rename = "sessionID", alias = "sessionId")]
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    id: Option<String>,
    block: serde_json::Value,
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

fn deserialize_opt_u32_lossy<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(number)) => {
            number.as_u64().and_then(|v| u32::try_from(v).ok())
        }
        Some(serde_json::Value::String(raw)) => raw.parse::<u32>().ok(),
        _ => None,
    })
}

fn deserialize_u32_lossy<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(deserialize_opt_u32_lossy(deserializer)?.unwrap_or(0))
}

fn deserialize_diff_entries_lossy<'de, D>(deserializer: D) -> Result<Vec<DiffEntryWire>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    Ok(match value {
        serde_json::Value::Array(_) => {
            serde_json::from_value::<Vec<DiffEntryWire>>(value).unwrap_or_default()
        }
        _ => Vec::new(),
    })
}

fn forward_server_event(data_lines: &[String], event_tx: &Sender<Event>) {
    if data_lines.is_empty() {
        return;
    }
    let payload = data_lines.join("\n");
    let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) else {
        return;
    };
    match event {
        ServerEvent::SessionUpdated { session_id, source } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::SessionUpdated { session_id, source },
            ))));
        }
        ServerEvent::ConfigUpdated => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::ConfigUpdated,
            ))));
        }
        ServerEvent::SessionStatus { session_id, status } => match status.kind() {
            Some("busy") => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusBusy(session_id),
                ))));
            }
            Some("idle") => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusIdle(session_id),
                ))));
            }
            Some("retry") => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusRetrying {
                        session_id,
                        attempt: status.retry_attempt(),
                        message: status.retry_message(),
                        next: status.retry_next(),
                    },
                ))));
            }
            _ => {}
        },
        ServerEvent::QuestionCreated(event) => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionCreated {
                    session_id: event.session_id,
                    request_id: event.request_id,
                },
            ))));
        }
        ServerEvent::QuestionResolved(event)
        | ServerEvent::QuestionReplied(event)
        | ServerEvent::QuestionRejected(event) => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionResolved {
                    session_id: event.session_id,
                    request_id: event.request_id,
                },
            ))));
        }
        ServerEvent::PermissionRequested { session_id, info } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::PermissionRequested {
                    session_id,
                    permission: info,
                },
            ))));
        }
        ServerEvent::PermissionResolved(event) | ServerEvent::PermissionReplied(event) => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::PermissionResolved {
                    session_id: event.session_id().to_string(),
                    permission_id: event.permission_id().to_string(),
                },
            ))));
        }
        ServerEvent::ToolCallLifecycle(event) => {
            let Some(tool_call_id) = event.tool_call_id.as_deref() else {
                tracing::warn!("tool_call.lifecycle missing toolCallId");
                return;
            };
            match event.phase.as_deref() {
                Some("start") => {
                    let Some(tool_name) = event.tool_name.as_deref() else {
                        tracing::warn!("tool_call.lifecycle start missing toolName");
                        return;
                    };
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::ToolCallStarted {
                            session_id: event.session_id,
                            tool_call_id: tool_call_id.to_string(),
                            tool_name: tool_name.to_string(),
                        },
                    ))));
                }
                Some("complete") => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::ToolCallCompleted {
                            session_id: event.session_id,
                            tool_call_id: tool_call_id.to_string(),
                        },
                    ))));
                }
                Some(other) => {
                    tracing::debug!(phase = other, "ignoring unknown tool_call.lifecycle phase");
                }
                None => {
                    tracing::warn!("tool_call.lifecycle missing phase");
                }
            }
        }
        ServerEvent::ToolCallStart(event) => {
            tracing::info!("Received tool_call.start event");
            let Some(tool_call_id) = event.tool_call_id.as_deref() else {
                tracing::warn!("tool_call.start missing toolCallId");
                return;
            };
            let Some(tool_name) = event.tool_name.as_deref() else {
                tracing::warn!("tool_call.start missing toolName");
                return;
            };
            tracing::info!(
                "Sending ToolCallStarted: id={}, name={}",
                tool_call_id,
                tool_name
            );
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::ToolCallStarted {
                    session_id: event.session_id,
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                },
            ))));
        }
        ServerEvent::ToolCallComplete(event) => {
            let Some(tool_call_id) = event.tool_call_id.as_deref() else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::ToolCallCompleted {
                    session_id: event.session_id,
                    tool_call_id: tool_call_id.to_string(),
                },
            ))));
        }
        ServerEvent::TopologyChanged { session_id } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::TopologyChanged { session_id },
            ))));
        }
        ServerEvent::DiffUpdated(event) | ServerEvent::SessionDiff(event) => {
            let diffs = event
                .diff
                .into_iter()
                .filter_map(|entry| {
                    let path = entry.path?;
                    Some(crate::context::DiffEntry {
                        file: path,
                        additions: entry.additions,
                        deletions: entry.deletions,
                    })
                })
                .collect::<Vec<_>>();
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::DiffUpdated {
                    session_id: event.session_id,
                    diffs,
                },
            ))));
        }
        ServerEvent::OutputBlock(event) => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::OutputBlock {
                    session_id: event.session_id,
                    id: event.id,
                    payload: event.block,
                },
            ))));
        }
        ServerEvent::Unknown => {}
    }
}

#[cfg(test)]
mod tests {
    use super::forward_server_event;
    use crate::event::{CustomEvent, StateChange};
    use crate::Event;
    use std::sync::mpsc::channel;

    #[test]
    fn output_block_forwarded_with_wrapper_id() {
        let (tx, rx) = channel();
        forward_server_event(
            &[serde_json::json!({
                "type": "output_block",
                "sessionID": "session-1",
                "id": "message-1",
                "block": {
                    "kind": "reasoning",
                    "phase": "delta",
                    "text": "thinking",
                }
            })
            .to_string()],
            &tx,
        );

        let event = rx.recv().expect("reasoning event");
        let Event::Custom(custom) = event else {
            panic!("expected custom event");
        };
        let CustomEvent::StateChanged(StateChange::OutputBlock {
            session_id,
            id,
            payload,
        }) = *custom
        else {
            panic!("expected output block event");
        };

        assert_eq!(session_id, "session-1");
        assert_eq!(id.as_deref(), Some("message-1"));
        assert_eq!(payload["kind"], "reasoning");
        assert_eq!(payload["phase"], "delta");
        assert_eq!(payload["text"], "thinking");
    }

    #[test]
    fn permission_requested_event_is_forwarded() {
        let (tx, rx) = channel();
        forward_server_event(
            &[serde_json::json!({
                "type": "permission.requested",
                "sessionID": "session-1",
                "permissionID": "permission-1",
                "info": {
                    "id": "permission-1",
                    "session_id": "session-1",
                    "tool": "bash",
                    "input": {
                        "permission": "bash",
                        "patterns": ["cargo test"],
                        "metadata": {"command": "cargo test"}
                    },
                    "message": "Permission required"
                }
            })
            .to_string()],
            &tx,
        );

        let event = rx.recv().expect("permission event");
        let Event::Custom(custom) = event else {
            panic!("expected custom event");
        };
        let CustomEvent::StateChanged(StateChange::PermissionRequested {
            session_id,
            permission,
        }) = *custom
        else {
            panic!("expected permission state change");
        };

        assert_eq!(session_id, "session-1");
        assert_eq!(permission.id, "permission-1");
        assert_eq!(permission.tool, "bash");
    }
}
