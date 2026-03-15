use std::convert::Infallible;

use axum::response::sse::Event;
use rocode_command::agent_presenter::output_block_to_web;
use rocode_command::output_blocks::OutputBlock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::ServerState;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "output_block")]
    OutputBlock {
        #[serde(rename = "sessionID")]
        session_id: String,
        block: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage {
        #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        prompt_tokens: u64,
        completion_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        done: Option<bool>,
    },
    #[serde(rename = "session.updated")]
    SessionUpdated {
        #[serde(rename = "sessionID")]
        session_id: String,
        source: String,
    },
    #[serde(rename = "session.status")]
    SessionStatus {
        #[serde(rename = "sessionID")]
        session_id: String,
        status: String,
    },
    #[serde(rename = "question.created")]
    QuestionCreated {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "requestID")]
        request_id: String,
        questions: serde_json::Value,
    },
    #[serde(rename = "question.replied")]
    QuestionReplied {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "requestID")]
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        answers: Option<serde_json::Value>,
    },
    #[serde(rename = "question.rejected")]
    QuestionRejected {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "requestID")]
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "permission.replied")]
    PermissionReplied {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "requestID")]
        request_id: String,
        reply: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "config.updated")]
    ConfigUpdated,
    #[serde(rename = "tool_call.start")]
    ToolCallStarted {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
    },
    #[serde(rename = "tool_call.complete")]
    ToolCallCompleted {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    },
    #[serde(rename = "execution.topology.changed")]
    TopologyChanged {
        #[serde(rename = "sessionID")]
        session_id: String,
        #[serde(rename = "executionID", skip_serializing_if = "Option::is_none")]
        execution_id: Option<String>,
        #[serde(rename = "stageID", skip_serializing_if = "Option::is_none")]
        stage_id: Option<String>,
    },
}

impl ServerEvent {
    pub(crate) fn output_block(
        session_id: impl Into<String>,
        block: &OutputBlock,
        id: Option<&str>,
    ) -> Self {
        Self::OutputBlock {
            session_id: session_id.into(),
            block: output_block_to_web(block),
            id: id.map(ToOwned::to_owned),
        }
    }

    pub(crate) fn event_name(&self) -> &'static str {
        match self {
            Self::OutputBlock { .. } => "output_block",
            Self::Usage { .. } => "usage",
            Self::Error { .. } => "error",
            Self::SessionUpdated { .. } => "session.updated",
            Self::SessionStatus { .. } => "session.status",
            Self::QuestionCreated { .. } => "question.created",
            Self::QuestionReplied { .. } => "question.replied",
            Self::QuestionRejected { .. } => "question.rejected",
            Self::PermissionReplied { .. } => "permission.replied",
            Self::ConfigUpdated => "config.updated",
            Self::ToolCallStarted { .. } => "tool_call.start",
            Self::ToolCallCompleted { .. } => "tool_call.complete",
            Self::TopologyChanged { .. } => "execution.topology.changed",
        }
    }

    pub(crate) fn to_json_string(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    pub(crate) fn to_json_value(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self).ok()
    }

    pub(crate) fn to_sse_event(&self) -> Option<Event> {
        Event::default().event(self.event_name()).json_data(self).ok()
    }
}

pub(crate) async fn send_sse_server_event(
    tx: &mpsc::Sender<std::result::Result<Event, Infallible>>,
    event: &ServerEvent,
) {
    if let Some(sse_event) = event.to_sse_event() {
        let _ = tx.send(Ok(sse_event)).await;
    }
}

pub(crate) fn broadcast_server_event(state: &ServerState, event: &ServerEvent) {
    if let Some(payload) = event.to_json_string() {
        state.broadcast(&payload);
    }
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
