use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

pub mod keys {
    pub const STAGE: &str = "scheduler_stage";
    pub const STAGE_ID: &str = "scheduler_stage_id";
    pub const STAGE_TITLE: &str = "scheduler_stage_title";
    pub const PROFILE: &str = "scheduler_profile";
    pub const RESOLVED_PROFILE: &str = "resolved_scheduler_profile";

    pub const EMITTED: &str = "scheduler_stage_emitted";
    pub const AGENT: &str = "scheduler_stage_agent";
    pub const STREAMING: &str = "scheduler_stage_streaming";

    pub const STAGE_INDEX: &str = "scheduler_stage_index";
    pub const STAGE_TOTAL: &str = "scheduler_stage_total";
    pub const STEP: &str = "scheduler_stage_step";

    pub const STATUS: &str = "scheduler_stage_status";
    pub const FOCUS: &str = "scheduler_stage_focus";
    pub const LAST_EVENT: &str = "scheduler_stage_last_event";
    pub const WAITING_ON: &str = "scheduler_stage_waiting_on";
    pub const ACTIVITY: &str = "scheduler_stage_activity";
    pub const LOOP_BUDGET: &str = "scheduler_stage_loop_budget";

    pub const PROJECTION: &str = "scheduler_stage_projection";
    pub const TOOL_POLICY: &str = "scheduler_stage_tool_policy";

    pub const PROMPT_TOKENS: &str = "scheduler_stage_prompt_tokens";
    pub const COMPLETION_TOKENS: &str = "scheduler_stage_completion_tokens";
    pub const REASONING_TOKENS: &str = "scheduler_stage_reasoning_tokens";
    pub const CACHE_READ_TOKENS: &str = "scheduler_stage_cache_read_tokens";
    pub const CACHE_WRITE_TOKENS: &str = "scheduler_stage_cache_write_tokens";

    pub const CHILD_SESSION_ID: &str = "scheduler_stage_child_session_id";
    pub const DECISION: &str = "scheduler_stage_decision";

    pub const ACTIVE_SKILLS: &str = "scheduler_stage_active_skills";
    pub const ACTIVE_AGENTS: &str = "scheduler_stage_active_agents";
    pub const ACTIVE_CATEGORIES: &str = "scheduler_stage_active_categories";

    pub const AVAILABLE_SKILL_COUNT: &str = "scheduler_stage_available_skill_count";
    pub const AVAILABLE_AGENT_COUNT: &str = "scheduler_stage_available_agent_count";
    pub const AVAILABLE_CATEGORY_COUNT: &str = "scheduler_stage_available_category_count";

    pub const DONE_AGENT_COUNT: &str = "scheduler_stage_done_agent_count";
    pub const TOTAL_AGENT_COUNT: &str = "scheduler_stage_total_agent_count";

    /// Scheduler run metadata: total orchestrator steps taken.
    pub const SCHEDULER_STEPS: &str = "scheduler_steps";
    /// Scheduler run metadata: tool calls performed during orchestration.
    pub const SCHEDULER_TOOL_CALLS: &str = "scheduler_tool_calls";
}

pub mod decision_keys {
    pub const KIND: &str = "scheduler_decision_kind";
    pub const TITLE: &str = "scheduler_decision_title";
    pub const SPEC: &str = "scheduler_decision_spec";
    pub const FIELDS: &str = "scheduler_decision_fields";
    pub const SECTIONS: &str = "scheduler_decision_sections";
}

pub mod gate_keys {
    pub const STATUS: &str = "scheduler_gate_status";
    pub const SUMMARY: &str = "scheduler_gate_summary";
    pub const NEXT_INPUT: &str = "scheduler_gate_next_input";
    pub const FINAL_RESPONSE: &str = "scheduler_gate_final_response";
}

/// Canonical scheduler stage names (wire format).
pub mod stage_names {
    pub const REQUEST_ANALYSIS: &str = "request-analysis";
    pub const ROUTE: &str = "route";
    pub const INTERVIEW: &str = "interview";
    pub const PLAN: &str = "plan";
    pub const DELEGATION: &str = "delegation";
    pub const REVIEW: &str = "review";
    pub const EXECUTION_ORCHESTRATION: &str = "execution-orchestration";
    pub const SYNTHESIS: &str = "synthesis";
    pub const HANDOFF: &str = "handoff";

    // Internal execution stages (multi-agent / verification loops)
    pub const SINGLE_PASS_EXECUTOR: &str = "single-pass-executor";
    pub const COORDINATION_VERIFICATION: &str = "coordination-verification";
    pub const COORDINATION_GATE: &str = "coordination-gate";
    pub const COORDINATION_RETRY: &str = "coordination-retry";
    pub const AUTONOMOUS_VERIFICATION: &str = "autonomous-verification";
    pub const AUTONOMOUS_GATE: &str = "autonomous-gate";
    pub const AUTONOMOUS_RETRY: &str = "autonomous-retry";
}

/// Scheduler stage name.
///
/// Wire format: kebab-case strings (e.g. `"execution-orchestration"`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum SchedulerStageName {
    RequestAnalysis,
    Route,
    Interview,
    Plan,
    Delegation,
    Review,
    ExecutionOrchestration,
    Synthesis,
    Handoff,
    SinglePassExecutor,
    CoordinationVerification,
    CoordinationGate,
    CoordinationRetry,
    AutonomousVerification,
    AutonomousGate,
    AutonomousRetry,
}

impl std::fmt::Display for SchedulerStageName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerStageName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequestAnalysis => stage_names::REQUEST_ANALYSIS,
            Self::Route => stage_names::ROUTE,
            Self::Interview => stage_names::INTERVIEW,
            Self::Plan => stage_names::PLAN,
            Self::Delegation => stage_names::DELEGATION,
            Self::Review => stage_names::REVIEW,
            Self::ExecutionOrchestration => stage_names::EXECUTION_ORCHESTRATION,
            Self::Synthesis => stage_names::SYNTHESIS,
            Self::Handoff => stage_names::HANDOFF,
            Self::SinglePassExecutor => stage_names::SINGLE_PASS_EXECUTOR,
            Self::CoordinationVerification => stage_names::COORDINATION_VERIFICATION,
            Self::CoordinationGate => stage_names::COORDINATION_GATE,
            Self::CoordinationRetry => stage_names::COORDINATION_RETRY,
            Self::AutonomousVerification => stage_names::AUTONOMOUS_VERIFICATION,
            Self::AutonomousGate => stage_names::AUTONOMOUS_GATE,
            Self::AutonomousRetry => stage_names::AUTONOMOUS_RETRY,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.contains('_') {
            return trimmed.replace('_', "-").parse().ok();
        }
        trimmed.parse().ok()
    }
}

/// Scheduler decision kind surfaced in message metadata.
///
/// Wire format: lowercase strings (`"route"`, `"gate"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerDecisionKind {
    Route,
    Gate,
}

impl std::fmt::Display for SchedulerDecisionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Route => "route",
            Self::Gate => "gate",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler stage runtime status.
///
/// Wire format: lowercase strings (e.g. `"running"`, `"waiting"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerStageStatus {
    Running,
    Waiting,
    Cancelling,
    Cancelled,
    Done,
    Blocked,
    #[strum(serialize = "retry")]
    Retrying,
}

impl std::fmt::Display for SchedulerStageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerStageStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Retrying => "retrying",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// What the scheduler stage is currently waiting on.
///
/// Wire format: lowercase strings (`"user"`, `"tool"`, `"model"`, `"none"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerStageWaitingOn {
    User,
    Tool,
    Model,
    None,
}

impl std::fmt::Display for SchedulerStageWaitingOn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerStageWaitingOn {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Tool => "tool",
            Self::Model => "model",
            Self::None => "none",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec version identifiers.
///
/// Wire format: opaque, stable strings (e.g. `"decision-card/v1"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum SchedulerDecisionRenderSpecVersion {
    #[strum(serialize = "decision-card/v1")]
    DecisionCardV1,
}

impl std::fmt::Display for SchedulerDecisionRenderSpecVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionRenderSpecVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecisionCardV1 => "decision-card/v1",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec "field_order" values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum SchedulerDecisionFieldOrder {
    #[strum(serialize = "as-provided")]
    AsProvided,
}

impl std::fmt::Display for SchedulerDecisionFieldOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionFieldOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AsProvided => "as-provided",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec "field_label_emphasis" values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerDecisionFieldLabelEmphasis {
    Bold,
    Normal,
}

impl std::fmt::Display for SchedulerDecisionFieldLabelEmphasis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionFieldLabelEmphasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Normal => "normal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec "status_palette" values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerDecisionStatusPalette {
    Semantic,
}

impl std::fmt::Display for SchedulerDecisionStatusPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionStatusPalette {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec "section_spacing" values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum SchedulerDecisionSectionSpacing {
    Loose,
    Tight,
}

impl std::fmt::Display for SchedulerDecisionSectionSpacing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionSectionSpacing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Loose => "loose",
            Self::Tight => "tight",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Scheduler decision render spec "update_policy" values.
///
/// These are intentionally verbose because they describe long-lived UI behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum SchedulerDecisionUpdatePolicy {
    #[strum(serialize = "stable-shell-live-runtime-append-decision")]
    StableShellLiveRuntimeAppendDecision,
}

impl std::fmt::Display for SchedulerDecisionUpdatePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SchedulerDecisionUpdatePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableShellLiveRuntimeAppendDecision => {
                "stable-shell-live-runtime-append-decision"
            }
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_stage_name_round_trips() {
        let cases: &[(&str, SchedulerStageName)] = &[
            (
                stage_names::REQUEST_ANALYSIS,
                SchedulerStageName::RequestAnalysis,
            ),
            (stage_names::ROUTE, SchedulerStageName::Route),
            (stage_names::INTERVIEW, SchedulerStageName::Interview),
            (stage_names::PLAN, SchedulerStageName::Plan),
            (stage_names::DELEGATION, SchedulerStageName::Delegation),
            (stage_names::REVIEW, SchedulerStageName::Review),
            (
                stage_names::EXECUTION_ORCHESTRATION,
                SchedulerStageName::ExecutionOrchestration,
            ),
            (stage_names::SYNTHESIS, SchedulerStageName::Synthesis),
            (stage_names::HANDOFF, SchedulerStageName::Handoff),
            (
                stage_names::SINGLE_PASS_EXECUTOR,
                SchedulerStageName::SinglePassExecutor,
            ),
            (
                stage_names::COORDINATION_VERIFICATION,
                SchedulerStageName::CoordinationVerification,
            ),
            (
                stage_names::COORDINATION_GATE,
                SchedulerStageName::CoordinationGate,
            ),
            (
                stage_names::COORDINATION_RETRY,
                SchedulerStageName::CoordinationRetry,
            ),
            (
                stage_names::AUTONOMOUS_VERIFICATION,
                SchedulerStageName::AutonomousVerification,
            ),
            (
                stage_names::AUTONOMOUS_GATE,
                SchedulerStageName::AutonomousGate,
            ),
            (
                stage_names::AUTONOMOUS_RETRY,
                SchedulerStageName::AutonomousRetry,
            ),
        ];

        for (raw, parsed) in cases {
            assert_eq!(SchedulerStageName::parse(raw), Some(*parsed));
            assert_eq!(parsed.as_str(), *raw);
            assert_eq!(parsed.to_string(), *raw);
        }

        assert_eq!(
            SchedulerStageName::parse("execution_orchestration"),
            Some(SchedulerStageName::ExecutionOrchestration)
        );
    }

    #[test]
    fn scheduler_stage_name_serde_uses_kebab_case() {
        let json =
            serde_json::to_string(&SchedulerStageName::ExecutionOrchestration).expect("serialize");
        assert_eq!(json, "\"execution-orchestration\"");
        let decoded: SchedulerStageName =
            serde_json::from_str("\"execution-orchestration\"").expect("deserialize");
        assert_eq!(decoded, SchedulerStageName::ExecutionOrchestration);
    }

    #[test]
    fn scheduler_decision_kind_parses_case_insensitively() {
        assert_eq!(
            SchedulerDecisionKind::parse("route"),
            Some(SchedulerDecisionKind::Route)
        );
        assert_eq!(
            SchedulerDecisionKind::parse("GATE"),
            Some(SchedulerDecisionKind::Gate)
        );
        assert_eq!(SchedulerDecisionKind::Route.as_str(), "route");
        assert_eq!(SchedulerDecisionKind::Gate.as_str(), "gate");
    }
}
