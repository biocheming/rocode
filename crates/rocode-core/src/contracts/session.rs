use strum_macros::EnumString;

/// Shared session + message metadata keys.
///
/// These are used across server/cli/tui/session layers and should remain stable.
pub mod keys {
    // Session/runtime selection
    pub const MODEL_PROVIDER: &str = "model_provider";
    pub const MODEL_ID: &str = "model_id";
    pub const MODEL_VARIANT: &str = "model_variant";
    pub const AGENT: &str = "agent";

    // Legacy compatibility
    pub const LEGACY_PROVIDER_ID: &str = "provider_id";

    // Scheduler applied flags (session-level)
    pub const SCHEDULER_APPLIED: &str = "scheduler_applied";
    pub const SCHEDULER_SKILL_TREE_APPLIED: &str = "scheduler_skill_tree_applied";
    pub const SCHEDULER_ROOT_AGENT: &str = "scheduler_root_agent";

    // Resolved prompt/debug metadata (message-level)
    pub const RESOLVED_AGENT: &str = "resolved_agent";
    pub const RESOLVED_EXECUTION_MODE_KIND: &str = "resolved_execution_mode_kind";
    pub const RESOLVED_SYSTEM_PROMPT: &str = "resolved_system_prompt";
    pub const RESOLVED_SYSTEM_PROMPT_PREVIEW: &str = "resolved_system_prompt_preview";
    pub const RESOLVED_SYSTEM_PROMPT_APPLIED: &str = "resolved_system_prompt_applied";
    pub const RESOLVED_USER_PROMPT: &str = "resolved_user_prompt";

    // Recovery bookkeeping (session-level)
    pub const LAST_RECOVERY_ACTION: &str = "last_recovery_action";
    pub const LAST_RECOVERY_TARGET_ID: &str = "last_recovery_target_id";
    pub const LAST_RECOVERY_TARGET_KIND: &str = "last_recovery_target_kind";
    pub const LAST_RECOVERY_TARGET_LABEL: &str = "last_recovery_target_label";

    // Recovery context attached to prompt messages (message-level)
    pub const RECOVERY_ACTION: &str = "recovery_action";
    pub const RECOVERY_TARGET_ID: &str = "recovery_target_id";
    pub const RECOVERY_TARGET_KIND: &str = "recovery_target_kind";
    pub const RECOVERY_TARGET_LABEL: &str = "recovery_target_label";

    // Generic message metadata
    pub const MODE: &str = "mode";
    pub const COMPLETED_AT: &str = "completed_at";
    pub const FINISH_REASON: &str = "finish_reason";
    pub const USAGE: &str = "usage";
    pub const COST: &str = "cost";
    pub const ERROR: &str = "error";

    // Token usage (message-level)
    pub const TOKENS_INPUT: &str = "tokens_input";
    pub const TOKENS_OUTPUT: &str = "tokens_output";
    pub const TOKENS_REASONING: &str = "tokens_reasoning";
    pub const TOKENS_CACHE_READ: &str = "tokens_cache_read";
    pub const TOKENS_CACHE_WRITE: &str = "tokens_cache_write";
}

/// Canonical message role strings used in persisted session/message payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum MessageRoleWire {
    User,
    Assistant,
    System,
    Tool,
}

impl std::fmt::Display for MessageRoleWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MessageRoleWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical part-type tags used in session message JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum MessagePartTypeWire {
    Text,
    ToolCall,
    ToolResult,
    Reasoning,
    File,
    StepStart,
    StepFinish,
    Snapshot,
    Patch,
    Agent,
    Subtask,
    Retry,
    Compaction,
}

impl std::fmt::Display for MessagePartTypeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MessagePartTypeWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Reasoning => "reasoning",
            Self::File => "file",
            Self::StepStart => "step_start",
            Self::StepFinish => "step_finish",
            Self::Snapshot => "snapshot",
            Self::Patch => "patch",
            Self::Agent => "agent",
            Self::Subtask => "subtask",
            Self::Retry => "retry",
            Self::Compaction => "compaction",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}
