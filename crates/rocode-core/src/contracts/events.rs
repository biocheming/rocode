use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

/// Canonical server event type strings.
///
/// These values are used as:
/// - SSE `event:` names (for streaming clients)
/// - The `type` field inside JSON payloads
///
/// Keep them stable — they form a cross-crate wire contract between
/// `rocode-server`, `rocode-cli`, `rocode-tui`, and any future frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum ServerEventType {
    #[strum(serialize = "config.updated")]
    ConfigUpdated,
    #[strum(serialize = "session.updated")]
    SessionUpdated,
    #[strum(serialize = "session.status")]
    SessionStatus,
    #[strum(serialize = "question.created")]
    QuestionCreated,
    #[strum(
        serialize = "question.resolved",
        serialize = "question.replied",
        serialize = "question.rejected"
    )]
    QuestionResolved,
    #[strum(serialize = "permission.requested")]
    PermissionRequested,
    #[strum(serialize = "permission.resolved", serialize = "permission.replied")]
    PermissionResolved,
    #[strum(serialize = "tool_call.lifecycle")]
    ToolCallLifecycle,
    // Legacy split events (kept for forward/backward compatibility)
    #[strum(serialize = "tool_call.start")]
    ToolCallStart,
    #[strum(serialize = "tool_call.complete")]
    ToolCallComplete,
    #[strum(serialize = "execution.topology.changed")]
    ExecutionTopologyChanged,
    #[strum(serialize = "child_session.attached")]
    ChildSessionAttached,
    #[strum(serialize = "child_session.detached")]
    ChildSessionDetached,
    #[strum(serialize = "diff.updated", serialize = "session.diff")]
    DiffUpdated,
    #[strum(serialize = "output_block")]
    OutputBlock,
    #[strum(serialize = "usage")]
    Usage,
    #[strum(serialize = "error")]
    Error,
    // Internal server↔TUI request bus (not consumed by most clients)
    #[strum(serialize = "tui.request")]
    TuiRequest,
}

impl std::fmt::Display for ServerEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ServerEventType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigUpdated => "config.updated",
            Self::SessionUpdated => "session.updated",
            Self::SessionStatus => "session.status",
            Self::QuestionCreated => "question.created",
            Self::QuestionResolved => "question.resolved",
            Self::PermissionRequested => "permission.requested",
            Self::PermissionResolved => "permission.resolved",
            Self::ToolCallLifecycle => "tool_call.lifecycle",
            Self::ToolCallStart => "tool_call.start",
            Self::ToolCallComplete => "tool_call.complete",
            Self::ExecutionTopologyChanged => "execution.topology.changed",
            Self::ChildSessionAttached => "child_session.attached",
            Self::ChildSessionDetached => "child_session.detached",
            Self::DiffUpdated => "diff.updated",
            Self::OutputBlock => "output_block",
            Self::Usage => "usage",
            Self::Error => "error",
            Self::TuiRequest => "tui.request",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Phase of the tool call lifecycle events.
///
/// Wire format: lowercase strings (`"start"`, `"complete"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum ToolCallPhase {
    Start,
    Complete,
}

impl std::fmt::Display for ToolCallPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ToolCallPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Complete => "complete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Session run status tag used inside `session.status` payloads.
///
/// Wire format: lowercase strings (`"busy"`, `"idle"`, `"retry"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SessionRunStatusType {
    Idle,
    Busy,
    Retry,
}

impl std::fmt::Display for SessionRunStatusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SessionRunStatusType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Retry => "retry",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// How a question request was resolved.
///
/// Wire format: snake_case strings (`"answered"`, `"rejected"`, `"cancelled"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum QuestionResolutionKind {
    Answered,
    Rejected,
    Cancelled,
}

impl std::fmt::Display for QuestionResolutionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl QuestionResolutionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical internal bus event name strings.
///
/// These values are used with `rocode_core::bus::BusEventDef` and consumed by
/// session/server/runtime components.
///
/// Keep them stable — they form a cross-crate contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum BusEventName {
    #[strum(serialize = "mcp.tools.changed")]
    McpToolsChanged,
    #[strum(serialize = "session.compacted")]
    SessionCompacted,
    #[strum(serialize = "todo.updated")]
    TodoUpdated,
    #[strum(serialize = "agent_task.registered")]
    AgentTaskRegistered,
    #[strum(serialize = "agent_task.completed")]
    AgentTaskCompleted,
    #[strum(serialize = "file.edited")]
    FileEdited,
    #[strum(serialize = "file_watcher.updated")]
    FileWatcherUpdated,
    #[strum(serialize = "session.status")]
    SessionStatus,
    #[strum(serialize = "session.idle")]
    SessionIdle,
    #[strum(serialize = "session.created")]
    SessionCreated,
    #[strum(serialize = "session.updated")]
    SessionUpdated,
    #[strum(serialize = "session.deleted")]
    SessionDeleted,
    #[strum(serialize = "session.diff")]
    SessionDiff,
    #[strum(serialize = "session.error")]
    SessionError,
    #[strum(serialize = "message.updated")]
    MessageUpdated,
    #[strum(serialize = "message.removed")]
    MessageRemoved,
    #[strum(serialize = "message.part.updated")]
    MessagePartUpdated,
    #[strum(serialize = "message.part.removed")]
    MessagePartRemoved,
    #[strum(serialize = "message.part.delta")]
    MessagePartDelta,
    #[strum(serialize = "command.executed")]
    CommandExecuted,
}

impl std::fmt::Display for BusEventName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BusEventName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::McpToolsChanged => "mcp.tools.changed",
            Self::SessionCompacted => "session.compacted",
            Self::TodoUpdated => "todo.updated",
            Self::AgentTaskRegistered => "agent_task.registered",
            Self::AgentTaskCompleted => "agent_task.completed",
            Self::FileEdited => "file.edited",
            Self::FileWatcherUpdated => "file_watcher.updated",
            Self::SessionStatus => "session.status",
            Self::SessionIdle => "session.idle",
            Self::SessionCreated => "session.created",
            Self::SessionUpdated => "session.updated",
            Self::SessionDeleted => "session.deleted",
            Self::SessionDiff => "session.diff",
            Self::SessionError => "session.error",
            Self::MessageUpdated => "message.updated",
            Self::MessageRemoved => "message.removed",
            Self::MessagePartUpdated => "message.part.updated",
            Self::MessagePartRemoved => "message.part.removed",
            Self::MessagePartDelta => "message.part.delta",
            Self::CommandExecuted => "command.executed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_event_type_parses_legacy_aliases() {
        assert_eq!(
            ServerEventType::parse("question.replied"),
            Some(ServerEventType::QuestionResolved)
        );
        assert_eq!(
            ServerEventType::parse("permission.replied"),
            Some(ServerEventType::PermissionResolved)
        );
        assert_eq!(
            ServerEventType::parse("session.diff"),
            Some(ServerEventType::DiffUpdated)
        );
    }

    #[test]
    fn bus_event_names_round_trip() {
        let values: &[BusEventName] = &[
            BusEventName::McpToolsChanged,
            BusEventName::SessionCompacted,
            BusEventName::TodoUpdated,
            BusEventName::AgentTaskRegistered,
            BusEventName::AgentTaskCompleted,
            BusEventName::FileEdited,
            BusEventName::FileWatcherUpdated,
            BusEventName::SessionStatus,
            BusEventName::SessionIdle,
            BusEventName::SessionCreated,
            BusEventName::SessionUpdated,
            BusEventName::SessionDeleted,
            BusEventName::SessionDiff,
            BusEventName::SessionError,
            BusEventName::MessageUpdated,
            BusEventName::MessageRemoved,
            BusEventName::MessagePartUpdated,
            BusEventName::MessagePartRemoved,
            BusEventName::MessagePartDelta,
            BusEventName::CommandExecuted,
        ];
        for value in values {
            assert_eq!(BusEventName::parse(value.as_str()), Some(*value));
            assert_eq!(value.to_string(), value.as_str());
        }
    }
}
