/// Shared JSON field-name constants used across wire payloads.
///
/// These keys appear in:
/// - SSE server events (`rocode-server` → CLI/TUI/Web)
/// - Bus event payloads (internal runtime hooks)
/// - Plugin hook I/O shims
///
/// Keep them stable — they are part of the cross-crate contract.
pub mod keys {
    /// Generic payload type discriminant key.
    pub const TYPE: &str = "type";

    /// Canonical session identifier key used in event payloads.
    pub const SESSION_ID: &str = "sessionID";
    /// Canonical message identifier key used in event payloads.
    pub const MESSAGE_ID: &str = "messageID";
    /// Canonical parent session identifier key used in tree-attach events.
    pub const PARENT_ID: &str = "parentID";
    /// Canonical child session identifier key used in tree-attach events.
    pub const CHILD_ID: &str = "childID";
    /// Canonical question request identifier key used in interaction events.
    pub const REQUEST_ID: &str = "requestID";
    /// Canonical permission identifier key used in permission events.
    pub const PERMISSION_ID: &str = "permissionID";
    /// Canonical tool call identifier key used in event payloads.
    pub const TOOL_CALL_ID: &str = "toolCallId";
    /// Canonical wrapped output block key used by `output_block` events.
    pub const BLOCK: &str = "block";
    /// Common error-message field key used by `error` events.
    pub const ERROR: &str = "error";
    /// Common message field key used by `error` events and status payloads.
    pub const MESSAGE: &str = "message";

    /// Execution topology identifier key used in stage/execution events.
    pub const EXECUTION_ID: &str = "executionID";
    /// Scheduler stage identifier key used in stage/execution events.
    pub const STAGE_ID: &str = "stageID";
}

/// Alternate key spellings accepted for backwards compatibility.
pub mod aliases {
    pub const SESSION_ID_CAMEL: &str = "sessionId";
    pub const SESSION_ID_SNAKE: &str = "session_id";
    pub const MESSAGE_ID_CAMEL: &str = "messageId";
    pub const MESSAGE_ID_SNAKE: &str = "message_id";
    pub const PARENT_ID_CAMEL: &str = "parentId";
    pub const PARENT_ID_SNAKE: &str = "parent_id";
    pub const CHILD_ID_CAMEL: &str = "childId";
    pub const CHILD_ID_SNAKE: &str = "child_id";
    pub const REQUEST_ID_CAMEL: &str = "requestId";
    pub const REQUEST_ID_SNAKE: &str = "request_id";
    pub const PERMISSION_ID_CAMEL: &str = "permissionId";
    pub const PERMISSION_ID_SNAKE: &str = "permission_id";
    pub const TOOL_CALL_ID_SNAKE: &str = "tool_call_id";
}

/// Common non-identifier payload field names reused across wire contracts.
pub mod fields {
    pub const SOURCE: &str = "source";
    pub const STATUS: &str = "status";
    pub const PHASE: &str = "phase";
    pub const ROLE: &str = "role";
    pub const TOOL_NAME: &str = "toolName";
    pub const TOOL_NAME_SNAKE: &str = "tool_name";
    pub const RESOLUTION: &str = "resolution";
    pub const QUESTIONS: &str = "questions";
    pub const QUESTION: &str = "question";
    pub const HEADER: &str = "header";
    pub const OPTIONS: &str = "options";
    pub const MULTIPLE: &str = "multiple";
    pub const LABEL: &str = "label";
    pub const VALUE: &str = "value";
    pub const INFO: &str = "info";
    pub const ID: &str = "id";
    pub const DONE: &str = "done";
    pub const PROMPT_TOKENS: &str = "prompt_tokens";
    pub const COMPLETION_TOKENS: &str = "completion_tokens";
    pub const ADDITIONS: &str = "additions";
    pub const DELETIONS: &str = "deletions";
}

/// Reusable key-sets for tolerant readers that accept canonical and legacy keys.
pub mod keysets {
    use super::{aliases, keys};

    pub const SESSION_ID_ANY: &[&str] = &[
        keys::SESSION_ID,
        aliases::SESSION_ID_CAMEL,
        aliases::SESSION_ID_SNAKE,
    ];
    pub const MESSAGE_ID_ANY: &[&str] = &[
        keys::MESSAGE_ID,
        aliases::MESSAGE_ID_CAMEL,
        aliases::MESSAGE_ID_SNAKE,
    ];
    pub const REQUEST_ID_ANY: &[&str] = &[
        keys::REQUEST_ID,
        aliases::REQUEST_ID_CAMEL,
        aliases::REQUEST_ID_SNAKE,
        fields::ID,
    ];
    pub const PERMISSION_ID_ANY: &[&str] = &[
        keys::PERMISSION_ID,
        aliases::PERMISSION_ID_CAMEL,
        aliases::PERMISSION_ID_SNAKE,
        keys::REQUEST_ID,
        aliases::REQUEST_ID_CAMEL,
        fields::ID,
    ];
    pub const PARENT_ID_ANY: &[&str] = &[
        keys::PARENT_ID,
        aliases::PARENT_ID_CAMEL,
        aliases::PARENT_ID_SNAKE,
    ];
    pub const CHILD_ID_ANY: &[&str] = &[
        keys::CHILD_ID,
        aliases::CHILD_ID_CAMEL,
        aliases::CHILD_ID_SNAKE,
    ];
    pub const TOOL_CALL_ID_ANY: &[&str] = &[keys::TOOL_CALL_ID, aliases::TOOL_CALL_ID_SNAKE];

    use super::fields;
}

/// Small JSON key lookup helpers for wire payload readers.
pub mod selectors {
    use serde_json::Value;

    pub fn first_value<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
        keys.iter().find_map(|key| payload.get(*key))
    }

    pub fn first_str<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
        first_value(payload, keys).and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::{keysets, selectors};

    #[test]
    fn session_id_selector_accepts_aliases() {
        let payload = serde_json::json!({
            "sessionId": "session-1",
        });

        assert_eq!(
            selectors::first_str(&payload, keysets::SESSION_ID_ANY),
            Some("session-1")
        );
    }

    #[test]
    fn permission_id_selector_falls_back_to_request_id() {
        let payload = serde_json::json!({
            "requestID": "permission-1",
        });

        assert_eq!(
            selectors::first_str(&payload, keysets::PERMISSION_ID_ANY),
            Some("permission-1")
        );
    }

    #[test]
    fn tool_call_id_selector_accepts_snake_case() {
        let payload = serde_json::json!({
            "tool_call_id": "tool-1",
        });

        assert_eq!(
            selectors::first_str(&payload, keysets::TOOL_CALL_ID_ANY),
            Some("tool-1")
        );
    }
}
