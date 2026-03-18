use strum_macros::EnumString;

/// Common argument keys used by built-in tool calls.
pub mod arg_keys {
    pub const QUESTIONS: &str = "questions";
    pub const ANSWERS: &str = "answers";
    pub const OPTIONS: &str = "options";
    pub const PARAMETERS: &str = "parameters";
    pub const RESULTS: &str = "results";
    pub const SUCCESS: &str = "success";
    pub const OUTPUT: &str = "output";
    pub const RESULT: &str = "result";
    pub const ERROR: &str = "error";
    pub const HEADER: &str = "header";
    pub const QUESTION: &str = "question";
    pub const COMMAND: &str = "command";
    pub const CMD: &str = "cmd";
    pub const SCRIPT: &str = "script";
    pub const INPUT: &str = "input";
    pub const FILE: &str = "file";
    pub const PATTERN: &str = "pattern";
    pub const URL: &str = "url";
    pub const QUERY: &str = "query";
    pub const PATH: &str = "path";
    pub const REPO: &str = "repo";
    pub const COMMIT: &str = "commit";
    pub const NUMBER: &str = "number";
    pub const STATE: &str = "state";
    pub const LANGUAGE: &str = "language";
    pub const BRANCH: &str = "branch";
    pub const DEPTH: &str = "depth";
    pub const LOCAL_ALIAS: &str = "local_alias";
    pub const LINE_START: &str = "line_start";
    pub const LINE_END: &str = "line_end";
    pub const VERSION: &str = "version";
    pub const SOURCE: &str = "source";
    pub const LIBRARY: &str = "library";
    pub const LIBRARY_ID: &str = "library_id";
    pub const PAGE_ID: &str = "page_id";
    pub const OPERATION: &str = "operation";
    pub const OFFSET: &str = "offset";
    pub const LIMIT: &str = "limit";
    pub const AGENT: &str = "agent";
    pub const CATEGORY: &str = "category";
    pub const DESCRIPTION: &str = "description";
    pub const PROMPT: &str = "prompt";
    pub const RUN_IN_BACKGROUND: &str = "run_in_background";
    pub const SYNC_TODO: &str = "sync_todo";
    pub const STATUS_FILTER: &str = "status_filter";
    pub const TODO_ITEM: &str = "todo_item";
    pub const SUBAGENT_TYPE: &str = "subagent_type";
    pub const SUBAGENT_TYPE_CAMEL: &str = "subagentType";
    pub const LOAD_SKILLS: &str = "load_skills";
    pub const LOADED_SKILLS: &str = "loadedSkills";
    pub const DELEGATED: &str = "delegated";
    pub const AGENT_TASK_ID: &str = "agentTaskId";
    pub const AGENT_TASK_ID_SNAKE: &str = "agent_task_id";
    pub const TASK: &str = "task";
    pub const TOOL_CALLS: &str = "tool_calls";
    pub const TOOL_CALLS_CAMEL: &str = "toolCalls";
    pub const TOOL: &str = "tool";
    pub const NAME: &str = "name";
    pub const TOOL_NAME: &str = "tool_name";
    pub const SKILL: &str = "skill";
    pub const PAGE: &str = "page";
    pub const MATCHES: &str = "matches";
    pub const TRUNCATED: &str = "truncated";
    pub const REGISTRY_PATH: &str = "registry_path";
    pub const BACKEND: &str = "backend";
    pub const STATUS: &str = "status";
    pub const COUNT: &str = "count";
    pub const HEAD: &str = "head";
    pub const COMMITS: &str = "commits";
    pub const BLAME: &str = "blame";
    pub const REPO_ROOT: &str = "repo_root";
    pub const SESSION: &str = "session";
    pub const FORMAT: &str = "format";
    pub const PARENT_DIR: &str = "parentDir";
    pub const REQUESTED_CURSOR: &str = "requestedCursor";
    pub const BUFFER_START: &str = "bufferStart";
    pub const START_CURSOR: &str = "startCursor";
    pub const END_CURSOR: &str = "endCursor";
    pub const TRUNCATED_REPLAY: &str = "truncatedReplay";
}

/// Canonical built-in tool identifier strings.
///
/// These values are used as tool call `name` strings across:
/// - runtime tool calls (server/orchestrator/agent)
/// - UI presenters (cli/tui/web) for special-casing rich views
///
/// Keep them stable — they are part of the wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum BuiltinToolName {
    #[strum(serialize = "question")]
    Question,

    #[strum(
        serialize = "todoread",
        serialize = "todo_read",
        serialize = "todoRead",
        serialize = "todo-read"
    )]
    TodoRead,

    #[strum(
        serialize = "todowrite",
        serialize = "todo_write",
        serialize = "todoWrite",
        serialize = "todo-write"
    )]
    TodoWrite,

    #[strum(serialize = "task")]
    Task,

    #[strum(
        serialize = "task_flow",
        serialize = "taskflow",
        serialize = "taskFlow"
    )]
    TaskFlow,

    #[strum(serialize = "edit", serialize = "editfile", serialize = "edit_file")]
    Edit,

    #[strum(
        serialize = "multiedit",
        serialize = "multi_edit",
        serialize = "multi-edit"
    )]
    MultiEdit,

    #[strum(serialize = "write", serialize = "writefile", serialize = "write_file")]
    Write,

    #[strum(serialize = "read", serialize = "readfile", serialize = "read_file")]
    Read,

    #[strum(serialize = "bash", serialize = "shell")]
    Bash,

    #[strum(serialize = "grep", serialize = "search", serialize = "ripgrep")]
    Grep,

    #[strum(serialize = "glob")]
    Glob,

    #[strum(
        serialize = "apply_patch",
        serialize = "applypatch",
        serialize = "apply-patch",
        serialize = "patch"
    )]
    ApplyPatch,

    #[strum(
        serialize = "webfetch",
        serialize = "web_fetch",
        serialize = "web-fetch",
        serialize = "fetch"
    )]
    WebFetch,

    #[strum(
        serialize = "websearch",
        serialize = "web_search",
        serialize = "web-search"
    )]
    WebSearch,

    #[strum(
        serialize = "codesearch",
        serialize = "code_search",
        serialize = "code-search"
    )]
    CodeSearch,

    #[strum(serialize = "lsp")]
    Lsp,

    #[strum(serialize = "skill")]
    Skill,

    #[strum(
        serialize = "ls",
        serialize = "list",
        serialize = "listdir",
        serialize = "list_dir",
        serialize = "list_directory"
    )]
    Ls,

    #[strum(
        serialize = "notebook_edit",
        serialize = "notebookedit",
        serialize = "notebook-edit"
    )]
    NotebookEdit,

    #[strum(serialize = "batch")]
    Batch,

    #[strum(
        serialize = "context_docs",
        serialize = "contextDocs",
        serialize = "context-docs"
    )]
    ContextDocs,

    #[strum(
        serialize = "github_research",
        serialize = "githubResearch",
        serialize = "github-research"
    )]
    GitHubResearch,

    #[strum(
        serialize = "repo_history",
        serialize = "repoHistory",
        serialize = "repo-history"
    )]
    RepoHistory,

    #[strum(
        serialize = "media_inspect",
        serialize = "mediaInspect",
        serialize = "media-inspect"
    )]
    MediaInspect,

    #[strum(
        serialize = "browser_session",
        serialize = "browserSession",
        serialize = "browser-session"
    )]
    BrowserSession,

    #[strum(
        serialize = "shell_session",
        serialize = "shellSession",
        serialize = "shell-session"
    )]
    ShellSession,

    #[strum(
        serialize = "ast_grep_search",
        serialize = "astGrepSearch",
        serialize = "ast-grep-search"
    )]
    AstGrepSearch,

    #[strum(
        serialize = "ast_grep_replace",
        serialize = "astGrepReplace",
        serialize = "ast-grep-replace"
    )]
    AstGrepReplace,

    #[strum(
        serialize = "plan_enter",
        serialize = "planEnter",
        serialize = "plan-enter"
    )]
    PlanEnter,

    #[strum(
        serialize = "plan_exit",
        serialize = "planExit",
        serialize = "plan-exit"
    )]
    PlanExit,

    #[strum(serialize = "invalid")]
    Invalid,
}

impl std::fmt::Display for BuiltinToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BuiltinToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::TodoRead => "todoread",
            Self::TodoWrite => "todowrite",
            Self::Task => "task",
            Self::TaskFlow => "task_flow",
            Self::Edit => "edit",
            Self::MultiEdit => "multiedit",
            Self::Write => "write",
            Self::Read => "read",
            Self::Bash => "bash",
            Self::Grep => "grep",
            Self::Glob => "glob",
            Self::ApplyPatch => "apply_patch",
            Self::WebFetch => "webfetch",
            Self::WebSearch => "websearch",
            Self::CodeSearch => "codesearch",
            Self::Lsp => "lsp",
            Self::Skill => "skill",
            Self::Ls => "ls",
            Self::NotebookEdit => "notebook_edit",
            Self::Batch => "batch",
            Self::ContextDocs => "context_docs",
            Self::GitHubResearch => "github_research",
            Self::RepoHistory => "repo_history",
            Self::MediaInspect => "media_inspect",
            Self::BrowserSession => "browser_session",
            Self::ShellSession => "shell_session",
            Self::AstGrepSearch => "ast_grep_search",
            Self::AstGrepReplace => "ast_grep_replace",
            Self::PlanEnter => "plan_enter",
            Self::PlanExit => "plan_exit",
            Self::Invalid => "invalid",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Question => "Question",
            Self::TodoRead => "TodoRead",
            Self::TodoWrite => "TodoWrite",
            Self::Task => "Task",
            Self::TaskFlow => "TaskFlow",
            Self::Edit => "Edit",
            Self::MultiEdit => "MultiEdit",
            Self::Write => "Write",
            Self::Read => "Read",
            Self::Bash => "Bash",
            Self::Grep => "Grep",
            Self::Glob => "Glob",
            Self::ApplyPatch => "ApplyPatch",
            Self::WebFetch => "WebFetch",
            Self::WebSearch => "WebSearch",
            Self::CodeSearch => "CodeSearch",
            Self::Lsp => "LSP",
            Self::Skill => "Skill",
            Self::Ls => "Ls",
            Self::NotebookEdit => "NotebookEdit",
            Self::Batch => "Batch",
            Self::ContextDocs => "ContextDocs",
            Self::GitHubResearch => "GitHubResearch",
            Self::RepoHistory => "RepoHistory",
            Self::MediaInspect => "MediaInspect",
            Self::BrowserSession => "BrowserSession",
            Self::ShellSession => "ShellSession",
            Self::AstGrepSearch => "AstGrepSearch",
            Self::AstGrepReplace => "AstGrepReplace",
            Self::PlanEnter => "PlanEnter",
            Self::PlanExit => "PlanExit",
            Self::Invalid => "Invalid",
        }
    }
}

/// Tool call status labels used across session history and UI projections.
///
/// Wire format: lowercase strings (`"pending"`, `"running"`, `"completed"`, `"error"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum ToolCallStatusWire {
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
    #[strum(serialize = "error", serialize = "failed", serialize = "failure")]
    Error,
}

impl std::fmt::Display for ToolCallStatusWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ToolCallStatusWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// `interaction.status` values for the built-in `question` tool UI contract.
///
/// Wire format: snake_case strings (`"pending"`, `"answered"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum QuestionInteractionStatus {
    Pending,
    Answered,
    Rejected,
    Cancelled,
    Error,
}

impl std::fmt::Display for QuestionInteractionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl QuestionInteractionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}
