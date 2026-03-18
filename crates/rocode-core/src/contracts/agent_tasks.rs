use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use strum_macros::{EnumIter, EnumString};

/// Bus event payload keys for `agent_task.*` events.
pub mod bus_keys {
    pub const TASK_ID: &str = "task_id";
    pub const SESSION_ID: &str = "session_id";
    pub const AGENT_NAME: &str = "agent_name";
    pub const PARENT_TOOL_CALL_ID: &str = "parent_tool_call_id";
}

/// Canonical agent task lifecycle status labels.
///
/// These values are used as:
/// - `task_flow` tool `status_filter` inputs
/// - API/UI status strings for agent task registry projections
///
/// Keep them stable — they are part of the cross-crate wire contract.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    EnumString,
    EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive)]
pub enum AgentTaskStatusKind {
    #[strum(serialize = "pending")]
    Pending,
    #[strum(
        serialize = "running",
        serialize = "in_progress",
        serialize = "in-progress",
        serialize = "inprogress"
    )]
    Running,
    #[strum(
        serialize = "completed",
        serialize = "done",
        serialize = "complete",
        serialize = "success"
    )]
    Completed,
    #[strum(serialize = "cancelled", serialize = "canceled")]
    Cancelled,
    #[strum(serialize = "failed", serialize = "error", serialize = "failure")]
    Failed,
}

impl std::fmt::Display for AgentTaskStatusKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AgentTaskStatusKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }

    pub fn allowed_values() -> Vec<&'static str> {
        Self::iter().map(Self::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases() {
        assert_eq!(
            AgentTaskStatusKind::parse("in-progress"),
            Some(AgentTaskStatusKind::Running)
        );
        assert_eq!(
            AgentTaskStatusKind::parse("done"),
            Some(AgentTaskStatusKind::Completed)
        );
        assert_eq!(
            AgentTaskStatusKind::parse("canceled"),
            Some(AgentTaskStatusKind::Cancelled)
        );
        assert_eq!(
            AgentTaskStatusKind::parse("error"),
            Some(AgentTaskStatusKind::Failed)
        );
    }
}
