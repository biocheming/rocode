use super::{RouteDecision, SchedulerPresetRuntimeFields, SchedulerPresetRuntimeUpdate};
use crate::OrchestratorOutput;

#[derive(Default)]
pub(super) struct SchedulerRouteState {
    pub(super) request_brief: String,
    pub(super) route_decision: Option<RouteDecision>,
    pub(super) direct_response: Option<String>,
    pub(super) interviewed: Option<String>,
    pub(super) routed: Option<String>,
}

#[derive(Default)]
pub(super) struct SchedulerExecutionState {
    pub(super) delegated: Option<OrchestratorOutput>,
    pub(super) reviewed: Option<OrchestratorOutput>,
    pub(super) handed_off: Option<OrchestratorOutput>,
    pub(super) synthesized: Option<OrchestratorOutput>,
}

#[derive(Default)]
pub(super) struct SchedulerPresetRuntimeState {
    pub(super) planned: Option<String>,
    pub(super) planning_artifact_path: Option<String>,
    pub(super) draft_artifact_path: Option<String>,
    pub(super) draft_snapshot: Option<String>,
    pub(super) metis_review: Option<String>,
    pub(super) momus_review: Option<String>,
    pub(super) handoff_choice: Option<String>,
    pub(super) high_accuracy_approved: Option<bool>,
}

#[derive(Default)]
pub(super) struct SchedulerMetricsState {
    pub(super) total_steps: u32,
    pub(super) total_tool_calls: u32,
}

#[derive(Default)]
pub(super) struct SchedulerProfileState {
    pub(super) route: SchedulerRouteState,
    pub(super) execution: SchedulerExecutionState,
    pub(super) preset_runtime: SchedulerPresetRuntimeState,
    pub(super) metrics: SchedulerMetricsState,
}

impl SchedulerProfileState {
    pub(super) fn preset_runtime_fields(&self) -> SchedulerPresetRuntimeFields<'_> {
        SchedulerPresetRuntimeFields {
            route_rationale_summary: self
                .route
                .route_decision
                .as_ref()
                .map(|decision| decision.rationale_summary.as_str()),
            planning_artifact_path: self.preset_runtime.planning_artifact_path.as_deref(),
            draft_artifact_path: self.preset_runtime.draft_artifact_path.as_deref(),
            interviewed: self.route.interviewed.as_deref(),
            planned: self.preset_runtime.planned.as_deref(),
            draft_snapshot: self.preset_runtime.draft_snapshot.as_deref(),
            metis_review: self.preset_runtime.metis_review.as_deref(),
            momus_review: self.preset_runtime.momus_review.as_deref(),
            handoff_choice: self.preset_runtime.handoff_choice.as_deref(),
            high_accuracy_approved: self.preset_runtime.high_accuracy_approved,
        }
    }

    pub(super) fn apply_runtime_update(&mut self, update: SchedulerPresetRuntimeUpdate) {
        match update {
            SchedulerPresetRuntimeUpdate::Planned(content) => {
                self.preset_runtime.planned = Some(content)
            }
            SchedulerPresetRuntimeUpdate::MetisReview(content) => {
                self.preset_runtime.metis_review = Some(content)
            }
            SchedulerPresetRuntimeUpdate::MomusReview(content) => {
                self.preset_runtime.momus_review = Some(content)
            }
            SchedulerPresetRuntimeUpdate::HandoffChoice(choice) => {
                self.preset_runtime.handoff_choice = Some(choice)
            }
            SchedulerPresetRuntimeUpdate::HighAccuracyApproved(approved) => {
                self.preset_runtime.high_accuracy_approved = Some(approved)
            }
        }
    }
}
