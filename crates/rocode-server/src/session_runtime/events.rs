use std::convert::Infallible;
use std::sync::Arc;

use axum::response::sse::Event;
use rocode_command::agent_presenter::output_block_to_web;
use rocode_command::output_blocks::OutputBlock;
use rocode_session::prompt::{OutputBlockEvent, OutputBlockHook};
use tokio::sync::mpsc;

use crate::ServerState;

pub use rocode_types::{DiffEntry, QuestionResolutionKind, ServerEvent};

fn output_block_event(
    session_id: impl Into<String>,
    block: &OutputBlock,
    id: Option<&str>,
) -> ServerEvent {
    ServerEvent::OutputBlock {
        session_id: session_id.into(),
        block: output_block_to_web(block),
        id: id.map(ToOwned::to_owned),
    }
}

fn server_event_to_sse_event(event: &ServerEvent) -> Option<Event> {
    Event::default()
        .event(event.event_name())
        .json_data(event)
        .ok()
}

pub(crate) fn server_output_block_event(event: &OutputBlockEvent) -> ServerEvent {
    output_block_event(event.session_id.clone(), &event.block, event.id.as_deref())
}

pub(crate) async fn send_sse_server_event(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    event: &ServerEvent,
) {
    if let Some(sse_event) = server_event_to_sse_event(event) {
        let _ = tx.send(Ok(sse_event)).await;
    }
}

pub(crate) fn broadcast_server_event(state: &ServerState, event: &ServerEvent) {
    if let Some(payload) = event.to_json_string() {
        state.broadcast(&payload);
    }
}

pub(crate) fn broadcast_output_block_event(state: &ServerState, event: &OutputBlockEvent) {
    let server_event = server_output_block_event(event);
    broadcast_server_event(state, &server_event);
}

pub(crate) fn server_output_block_hook(state: Arc<ServerState>) -> OutputBlockHook {
    Arc::new(move |event| {
        let state = state.clone();
        Box::pin(async move {
            broadcast_output_block_event(state.as_ref(), &event);
        })
    })
}

pub(crate) async fn emit_output_block_via_hook(
    output_hook: Option<&OutputBlockHook>,
    event: OutputBlockEvent,
) {
    let Some(output_hook) = output_hook else {
        return;
    };
    output_hook(event).await;
}

pub(crate) fn sse_output_block_hook(
    tx: mpsc::Sender<std::result::Result<Event, Infallible>>,
) -> OutputBlockHook {
    Arc::new(move |event| {
        let tx = tx.clone();
        Box::pin(async move {
            let server_event = server_output_block_event(&event);
            send_sse_server_event(&tx, &server_event).await;
        })
    })
}

pub(crate) fn broadcast_session_updated(
    state: &ServerState,
    session_id: impl Into<String>,
    source: impl Into<String>,
) {
    broadcast_server_event(
        state,
        &ServerEvent::SessionUpdated {
            session_id: session_id.into(),
            source: source.into(),
        },
    );
}

pub(crate) fn broadcast_config_updated(state: &ServerState) {
    broadcast_server_event(state, &ServerEvent::ConfigUpdated);
}

pub(crate) fn broadcast_child_session_attached(
    state: &ServerState,
    parent_id: impl Into<String>,
    child_id: impl Into<String>,
) {
    broadcast_server_event(
        state,
        &ServerEvent::ChildSessionAttached {
            parent_id: parent_id.into(),
            child_id: child_id.into(),
        },
    );
}

pub(crate) fn broadcast_child_session_detached(
    state: &ServerState,
    parent_id: impl Into<String>,
    child_id: impl Into<String>,
) {
    broadcast_server_event(
        state,
        &ServerEvent::ChildSessionDetached {
            parent_id: parent_id.into(),
            child_id: child_id.into(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{DiffEntry, QuestionResolutionKind, ServerEvent};
    use rocode_command::output_blocks::{OutputBlock, StatusBlock};
    use rocode_types::ToolCallPhase;

    #[test]
    fn server_event_serializes_output_block_wrapper() {
        let event = super::output_block_event(
            "session-1",
            &OutputBlock::Status(StatusBlock::success("ok")),
            Some("block-1"),
        );

        let value = event.to_json_value().expect("event json");
        assert_eq!(value["type"], "output_block");
        assert_eq!(value["sessionID"], "session-1");
        assert_eq!(value["id"], "block-1");
        assert_eq!(value["block"]["kind"], "status");
        assert_eq!(value["block"]["tone"], "success");
        assert_eq!(value["block"]["text"], "ok");
    }

    #[test]
    fn config_updated_event_serializes_as_tagged_type() {
        let value = ServerEvent::ConfigUpdated
            .to_json_value()
            .expect("event json");
        assert_eq!(value, serde_json::json!({ "type": "config.updated" }));
    }

    #[test]
    fn child_session_attached_serializes_with_parent_and_child_ids() {
        let value = ServerEvent::ChildSessionAttached {
            parent_id: "parent-1".to_string(),
            child_id: "child-1".to_string(),
        }
        .to_json_value()
        .expect("event json");
        assert_eq!(value["type"], "child_session.attached");
        assert_eq!(value["parentID"], "parent-1");
        assert_eq!(value["childID"], "child-1");
    }

    #[test]
    fn question_resolved_serializes_with_canonical_type() {
        let value = ServerEvent::QuestionResolved {
            session_id: "session-1".to_string(),
            request_id: "question-1".to_string(),
            resolution: Some(QuestionResolutionKind::Answered),
            answers: Some(serde_json::json!([["Yes"]])),
            reason: None,
        }
        .to_json_value()
        .expect("event json");

        assert_eq!(value["type"], "question.resolved");
        assert_eq!(value["resolution"], "answered");
        assert_eq!(value["requestID"], "question-1");
    }

    #[test]
    fn tool_call_lifecycle_serializes_with_phase() {
        let value = ServerEvent::ToolCallLifecycle {
            session_id: "session-1".to_string(),
            tool_call_id: "tool-1".to_string(),
            phase: ToolCallPhase::Start,
            tool_name: Some("shell".to_string()),
        }
        .to_json_value()
        .expect("event json");

        assert_eq!(value["type"], "tool_call.lifecycle");
        assert_eq!(value["phase"], "start");
        assert_eq!(value["toolName"], "shell");
    }

    #[test]
    fn diff_updated_serializes_with_canonical_type() {
        let value = ServerEvent::DiffUpdated {
            session_id: "session-1".to_string(),
            diff: vec![DiffEntry {
                path: "src/main.rs".to_string(),
                additions: 12,
                deletions: 3,
            }],
        }
        .to_json_value()
        .expect("event json");

        assert_eq!(value["type"], "diff.updated");
        assert_eq!(value["sessionID"], "session-1");
        assert_eq!(value["diff"][0]["path"], "src/main.rs");
    }

    #[test]
    fn legacy_question_replied_deserializes_as_question_resolved() {
        let event: ServerEvent = serde_json::from_value(serde_json::json!({
            "type": "question.replied",
            "sessionID": "session-1",
            "requestID": "question-1",
            "answers": [["Yes"]],
        }))
        .expect("legacy event");

        assert!(matches!(
            event,
            ServerEvent::QuestionResolved { request_id, .. } if request_id == "question-1"
        ));
    }

    #[test]
    fn legacy_permission_replied_deserializes_as_permission_resolved() {
        let event: ServerEvent = serde_json::from_value(serde_json::json!({
            "type": "permission.replied",
            "sessionID": "session-1",
            "requestID": "permission-1",
            "reply": "once",
        }))
        .expect("legacy event");

        assert!(matches!(
            event,
            ServerEvent::PermissionResolved { permission_id, .. }
                if permission_id == "permission-1"
        ));
    }

    #[test]
    fn legacy_session_diff_deserializes_as_diff_updated() {
        let event: ServerEvent = serde_json::from_value(serde_json::json!({
            "type": "session.diff",
            "sessionID": "session-1",
            "diff": [{
                "path": "src/main.rs",
                "additions": 1,
                "deletions": 0,
            }],
        }))
        .expect("legacy event");

        assert!(matches!(
            event,
            ServerEvent::DiffUpdated { session_id, diff }
                if session_id == "session-1" && diff.len() == 1
        ));
    }
}
