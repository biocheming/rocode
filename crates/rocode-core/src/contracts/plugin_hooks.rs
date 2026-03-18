/// Shared plugin hook I/O key contracts.
///
/// Script-style hooks (TypeScript plugins) use a small set of normalized keys
/// for `(input, output)` JSON payloads. These keys are consumed across:
/// - `rocode-tool` (hook context producers)
/// - `rocode-plugin` (hook I/O normalization shims)
/// - TypeScript plugin host/runtime
///
/// Keep them stable — they are part of the cross-crate wire contract.
pub mod keys {
    // Tool definition hook
    pub const TOOL_ID: &str = "toolID";
    pub const DESCRIPTION: &str = "description";
    pub const PARAMETERS: &str = "parameters";

    // Tool execution hooks
    pub const TOOL: &str = "tool";
    pub const CALL_ID: &str = "callID";
    pub const ARGS: &str = "args";
    pub const ERROR: &str = "error";
    pub const TITLE: &str = "title";
    pub const OUTPUT: &str = "output";
    pub const METADATA: &str = "metadata";
    /// Common wrapper key used by some plugin payload shapes.
    pub const DATA: &str = "data";
    pub const DIRECTORY: &str = "directory";
    pub const WORKTREE: &str = "worktree";

    // Chat hooks
    pub const MODEL: &str = "model";
    pub const ID: &str = "id";
    pub const INFO: &str = "info";
    pub const MODEL_ID: &str = "modelID";
    pub const PROVIDER: &str = "provider";
    pub const PROVIDER_ID: &str = "providerID";
    pub const SYSTEM: &str = "system";
    pub const MESSAGES: &str = "messages";
    pub const AGENT: &str = "agent";
    pub const MESSAGE: &str = "message";
    pub const TEMPERATURE: &str = "temperature";
    pub const TOP_P: &str = "topP";
    pub const TOP_K: &str = "topK";
    pub const OPTIONS: &str = "options";
    pub const MAX_TOKENS: &str = "maxTokens";
    pub const HEADERS: &str = "headers";
    pub const VARIANT: &str = "variant";
    pub const HAS_TOOL_CALLS: &str = "has_tool_calls";
    pub const PARTS: &str = "parts";

    // Session compaction hooks
    pub const AUTO: &str = "auto";
    pub const COMPLETED: &str = "completed";
    pub const CONTEXT: &str = "context";
    pub const PROMPT: &str = "prompt";

    // Text completion hooks
    pub const PART_ID: &str = "partID";
    pub const TEXT: &str = "text";

    // Shell hooks
    pub const CWD: &str = "cwd";
    pub const ENV: &str = "env";

    // Command hooks
    pub const COMMAND: &str = "command";
    pub const ARGUMENTS: &str = "arguments";
    pub const SOURCE: &str = "source";

    // Permission hooks
    pub const PERMISSION: &str = "permission";
    pub const PERMISSION_TYPE: &str = "permission_type";
    pub const PERMISSION_ID: &str = "permission_id";
    pub const STATUS: &str = "status";
}

/// Alternate key spellings accepted for normalization (legacy + convenience).
pub mod aliases {
    pub const TOOL_ID_SNAKE: &str = "tool_id";
    pub const CALL_ID_SNAKE: &str = "call_id";
    pub const MODEL_ID_SNAKE: &str = "model_id";
    pub const PROVIDER_ID_SNAKE: &str = "provider_id";
    pub const MESSAGE_ID_SNAKE: &str = "message_id";
    pub const PART_ID_SNAKE: &str = "part_id";
    pub const MAX_TOKENS_SNAKE: &str = "max_tokens";

    pub const PERMISSION_TYPE_CAMEL: &str = "permissionType";
    pub const PERMISSION_ID_CAMEL: &str = "permissionID";
}
