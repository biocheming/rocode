use crate::stage_protocol::{parse_step_limit_from_budget, StageStatus, StageSummary};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockTone {
    Title,
    Normal,
    Muted,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBlock {
    pub tone: BlockTone,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePhase {
    Start,
    Delta,
    End,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningBlock {
    pub phase: MessagePhase,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageBlock {
    pub role: Role,
    pub phase: MessagePhase,
    pub text: String,
}

impl MessageBlock {
    pub fn start(role: Role) -> Self {
        Self {
            role,
            phase: MessagePhase::Start,
            text: String::new(),
        }
    }

    pub fn delta(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Delta,
            text: text.into(),
        }
    }

    pub fn end(role: Role) -> Self {
        Self {
            role,
            phase: MessagePhase::End,
            text: String::new(),
        }
    }

    pub fn full(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            phase: MessagePhase::Full,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPhase {
    Start,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBlock {
    pub name: String,
    pub phase: ToolPhase,
    pub detail: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEventField {
    pub label: String,
    pub value: String,
    pub tone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEventBlock {
    pub event: String,
    pub title: String,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub fields: Vec<SessionEventField>,
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
    pub active_skills: Vec<String>,
    pub active_agents: Vec<String>,
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

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        _ => None,
    })
}

fn deserialize_opt_u64_lossy<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(value)) => value.as_u64(),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn deserialize_u32_lossy_default<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(value)) => value.as_u64().unwrap_or(0) as u32,
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<u32>().unwrap_or(0),
        _ => 0,
    })
}

fn deserialize_string_vec_lossy<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        serde_json::Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|value| match value {
                serde_json::Value::String(value) => Some(value),
                _ => None,
            })
            .collect()),
        _ => Ok(Vec::new()),
    }
}

impl SchedulerStageBlock {
    pub fn from_metadata(
        text: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> Option<Self> {
        #[derive(Debug, Deserialize, Default)]
        struct SchedulerStageMetadataWire {
            #[serde(
                default,
                rename = "scheduler_stage",
                deserialize_with = "deserialize_opt_string_lossy"
            )]
            stage: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_id: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            resolved_scheduler_profile: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_profile: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_index: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_total: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_step: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_status: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_focus: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_last_event: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_waiting_on: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_activity: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
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
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            scheduler_stage_child_session_id: Option<String>,

            #[serde(default, deserialize_with = "deserialize_string_vec_lossy")]
            scheduler_stage_active_skills: Vec<String>,
            #[serde(default, deserialize_with = "deserialize_string_vec_lossy")]
            scheduler_stage_active_agents: Vec<String>,
            #[serde(default, deserialize_with = "deserialize_string_vec_lossy")]
            scheduler_stage_active_categories: Vec<String>,

            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_skill_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_agent_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
            scheduler_stage_available_category_count: Option<u64>,
            #[serde(default, deserialize_with = "deserialize_u32_lossy_default")]
            scheduler_stage_done_agent_count: u32,
            #[serde(default, deserialize_with = "deserialize_u32_lossy_default")]
            scheduler_stage_total_agent_count: u32,
        }

        let map: serde_json::Map<String, serde_json::Value> = metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let parsed: SchedulerStageMetadataWire =
            serde_json::from_value(serde_json::Value::Object(map)).ok()?;

        let stage = parsed.stage?;
        let stage_id = parsed.scheduler_stage_id;
        let profile = parsed
            .resolved_scheduler_profile
            .or(parsed.scheduler_profile);
        let stage_index = parsed.scheduler_stage_index;
        let stage_total = parsed.scheduler_stage_total;
        let step = parsed.scheduler_stage_step;
        let status = parsed.scheduler_stage_status;
        let focus = parsed
            .scheduler_stage_focus
            .filter(|s| !s.trim().is_empty());
        let last_event = parsed
            .scheduler_stage_last_event
            .filter(|s| !s.trim().is_empty());
        let waiting_on = parsed.scheduler_stage_waiting_on;
        let activity = parsed
            .scheduler_stage_activity
            .filter(|s| !s.trim().is_empty());
        let loop_budget = parsed.scheduler_stage_loop_budget;
        let prompt_tokens = parsed.scheduler_stage_prompt_tokens;
        let completion_tokens = parsed.scheduler_stage_completion_tokens;
        let reasoning_tokens = parsed.scheduler_stage_reasoning_tokens;
        let cache_read_tokens = parsed.scheduler_stage_cache_read_tokens;
        let cache_write_tokens = parsed.scheduler_stage_cache_write_tokens;
        let child_session_id = parsed
            .scheduler_stage_child_session_id
            .filter(|s| !s.trim().is_empty());

        let active_skills = parsed.scheduler_stage_active_skills;
        let active_agents = parsed.scheduler_stage_active_agents;
        let active_categories = parsed.scheduler_stage_active_categories;

        let available_skill_count = parsed.scheduler_stage_available_skill_count;
        let available_agent_count = parsed.scheduler_stage_available_agent_count;
        let available_category_count = parsed.scheduler_stage_available_category_count;
        let done_agent_count = parsed.scheduler_stage_done_agent_count;
        let total_agent_count = parsed.scheduler_stage_total_agent_count;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectBlock {
    pub stage_ids: Vec<String>,
    pub events: Vec<InspectEventRow>,
    pub filter_stage_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
