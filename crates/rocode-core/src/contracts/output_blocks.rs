use strum_macros::EnumString;

/// Shared metadata keys used for structured output blocks and UI rendering.
pub mod keys {
    /// Generic identifier key for block-like records.
    pub const ID: &str = "id";
    /// Output block discriminant key.
    pub const KIND: &str = "kind";
    /// Output block tone key.
    pub const TONE: &str = "tone";
    /// Output block text key.
    pub const TEXT: &str = "text";
    /// Message/reasoning/tool phase key.
    pub const PHASE: &str = "phase";
    /// Message role key.
    pub const ROLE: &str = "role";
    /// Session event name key.
    pub const EVENT: &str = "event";
    /// Shared title key for event/stage blocks.
    pub const TITLE: &str = "title";
    /// Shared status key for event/stage blocks.
    pub const STATUS: &str = "status";
    /// Shared summary key for event/stage blocks.
    pub const SUMMARY: &str = "summary";
    /// Shared fields key for event/stage blocks.
    pub const FIELDS: &str = "fields";
    /// Shared body key for event/stage blocks.
    pub const BODY: &str = "body";
    /// Tool name key.
    pub const NAME: &str = "name";
    /// Tool detail key.
    pub const DETAIL: &str = "detail";
    /// Queue position key.
    pub const POSITION: &str = "position";
    /// Generic field label key for rendered key-value rows.
    pub const LABEL: &str = "label";
    /// Generic field value key for rendered key-value rows.
    pub const VALUE: &str = "value";
    /// Optional nested display payload key.
    pub const DISPLAY: &str = "display";
    /// Display header key.
    pub const HEADER: &str = "header";
    /// Optional display preview payload key.
    pub const PREVIEW: &str = "preview";
    /// Optional typed structured detail payload key.
    pub const STRUCTURED: &str = "structured";
    /// Generic truncated flag key for preview payloads.
    pub const TRUNCATED: &str = "truncated";
    /// Scheduler stage embedded decision payload key.
    pub const DECISION: &str = "decision";

    /// Metadata key for a one-line display summary.
    pub const DISPLAY_SUMMARY: &str = "display.summary";
    /// Metadata key for display fields (array of `{ key, value }` objects).
    pub const DISPLAY_FIELDS: &str = "display.fields";
    /// Metadata key for optional rich preview payload.
    pub const DISPLAY_PREVIEW: &str = "display.preview";
    /// Metadata key for display-mode overrides.
    pub const DISPLAY_MODE: &str = "display.mode";

    /// Key name inside `display.fields` objects for the field label.
    pub const DISPLAY_FIELD_KEY: &str = "key";
    /// Key name inside `display.fields` objects for the field value.
    pub const DISPLAY_FIELD_VALUE: &str = "value";

    /// Metadata key for tool interaction payload (question tool, permissions, etc).
    pub const INTERACTION: &str = "interaction";
    pub const INTERACTION_TYPE: &str = "type";
    pub const INTERACTION_STATUS: &str = "status";
    pub const INTERACTION_CAN_REPLY: &str = "can_reply";
    pub const INTERACTION_CAN_REJECT: &str = "can_reject";
}

/// Output-block JSON keys for `OutputBlock::SchedulerStage`.
///
/// These are the serialized field names of `rocode_content::output_blocks::SchedulerStageBlock`.
pub mod scheduler_stage_keys {
    pub const STAGE_ID: &str = "stage_id";
    pub const PROFILE: &str = "profile";
    pub const STAGE: &str = "stage";
    pub const STAGE_INDEX: &str = "stage_index";
    pub const STAGE_TOTAL: &str = "stage_total";
    pub const STEP: &str = "step";
    pub const FOCUS: &str = "focus";
    pub const LAST_EVENT: &str = "last_event";
    pub const WAITING_ON: &str = "waiting_on";
    pub const ACTIVITY: &str = "activity";
    pub const LOOP_BUDGET: &str = "loop_budget";
    pub const AVAILABLE_SKILL_COUNT: &str = "available_skill_count";
    pub const AVAILABLE_AGENT_COUNT: &str = "available_agent_count";
    pub const AVAILABLE_CATEGORY_COUNT: &str = "available_category_count";
    pub const ACTIVE_SKILLS: &str = "active_skills";
    pub const ACTIVE_AGENTS: &str = "active_agents";
    pub const ACTIVE_CATEGORIES: &str = "active_categories";
    pub const DONE_AGENT_COUNT: &str = "done_agent_count";
    pub const TOTAL_AGENT_COUNT: &str = "total_agent_count";
    pub const REASONING_TOKENS: &str = "reasoning_tokens";
    pub const CACHE_READ_TOKENS: &str = "cache_read_tokens";
    pub const CACHE_WRITE_TOKENS: &str = "cache_write_tokens";
    pub const CHILD_SESSION_ID: &str = "child_session_id";
}

/// JSON keys for embedded scheduler decision blocks inside output blocks.
pub mod scheduler_decision_keys {
    pub const KIND: &str = "kind";
    pub const TITLE: &str = "title";
    pub const SPEC: &str = "spec";
    pub const FIELDS: &str = "fields";
    pub const SECTIONS: &str = "sections";
}

/// JSON keys for `SchedulerDecisionRenderSpec`.
pub mod scheduler_decision_spec_keys {
    pub const VERSION: &str = "version";
    pub const SHOW_HEADER_DIVIDER: &str = "show_header_divider";
    pub const FIELD_ORDER: &str = "field_order";
    pub const FIELD_LABEL_EMPHASIS: &str = "field_label_emphasis";
    pub const STATUS_PALETTE: &str = "status_palette";
    pub const SECTION_SPACING: &str = "section_spacing";
    pub const UPDATE_POLICY: &str = "update_policy";
}

/// Canonical display mode override values used in `display.mode`.
///
/// Wire format: lowercase strings (`"block"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum DisplayModeWire {
    Block,
}

impl std::fmt::Display for DisplayModeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl DisplayModeWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical output block "kind" strings for the web/streaming contract.
///
/// These values are used in the `kind` field for `output_block` payloads and are
/// consumed by:
/// - `rocode-server` (producer)
/// - `rocode-cli` / `rocode-tui` (consumers)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum OutputBlockKind {
    Status,
    Message,
    Reasoning,
    Tool,
    SessionEvent,
    QueueItem,
    SchedulerStage,
    Inspect,
}

impl std::fmt::Display for OutputBlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl OutputBlockKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Message => "message",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
            Self::SessionEvent => "session_event",
            Self::QueueItem => "queue_item",
            Self::SchedulerStage => "scheduler_stage",
            Self::Inspect => "inspect",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical tone strings for status/message/field styling.
///
/// Wire format: lowercase strings (`"normal"`, `"warning"`, `"error"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum BlockToneWire {
    Title,
    Normal,
    Muted,
    Success,
    Warning,
    Error,
    Info,
    Status,
}

impl std::fmt::Display for BlockToneWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BlockToneWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Normal => "normal",
            Self::Muted => "muted",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Info => "info",
            Self::Status => "status",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical phase strings for streaming message/reasoning blocks.
///
/// Wire format: lowercase strings (`"start"`, `"delta"`, `"end"`, `"full"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum MessagePhaseWire {
    Start,
    Delta,
    End,
    Full,
}

impl std::fmt::Display for MessagePhaseWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MessagePhaseWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Delta => "delta",
            Self::End => "end",
            Self::Full => "full",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical phase strings for tool blocks.
///
/// Wire format: lowercase strings (`"start"`, `"running"`, `"done"`, `"error"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum ToolPhaseWire {
    #[strum(serialize = "start")]
    Start,
    #[strum(serialize = "running")]
    Running,
    #[strum(serialize = "done", serialize = "result")]
    Done,
    #[strum(serialize = "error")]
    Error,
}

impl std::fmt::Display for ToolPhaseWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ToolPhaseWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Running => "running",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical structured detail "type" strings for tool blocks.
///
/// Wire format: snake_case strings (e.g. `"file_edit"`, `"bash_exec"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum ToolStructuredDetailTypeWire {
    FileEdit,
    FileWrite,
    FileRead,
    BashExec,
    Search,
    Generic,
}

impl std::fmt::Display for ToolStructuredDetailTypeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ToolStructuredDetailTypeWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FileEdit => "file_edit",
            Self::FileWrite => "file_write",
            Self::FileRead => "file_read",
            Self::BashExec => "bash_exec",
            Self::Search => "search",
            Self::Generic => "generic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical preview kind strings surfaced in `display.preview.kind`.
///
/// Wire format: lowercase strings (`"diff"`, `"code"`, `"text"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum DisplayPreviewKindWire {
    Diff,
    Code,
    Text,
}

impl std::fmt::Display for DisplayPreviewKindWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl DisplayPreviewKindWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Diff => "diff",
            Self::Code => "code",
            Self::Text => "text",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}
