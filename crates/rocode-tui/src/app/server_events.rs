use super::*;

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

pub(super) fn spawn_server_event_listener(event_tx: Sender<Event>, base_url: String) {
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

        let event_url = format!("{}/event", base_url.trim_end_matches('/'));
        loop {
            match client
                .get(&event_url)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .send()
            {
                Ok(response) if response.status().is_success() => {
                    consume_server_event_stream(response, &event_tx);
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

fn consume_server_event_stream(response: reqwest::blocking::Response, event_tx: &Sender<Event>) {
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
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return;
    };
    let event_type = value.get("type").and_then(|item| item.as_str());
    let session_id = value
        .get("sessionID")
        .and_then(|item| item.as_str())
        .or_else(|| value.get("sessionId").and_then(|item| item.as_str()));
    match event_type {
        Some("session.updated") => {
            let Some(session_id) = session_id else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::SessionUpdated(session_id.to_string()),
            ))));
        }
        Some("session.status") => {
            let Some(session_id) = session_id else {
                return;
            };
            let status_type = value
                .get("status")
                .and_then(|status| status.get("type"))
                .and_then(|item| item.as_str())
                .or_else(|| value.get("status").and_then(|item| item.as_str()));
            match status_type {
                Some("busy") => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusBusy(session_id.to_string()),
                    ))));
                }
                Some("idle") => {
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusIdle(session_id.to_string()),
                    ))));
                }
                Some("retry") => {
                    let attempt = value
                        .get("status")
                        .and_then(|status| status.get("attempt"))
                        .and_then(|item| item.as_u64())
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0);
                    let message = value
                        .get("status")
                        .and_then(|status| status.get("message"))
                        .and_then(|item| item.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let next = value
                        .get("status")
                        .and_then(|status| status.get("next"))
                        .and_then(|item| item.as_i64())
                        .unwrap_or_default();
                    let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                        StateChange::SessionStatusRetrying {
                            session_id: session_id.to_string(),
                            attempt,
                            message,
                            next,
                        },
                    ))));
                }
                _ => {}
            }
        }
        Some("question.created") => {
            let Some(session_id) = session_id else {
                return;
            };
            let Some(request_id) = value
                .get("requestID")
                .and_then(|item| item.as_str())
                .or_else(|| value.get("requestId").and_then(|item| item.as_str()))
            else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionCreated {
                    session_id: session_id.to_string(),
                    request_id: request_id.to_string(),
                },
            ))));
        }
        Some("question.replied") | Some("question.rejected") => {
            let Some(session_id) = session_id else {
                return;
            };
            let Some(request_id) = value
                .get("requestID")
                .and_then(|item| item.as_str())
                .or_else(|| value.get("requestId").and_then(|item| item.as_str()))
            else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::QuestionResolved {
                    session_id: session_id.to_string(),
                    request_id: request_id.to_string(),
                },
            ))));
        }
        Some("tool_call.start") => {
            tracing::info!("Received tool_call.start event");
            let Some(session_id) = session_id else {
                return;
            };
            let Some(tool_call_id) = value.get("toolCallId").and_then(|item| item.as_str()) else {
                tracing::warn!("tool_call.start missing toolCallId");
                return;
            };
            let Some(tool_name) = value.get("toolName").and_then(|item| item.as_str()) else {
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
                    session_id: session_id.to_string(),
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                },
            ))));
        }
        Some("tool_call.complete") => {
            let Some(session_id) = session_id else {
                return;
            };
            let Some(tool_call_id) = value.get("toolCallId").and_then(|item| item.as_str()) else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::ToolCallCompleted {
                    session_id: session_id.to_string(),
                    tool_call_id: tool_call_id.to_string(),
                },
            ))));
        }
        Some("execution.topology.changed") => {
            let Some(session_id) = session_id else {
                return;
            };
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::TopologyChanged {
                    session_id: session_id.to_string(),
                },
            ))));
        }
        Some("session.diff") => {
            let Some(session_id) = session_id else {
                return;
            };
            let diffs = value
                .get("diff")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|entry| {
                            let path = entry.get("path")?.as_str()?;
                            let additions = entry.get("additions")?.as_u64().unwrap_or(0);
                            let deletions = entry.get("deletions")?.as_u64().unwrap_or(0);
                            Some(crate::context::DiffEntry {
                                file: path.to_string(),
                                additions: additions as u32,
                                deletions: deletions as u32,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                StateChange::DiffUpdated {
                    session_id: session_id.to_string(),
                    diffs,
                },
            ))));
        }
        Some("output_block") => {
            let Some(session_id) = session_id else {
                return;
            };
            let Some(block) = value.get("block") else {
                return;
            };
            let kind = block.get("kind").and_then(|item| item.as_str());

            if kind == Some("reasoning") {
                let phase = block
                    .get("phase")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default();
                let text = block
                    .get("text")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default();
                let message_id = block
                    .get("id")
                    .and_then(|item| item.as_str())
                    .or_else(|| value.get("id").and_then(|item| item.as_str()))
                    .unwrap_or_default();

                let _ = event_tx.send(Event::Custom(Box::new(CustomEvent::StateChanged(
                    StateChange::ReasoningUpdated {
                        session_id: session_id.to_string(),
                        message_id: message_id.to_string(),
                        phase: phase.to_string(),
                        text: text.to_string(),
                    },
                ))));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::forward_server_event;
    use crate::event::{CustomEvent, StateChange};
    use crate::Event;
    use std::sync::mpsc::channel;

    #[test]
    fn output_block_reasoning_falls_back_to_wrapper_id() {
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
        let CustomEvent::StateChanged(StateChange::ReasoningUpdated {
            session_id,
            message_id,
            phase,
            text,
        }) = *custom
        else {
            panic!("expected reasoning update");
        };

        assert_eq!(session_id, "session-1");
        assert_eq!(message_id, "message-1");
        assert_eq!(phase, "delta");
        assert_eq!(text, "thinking");
    }
}
