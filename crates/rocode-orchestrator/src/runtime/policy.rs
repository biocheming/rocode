// ---------------------------------------------------------------------------
// LoopPolicy – controls retry, dedup, and error handling behavior.
// Passed to run_loop to configure the execution semantics.
// ---------------------------------------------------------------------------

/// Policy configuration for a single `run_loop` invocation.
#[derive(Debug, Clone)]
pub struct LoopPolicy {
    /// Maximum number of agentic steps (model calls) before stopping.
    /// `None` means no scheduler-imposed step cap.
    pub max_steps: Option<u32>,

    /// Scope for tool_call_id deduplication.
    pub tool_dedup: ToolDedupScope,

    /// How to handle tool execution errors.
    pub on_tool_error: ToolErrorStrategy,
}

impl Default for LoopPolicy {
    fn default() -> Self {
        Self {
            max_steps: Some(100),
            tool_dedup: ToolDedupScope::Global,
            on_tool_error: ToolErrorStrategy::ReportAndContinue,
        }
    }
}

/// Scope of tool_call_id deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDedupScope {
    /// Dedup across the entire `run_loop` invocation (default).
    /// A tool_call_id seen in any step will not be dispatched again.
    Global,

    /// Only dedup within a single step.
    /// The same tool_call_id in different steps will be dispatched.
    PerStep,

    /// No dedup at all.
    None,
}

/// Strategy for handling tool execution errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolErrorStrategy {
    /// Fail the entire loop on the first tool error.
    Fail,

    /// Skip the failed tool call and continue. A synthetic error result is
    /// added to the conversation so the model sees a complete tool response.
    Skip,

    /// Report the error as a tool result and continue (default).
    /// The model sees the error message and can decide how to proceed.
    ReportAndContinue,
}
