use crate::stage_protocol::{parse_step_limit_from_budget, StageStatus, StageSummary};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlockTone {
    Title,
    Muted,
    Success,
    Warning,
    Error,
    #[default]
    #[serde(other)]
    Normal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusBlock {
    #[serde(default)]
    pub tone: BlockTone,
    #[serde(default)]
    pub text: String,
}

impl StatusBlock {
    pub fn title(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Title,
            text: text.into(),
        }
    }

    pub fn normal(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Normal,
            text: text.into(),
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Muted,
            text: text.into(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Success,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            tone: BlockTone::Error,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    System,
    #[default]
    #[serde(other)]
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MessagePhase {
    Start,
    End,
    Full,
    #[default]
    #[serde(other)]
    Delta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningBlock {
    #[serde(default)]
    pub phase: MessagePhase,
    #[serde(default)]
    pub text: String,
}

impl ReasoningBlock {
    pub fn start() -> Self {
        Self {
            phase: MessagePhase::Start,
            text: String::new(),
        }
    }

    pub fn delta(text: impl Into<String>) -> Self {
        Self {
            phase: MessagePhase::Delta,
            text: text.into(),
        }
    }

    pub fn end() -> Self {
        Self {
            phase: MessagePhase::End,
            text: String::new(),
        }
    }

    pub fn full(text: impl Into<String>) -> Self {
        Self {
            phase: MessagePhase::Full,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageBlock {
    #[serde(default)]
    pub role: MessageRole,
    #[serde(default)]
    pub phase: MessagePhase,
    #[serde(default)]
    pub text: String,
}

impl MessageBlock {
    pub fn start(role: MessageRole) -> Self {
        Self {
            role,
            phase: MessagePhase::Start,
            text: String::new(),
        }
    }

    pub fn delta(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Delta,
            text: text.into(),
        }
    }

    pub fn end(role: MessageRole) -> Self {
        Self {
            role,
            phase: MessagePhase::End,
            text: String::new(),
        }
    }

    pub fn full(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Full,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolPhase {
    Start,
    #[serde(alias = "result")]
    Done,
    Error,
    #[default]
    #[serde(other)]
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolStructuredDetail {
    FileEdit {
        file_path: String,
        diff_preview: Option<String>,
    },
    FileWrite {
        file_path: String,
        bytes: Option<u64>,
        lines: Option<u64>,
        diff_preview: Option<String>,
    },
    FileRead {
        file_path: String,
        total_lines: Option<u64>,
        truncated: bool,
    },
    BashExec {
        command_preview: String,
        exit_code: Option<i64>,
        output_preview: Option<String>,
        truncated: bool,
    },
    Search {
        pattern: String,
        matches: Option<u64>,
        truncated: bool,
    },
    #[serde(other)]
    Generic,
}

fn default_tool_block_name() -> String {
    "tool".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolBlock {
    #[serde(default = "default_tool_block_name")]
    pub name: String,
    #[serde(default)]
    pub phase: ToolPhase,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub structured: Option<ToolStructuredDetail>,
}

impl ToolBlock {
    pub fn start(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Start,
            detail: None,
            structured: None,
        }
    }

    pub fn running(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Running,
            detail: Some(detail.into()),
            structured: None,
        }
    }

    pub fn done(name: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Done,
            detail,
            structured: None,
        }
    }

    pub fn error(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phase: ToolPhase::Error,
            detail: Some(detail.into()),
            structured: None,
        }
    }

    pub fn with_structured(mut self, detail: ToolStructuredDetail) -> Self {
        self.structured = Some(detail);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerDecisionField {
    pub label: String,
    pub value: String,
    pub tone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerDecisionSection {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerDecisionRenderSpec {
    pub version: String,
    pub show_header_divider: bool,
    pub field_order: String,
    pub field_label_emphasis: String,
    pub status_palette: String,
    pub section_spacing: String,
    pub update_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerDecisionBlock {
    pub kind: String,
    pub title: String,
    pub spec: SchedulerDecisionRenderSpec,
    pub fields: Vec<SchedulerDecisionField>,
    pub sections: Vec<SchedulerDecisionSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEventField {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub tone: Option<String>,
}

fn default_session_event_name() -> String {
    "event".to_string()
}

fn default_session_event_title() -> String {
    "Session Event".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEventBlock {
    #[serde(default = "default_session_event_name")]
    pub event: String,
    #[serde(default = "default_session_event_title")]
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub fields: Vec<SessionEventField>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItemBlock {
    pub position: usize,
    pub text: String,
}

pub fn default_scheduler_decision_render_spec() -> SchedulerDecisionRenderSpec {
    SchedulerDecisionRenderSpec {
        version: "decision-card/v1".to_string(),
        show_header_divider: true,
        field_order: "as-provided".to_string(),
        field_label_emphasis: "bold".to_string(),
        status_palette: "semantic".to_string(),
        section_spacing: "loose".to_string(),
        update_policy: "stable-shell-live-runtime-append-decision".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerStageBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
    pub profile: Option<String>,
    pub stage: String,
    pub title: String,
    pub text: String,
    pub stage_index: Option<u64>,
    pub stage_total: Option<u64>,
    pub step: Option<u64>,
    pub status: Option<String>,
    pub focus: Option<String>,
    pub last_event: Option<String>,
    pub waiting_on: Option<String>,
    pub activity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_budget: Option<String>,
    pub available_skill_count: Option<u64>,
    pub available_agent_count: Option<u64>,
    pub available_category_count: Option<u64>,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_agents: Vec<String>,
    #[serde(default)]
    pub active_categories: Vec<String>,
    #[serde(default)]
    pub done_agent_count: u32,
    #[serde(default)]
    pub total_agent_count: u32,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub decision: Option<SchedulerDecisionBlock>,
    pub child_session_id: Option<String>,
}

impl SchedulerStageBlock {
    pub fn from_metadata(
        text: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> Option<Self> {
        #[derive(Debug, Default, Deserialize)]
        struct SchedulerStageMetadataWire {
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_id: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            resolved_scheduler_profile: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_profile: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_index: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_total: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_step: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_status: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_focus: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_last_event: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_waiting_on: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_activity: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_loop_budget: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_prompt_tokens: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_completion_tokens: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_reasoning_tokens: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_cache_read_tokens: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_cache_write_tokens: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_string_trimmed")]
            scheduler_stage_child_session_id: Option<String>,
            #[serde(default, deserialize_with = "deserialize_vec_string_lossy")]
            scheduler_stage_active_skills: Vec<String>,
            #[serde(default, deserialize_with = "deserialize_vec_string_lossy")]
            scheduler_stage_active_agents: Vec<String>,
            #[serde(default, deserialize_with = "deserialize_vec_string_lossy")]
            scheduler_stage_active_categories: Vec<String>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_skill_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_agent_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_category_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_done_agent_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_total_agent_count: Option<u64>,
        }

        fn deserialize_opt_string_trimmed<'de, D>(
            deserializer: D,
        ) -> Result<Option<String>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = Option::<serde_json::Value>::deserialize(deserializer)?;
            Ok(match value {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(value)) => {
                    let trimmed = value.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                }
                Some(serde_json::Value::Number(value)) => Some(value.to_string()),
                Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
                _ => None,
            })
        }

        fn deserialize_opt_u64_lossy<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = Option::<serde_json::Value>::deserialize(deserializer)?;
            Ok(match value {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::Number(value)) => value.as_u64(),
                Some(serde_json::Value::String(value)) => value.trim().parse().ok(),
                _ => None,
            })
        }

        fn deserialize_vec_string_lossy<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = Option::<serde_json::Value>::deserialize(deserializer)?;
            let Some(serde_json::Value::Array(values)) = value else {
                return Ok(Vec::new());
            };
            Ok(values
                .into_iter()
                .filter_map(|value| value.as_str().map(String::from))
                .collect())
        }

        let wire = serde_json::to_value(metadata)
            .ok()
            .and_then(|value| serde_json::from_value::<SchedulerStageMetadataWire>(value).ok())
            .unwrap_or_default();

        let stage = wire.scheduler_stage?;
        let stage_id = wire.scheduler_stage_id;
        let profile = wire.resolved_scheduler_profile.or(wire.scheduler_profile);
        let stage_index = wire.scheduler_stage_index;
        let stage_total = wire.scheduler_stage_total;
        let step = wire.scheduler_stage_step;
        let status = wire.scheduler_stage_status;
        let focus = wire.scheduler_stage_focus;
        let last_event = wire.scheduler_stage_last_event;
        let waiting_on = wire.scheduler_stage_waiting_on;
        let activity = wire.scheduler_stage_activity;
        let loop_budget = wire.scheduler_stage_loop_budget;
        let prompt_tokens = wire.scheduler_stage_prompt_tokens;
        let completion_tokens = wire.scheduler_stage_completion_tokens;
        let reasoning_tokens = wire.scheduler_stage_reasoning_tokens;
        let cache_read_tokens = wire.scheduler_stage_cache_read_tokens;
        let cache_write_tokens = wire.scheduler_stage_cache_write_tokens;
        let child_session_id = wire.scheduler_stage_child_session_id;

        let active_skills = wire.scheduler_stage_active_skills;
        let active_agents = wire.scheduler_stage_active_agents;
        let active_categories = wire.scheduler_stage_active_categories;

        let available_skill_count = wire.scheduler_stage_available_skill_count;
        let available_agent_count = wire.scheduler_stage_available_agent_count;
        let available_category_count = wire.scheduler_stage_available_category_count;
        let done_agent_count = wire.scheduler_stage_done_agent_count.unwrap_or(0) as u32;
        let total_agent_count = wire.scheduler_stage_total_agent_count.unwrap_or(0) as u32;

        let (title, body) = if let Some(rest) = text.trim().strip_prefix("## ") {
            if let Some((heading, after)) = rest.split_once('\n') {
                (heading.trim().to_string(), after.trim_start().to_string())
            } else {
                (rest.trim().to_string(), String::new())
            }
        } else {
            (String::new(), text.to_string())
        };

        Some(Self {
            stage_id,
            profile,
            stage,
            title,
            text: body,
            stage_index,
            stage_total,
            step,
            status,
            focus,
            last_event,
            waiting_on,
            activity,
            loop_budget,
            available_skill_count,
            available_agent_count,
            available_category_count,
            active_skills,
            active_agents,
            active_categories,
            done_agent_count,
            total_agent_count,
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            cache_read_tokens,
            cache_write_tokens,
            decision: None,
            child_session_id,
        })
    }

    pub fn to_summary(&self) -> StageSummary {
        StageSummary {
            stage_id: self.stage_id.clone().unwrap_or_default(),
            stage_name: self.stage.clone(),
            index: self.stage_index,
            total: self.stage_total,
            step: self.step,
            step_total: parse_step_limit_from_budget(self.loop_budget.as_deref()),
            status: StageStatus::from_str_lossy(self.status.as_deref()),
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            reasoning_tokens: self.reasoning_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            focus: self.focus.clone(),
            last_event: self.last_event.clone(),
            active_agent_count: self.active_agents.len() as u32,
            active_tool_count: 0,
            child_session_count: if self.child_session_id.is_some() {
                1
            } else {
                0
            },
            primary_child_session_id: self.child_session_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectBlock {
    #[serde(default)]
    pub stage_ids: Vec<String>,
    #[serde(default)]
    pub events: Vec<InspectEventRow>,
    #[serde(default)]
    pub filter_stage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectEventRow {
    pub ts: i64,
    pub event_type: String,
    pub execution_id: Option<String>,
    pub stage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputBlock {
    Status(StatusBlock),
    Message(MessageBlock),
    Reasoning(ReasoningBlock),
    Tool(ToolBlock),
    SessionEvent(SessionEventBlock),
    QueueItem(QueueItemBlock),
    SchedulerStage(Box<SchedulerStageBlock>),
    Inspect(InspectBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OutputBlockWire {
    Status {
        #[serde(flatten)]
        block: StatusBlock,
    },
    Message {
        #[serde(flatten)]
        block: MessageBlock,
    },
    Reasoning {
        #[serde(flatten)]
        block: ReasoningBlock,
    },
    Tool {
        #[serde(flatten)]
        block: ToolBlock,
    },
    SessionEvent {
        #[serde(flatten)]
        block: SessionEventBlock,
    },
    QueueItem {
        #[serde(flatten)]
        block: QueueItemBlock,
    },
    SchedulerStage {
        #[serde(flatten)]
        block: SchedulerStageBlock,
    },
    Inspect {
        #[serde(flatten)]
        block: InspectBlock,
    },
}

impl<'de> Deserialize<'de> for OutputBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = OutputBlockWire::deserialize(deserializer)?;
        Ok(match wire {
            OutputBlockWire::Status { block } => Self::Status(block),
            OutputBlockWire::Message { block } => Self::Message(block),
            OutputBlockWire::Reasoning { block } => Self::Reasoning(block),
            OutputBlockWire::Tool { block } => Self::Tool(block),
            OutputBlockWire::SessionEvent { block } => Self::SessionEvent(block),
            OutputBlockWire::QueueItem { block } => Self::QueueItem(block),
            OutputBlockWire::SchedulerStage { block } => Self::SchedulerStage(Box::new(block)),
            OutputBlockWire::Inspect { block } => Self::Inspect(block),
        })
    }
}
