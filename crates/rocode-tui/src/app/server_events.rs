use super::*;
use rocode_command::output_blocks::OutputBlock as WireOutputBlock;
use rocode_types::{DiffEntry, ServerEvent, SessionRunStatus, SessionRunStatusWire, ToolCallPhase};
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
                StateChange::SessionUpdated {
                    session_id,
                    source: Some(source),
                },
            ))));
        }
        ServerEvent::ConfigUpdated => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::ConfigUpdated,
            ))));
        }
        ServerEvent::SessionStatus { session_id, status } => match status {
            SessionRunStatusWire::Tagged(SessionRunStatus::Busy) => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusBusy(session_id),
                ))));
            }
            SessionRunStatusWire::Tagged(SessionRunStatus::Idle) => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusIdle(session_id),
                ))));
            }
            SessionRunStatusWire::Tagged(SessionRunStatus::Retry {
                attempt,
                message,
                next,
            }) => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::SessionStatusRetrying {
                        session_id,
                        attempt,
                        message,
                        next,
                    },
                ))));
            }
            SessionRunStatusWire::String(kind) => match kind.as_str() {
                "busy" => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusBusy(session_id),
                    ))));
                }
                "idle" => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusIdle(session_id),
                    ))));
                }
                "retry" => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusRetrying {
                            session_id,
                            attempt: 0,
                            message: String::new(),
                            next: 0,
                        },
                    ))));
                }
                _ => {}
            },
        },
        ServerEvent::QuestionCreated {
            session_id,
            request_id,
            ..
        } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionCreated {
                    session_id,
                    request_id,
                },
            ))));
        }
        ServerEvent::QuestionResolved {
            session_id,
            request_id,
            ..
        } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionResolved {
                    session_id,
                    request_id,
                },
            ))));
        }
        ServerEvent::PermissionRequested {
            session_id, info, ..
        } => {
            let Ok(permission) = serde_json::from_value::<crate::api::PermissionRequestInfo>(info)
            else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::PermissionRequested {
                    session_id,
                    permission,
                },
            ))));
        }
        ServerEvent::PermissionResolved {
            session_id,
            permission_id,
            ..
        } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::PermissionResolved {
                    session_id,
                    permission_id,
                },
            ))));
        }
        ServerEvent::ToolCallLifecycle {
            session_id,
            tool_call_id,
            phase,
            tool_name,
        } => match phase {
            ToolCallPhase::Start => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::ToolCallStarted {
                        session_id,
                        tool_call_id,
                        tool_name: tool_name.unwrap_or_default(),
                    },
                ))));
            }
            ToolCallPhase::Complete => {
                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::ToolCallCompleted {
                        session_id,
                        tool_call_id,
                    },
                ))));
            }
        },
        ServerEvent::TopologyChanged { session_id, .. } => {
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::TopologyChanged { session_id },
            ))));
        }
        ServerEvent::DiffUpdated { session_id, diff } => {
            let diffs = diff
                .into_iter()
                .map(
                    |DiffEntry {
                         path,
                         additions,
                         deletions,
                     }| crate::context::DiffEntry {
                        file: path,
                        additions: additions as u32,
                        deletions: deletions as u32,
                    },
                )
                .collect::<Vec<_>>();
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::DiffUpdated { session_id, diffs },
            ))));
        }
        ServerEvent::OutputBlock {
            session_id,
            block,
            id,
            ..
        } => {
            let block = match serde_json::from_value::<WireOutputBlock>(block) {
                Ok(block) => block,
                Err(err) => {
                    tracing::debug!(%err, session_id, "failed to parse output block payload");
                    return;
                }
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::OutputBlock {
                    id,
                    session_id,
                    payload: block,
                },
            ))));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::forward_server_event;
    use crate::event::{CustomEvent, StateChange};
    use crate::Event;
    use rocode_command::output_blocks::{MessagePhase, OutputBlock};
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
        match payload {
            OutputBlock::Reasoning(block) => {
                assert_eq!(block.phase, MessagePhase::Delta);
                assert_eq!(block.text, "thinking");
            }
            other => panic!("expected reasoning block, got {other:?}"),
        }
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
