#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("model error: {0}")]
    ModelError(String),

    #[error("tool execution failed: {tool} - {error}")]
    ToolError { tool: String, error: String },

    #[error("max steps exceeded{0}")]
    MaxStepsExceeded(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("no provider available")]
    NoProvider,

    #[error("orchestrator error: {0}")]
    Other(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ToolExecError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("execution error: {0}")]
    ExecutionError(String),
}
