use crate::stage_protocol::{parse_step_limit_from_budget, StageStatus, StageSummary};
use rocode_core::contracts::scheduler::keys as scheduler_keys;
use rocode_core::contracts::scheduler::{
    SchedulerDecisionFieldLabelEmphasis, SchedulerDecisionFieldOrder,
    SchedulerDecisionRenderSpecVersion, SchedulerDecisionSectionSpacing,
    SchedulerDecisionStatusPalette, SchedulerDecisionUpdatePolicy,
};
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
pub enum MessageRole {
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
    pub role: MessageRole,
    pub phase: MessagePhase,
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
        version: SchedulerDecisionRenderSpecVersion::DecisionCardV1
            .as_str()
            .to_string(),
        show_header_divider: true,
        field_order: SchedulerDecisionFieldOrder::AsProvided.as_str().to_string(),
        field_label_emphasis: SchedulerDecisionFieldLabelEmphasis::Bold.as_str().to_string(),
        status_palette: SchedulerDecisionStatusPalette::Semantic.as_str().to_string(),
        section_spacing: SchedulerDecisionSectionSpacing::Loose.as_str().to_string(),
        update_policy: SchedulerDecisionUpdatePolicy::StableShellLiveRuntimeAppendDecision
            .as_str()
            .to_string(),
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

impl SchedulerStageBlock {
    pub fn from_metadata(
        text: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> Option<Self> {
        let stage = metadata.get(scheduler_keys::STAGE)?.as_str()?.to_string();
        let stage_id = metadata
            .get(scheduler_keys::STAGE_ID)
            .and_then(|v| v.as_str())
            .map(String::from);
        let profile = metadata
            .get(scheduler_keys::RESOLVED_PROFILE)
            .or_else(|| metadata.get(scheduler_keys::PROFILE))
            .and_then(|v| v.as_str())
            .map(String::from);
        let stage_index = metadata
            .get(scheduler_keys::STAGE_INDEX)
            .and_then(|v| v.as_u64());
        let stage_total = metadata
            .get(scheduler_keys::STAGE_TOTAL)
            .and_then(|v| v.as_u64());
        let step = metadata.get(scheduler_keys::STEP).and_then(|v| v.as_u64());
        let status = metadata
            .get(scheduler_keys::STATUS)
            .and_then(|v| v.as_str())
            .map(String::from);
        let focus = metadata
            .get(scheduler_keys::FOCUS)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from);
        let last_event = metadata
            .get(scheduler_keys::LAST_EVENT)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from);
        let waiting_on = metadata
            .get(scheduler_keys::WAITING_ON)
            .and_then(|v| v.as_str())
            .map(String::from);
        let activity = metadata
            .get(scheduler_keys::ACTIVITY)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from);
        let loop_budget = metadata
            .get(scheduler_keys::LOOP_BUDGET)
            .and_then(|v| v.as_str())
            .map(String::from);
        let prompt_tokens = metadata
            .get(scheduler_keys::PROMPT_TOKENS)
            .and_then(|v| v.as_u64());
        let completion_tokens = metadata
            .get(scheduler_keys::COMPLETION_TOKENS)
            .and_then(|v| v.as_u64());
        let reasoning_tokens = metadata
            .get(scheduler_keys::REASONING_TOKENS)
            .and_then(|v| v.as_u64());
        let cache_read_tokens = metadata
            .get(scheduler_keys::CACHE_READ_TOKENS)
            .and_then(|v| v.as_u64());
        let cache_write_tokens = metadata
            .get(scheduler_keys::CACHE_WRITE_TOKENS)
            .and_then(|v| v.as_u64());
        let child_session_id = metadata
            .get(scheduler_keys::CHILD_SESSION_ID)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from);

        let extract_string_array = |key: &str| -> Vec<String> {
            metadata
                .get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let active_skills = extract_string_array(scheduler_keys::ACTIVE_SKILLS);
        let active_agents = extract_string_array(scheduler_keys::ACTIVE_AGENTS);
        let active_categories = extract_string_array(scheduler_keys::ACTIVE_CATEGORIES);

        let available_skill_count = metadata
            .get(scheduler_keys::AVAILABLE_SKILL_COUNT)
            .and_then(|v| v.as_u64());
        let available_agent_count = metadata
            .get(scheduler_keys::AVAILABLE_AGENT_COUNT)
            .and_then(|v| v.as_u64());
        let available_category_count = metadata
            .get(scheduler_keys::AVAILABLE_CATEGORY_COUNT)
            .and_then(|v| v.as_u64());
        let done_agent_count = metadata
            .get(scheduler_keys::DONE_AGENT_COUNT)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let total_agent_count = metadata
            .get(scheduler_keys::TOTAL_AGENT_COUNT)
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

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
