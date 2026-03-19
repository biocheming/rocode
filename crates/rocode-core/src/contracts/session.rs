/// Shared session + message metadata keys.
///
/// These are used across server/cli/tui/session layers and should remain stable.
pub mod keys {
    // Session/runtime selection
    pub const MODEL_PROVIDER: &str = "model_provider";
    pub const MODEL_ID: &str = "model_id";
    pub const MODEL_VARIANT: &str = "model_variant";
    pub const LEGACY_VARIANT: &str = "variant";
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

    // Prompt/runtime bookkeeping
    pub const PENDING_SUBTASKS: &str = "pending_subtasks";
    pub const SUBTASK_ID: &str = "subtask_id";
    pub const SUBTASK_STATUS: &str = "subtask_status";
    pub const SUBTASK_AGENT: &str = "agent";
    pub const SUBTASK_PROMPT: &str = "prompt";
    pub const SUBTASK_DESCRIPTION: &str = "description";
    pub const SYNTHETIC_AGENT: &str = "synthetic_agent";
    pub const MCP_TOOLS: &str = "mcp_tools";
    pub const SUBSESSIONS: &str = "subsessions";
    pub const SUMMARY: &str = "summary";
    pub const SNAPSHOT: &str = "snapshot";
    pub const STEP_START_SNAPSHOT: &str = "step_start_snapshot";
    pub const STEP_FINISH_SNAPSHOT: &str = "step_finish_snapshot";
    pub const LOADED_INSTRUCTION_FILES: &str = "loaded_instruction_files";
}
