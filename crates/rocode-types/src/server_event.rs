use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionResolutionKind {
    Answered,
    Rejected,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallPhase {
    Start,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffEntry {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

/// Run status pushed over the server event stream as `session.status`.
///
/// This matches the server-side representation and is also accepted when
/// deserializing event payloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SessionRunStatus {
    #[default]
    Idle,
    Busy,
    Retry {
        attempt: u32,
        message: String,
        next: i64,
    },
}

/// Some clients/servers may send a plain string `"idle"` instead of
/// `{"type":"idle"}`; accept both formats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum SessionRunStatusWire {
    Tagged(SessionRunStatus),
    String(String),
}

impl SessionRunStatusWire {
    pub fn kind(&self) -> Option<&str> {
        match self {
            Self::Tagged(SessionRunStatus::Idle) => Some("idle"),
            Self::Tagged(SessionRunStatus::Busy) => Some("busy"),
            Self::Tagged(SessionRunStatus::Retry { .. }) => Some("retry"),
            Self::String(s) => Some(s.as_str()),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "output_block")]
    OutputBlock {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        block: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage {
        #[serde(
            rename = "sessionID",
            alias = "sessionId",
            alias = "session_id",
            skip_serializing_if = "Option::is_none"
        )]
        session_id: Option<String>,
        prompt_tokens: u64,
        completion_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(
            rename = "sessionID",
            alias = "sessionId",
            alias = "session_id",
            skip_serializing_if = "Option::is_none"
        )]
        session_id: Option<String>,
        error: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        done: Option<bool>,
    },
    #[serde(rename = "session.updated")]
    SessionUpdated {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        source: String,
    },
    #[serde(rename = "session.status")]
    SessionStatus {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        status: SessionRunStatusWire,
    },
    #[serde(rename = "question.created")]
    QuestionCreated {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(
            rename = "requestID",
            alias = "requestId",
            alias = "request_id",
            alias = "id"
        )]
        request_id: String,
        #[serde(default)]
        questions: serde_json::Value,
    },
    #[serde(
        rename = "question.resolved",
        alias = "question.replied",
        alias = "question.rejected"
    )]
    QuestionResolved {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(
            rename = "requestID",
            alias = "requestId",
            alias = "request_id",
            alias = "id"
        )]
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolution: Option<QuestionResolutionKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answers: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(
            rename = "permissionID",
            alias = "permissionId",
            alias = "requestID",
            alias = "requestId",
            alias = "id"
        )]
        permission_id: String,
        info: serde_json::Value,
    },
    #[serde(rename = "permission.resolved", alias = "permission.replied")]
    PermissionResolved {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(
            rename = "permissionID",
            alias = "permissionId",
            alias = "requestID",
            alias = "requestId",
            alias = "id"
        )]
        permission_id: String,
        reply: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "config.updated")]
    ConfigUpdated,
    #[serde(rename = "tool_call.lifecycle")]
    ToolCallLifecycle {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(rename = "toolCallId", alias = "tool_call_id", alias = "toolCallID")]
        tool_call_id: String,
        phase: ToolCallPhase,
        #[serde(
            rename = "toolName",
            alias = "tool_name",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        tool_name: Option<String>,
    },
    #[serde(rename = "execution.topology.changed")]
    TopologyChanged {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(
            rename = "executionID",
            alias = "executionId",
            alias = "execution_id",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        execution_id: Option<String>,
        #[serde(
            rename = "stageID",
            alias = "stageId",
            alias = "stage_id",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        stage_id: Option<String>,
    },
    #[serde(rename = "child_session.attached")]
    ChildSessionAttached {
        #[serde(rename = "parentID", alias = "parentId", alias = "parent_id")]
        parent_id: String,
        #[serde(rename = "childID", alias = "childId", alias = "child_id")]
        child_id: String,
    },
    #[serde(rename = "child_session.detached")]
    ChildSessionDetached {
        #[serde(rename = "parentID", alias = "parentId", alias = "parent_id")]
        parent_id: String,
        #[serde(rename = "childID", alias = "childId", alias = "child_id")]
        child_id: String,
    },
    #[serde(rename = "diff.updated", alias = "session.diff")]
    DiffUpdated {
        #[serde(rename = "sessionID", alias = "sessionId", alias = "session_id")]
        session_id: String,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        diff: Vec<DiffEntry>,
    },
    #[serde(rename = "tui.request")]
    TuiRequest {
        path: String,
        #[serde(default)]
        body: serde_json::Value,
    },
}

impl ServerEvent {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::OutputBlock { session_id, .. }
            | Self::Usage {
                session_id: Some(session_id),
                ..
            }
            | Self::Error {
                session_id: Some(session_id),
                ..
            }
            | Self::SessionUpdated { session_id, .. }
            | Self::SessionStatus { session_id, .. }
            | Self::QuestionCreated { session_id, .. }
            | Self::QuestionResolved { session_id, .. }
            | Self::PermissionRequested { session_id, .. }
            | Self::PermissionResolved { session_id, .. }
            | Self::ToolCallLifecycle { session_id, .. }
            | Self::TopologyChanged { session_id, .. }
            | Self::DiffUpdated { session_id, .. } => Some(session_id),
            Self::ChildSessionAttached { parent_id, .. }
            | Self::ChildSessionDetached { parent_id, .. } => Some(parent_id),
            Self::Usage {
                session_id: None, ..
            }
            | Self::Error {
                session_id: None, ..
            }
            | Self::ConfigUpdated
            | Self::TuiRequest { .. } => None,
        }
    }

    pub fn event_name(&self) -> &'static str {
        match self {
            Self::OutputBlock { .. } => "output_block",
            Self::Usage { .. } => "usage",
            Self::Error { .. } => "error",
            Self::SessionUpdated { .. } => "session.updated",
            Self::SessionStatus { .. } => "session.status",
            Self::QuestionCreated { .. } => "question.created",
            Self::QuestionResolved { .. } => "question.resolved",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionResolved { .. } => "permission.resolved",
            Self::ConfigUpdated => "config.updated",
            Self::ToolCallLifecycle { .. } => "tool_call.lifecycle",
            Self::TopologyChanged { .. } => "execution.topology.changed",
            Self::ChildSessionAttached { .. } => "child_session.attached",
            Self::ChildSessionDetached { .. } => "child_session.detached",
            Self::DiffUpdated { .. } => "diff.updated",
            Self::TuiRequest { .. } => "tui.request",
        }
    }

    pub fn to_json_string(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    pub fn to_json_value(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self).ok()
    }
}
