use super::{
    RouteDecision, SchedulerArtifactKind, SchedulerAutonomousGateStageInput,
    SchedulerAutonomousVerificationStageInput, SchedulerCoordinationGateStageInput,
    SchedulerCoordinationVerificationStageInput, SchedulerDraftArtifactInput,
    SchedulerExecutionOrchestrationStageInput, SchedulerHandoffDecoration,
    SchedulerHandoffStageInput, SchedulerInterviewStageInput, SchedulerMetisConsultInput,
    SchedulerPlanStageInput, SchedulerPlanningArtifactInput, SchedulerPresetRuntimeFields,
    SchedulerPresetRuntimeUpdate, SchedulerReviewStageInput, SchedulerSynthesisStageInput,
    SchedulerTransitionSpec, SchedulerTransitionTarget,
};
use crate::scheduler::profile::SchedulerProfilePlan;

impl SchedulerProfilePlan {
    pub(super) fn route_constraint_note(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.route_constraint_note())
    }

    pub(super) fn interview_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.interview_stage_prompt(profile_suffix))
    }

    pub(super) fn plan_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.plan_stage_prompt(profile_suffix))
    }

    pub(super) fn delegation_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.delegation_stage_prompt(profile_suffix))
    }

    pub(super) fn delegation_charter(&self) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.delegation_charter(self))
    }

    pub(super) fn review_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.review_stage_prompt(profile_suffix))
    }

    pub(super) fn handoff_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.handoff_stage_prompt(profile_suffix))
    }

    pub(super) fn synthesis_stage_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.synthesis_stage_prompt(profile_suffix))
    }

    pub(super) fn execution_fallback_prompt(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.execution_fallback_prompt(profile_suffix))
    }

    pub(super) fn execution_orchestration_charter(&self, profile_suffix: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.execution_orchestration_charter(self, profile_suffix))
    }

    pub(super) fn coordination_verification_charter(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.coordination_verification_charter())
    }

    pub(super) fn coordination_gate_contract(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.coordination_gate_contract())
    }

    pub(super) fn coordination_gate_prompt(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.coordination_gate_prompt())
    }

    pub(super) fn autonomous_verification_charter(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.autonomous_verification_charter())
    }

    pub(super) fn autonomous_gate_contract(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.autonomous_gate_contract())
    }

    pub(super) fn autonomous_gate_prompt(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.autonomous_gate_prompt())
    }

    pub(super) fn compose_interview_stage_input(
        &self,
        input: SchedulerInterviewStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_interview_input(input))
    }

    pub(super) fn compose_plan_stage_input(
        &self,
        input: SchedulerPlanStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_plan_input(input))
    }

    pub(super) fn compose_execution_orchestration_stage_input(
        &self,
        input: SchedulerExecutionOrchestrationStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_execution_orchestration_input(input))
    }

    pub(super) fn compose_synthesis_stage_input(
        &self,
        input: SchedulerSynthesisStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_synthesis_input(input))
    }

    pub(super) fn compose_coordination_verification_stage_input(
        &self,
        input: SchedulerCoordinationVerificationStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_coordination_verification_input(input))
    }

    pub(super) fn compose_coordination_gate_stage_input(
        &self,
        input: SchedulerCoordinationGateStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_coordination_gate_input(input))
    }

    pub(super) fn compose_autonomous_verification_stage_input(
        &self,
        input: SchedulerAutonomousVerificationStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_autonomous_verification_input(input))
    }

    pub(super) fn compose_autonomous_gate_stage_input(
        &self,
        input: SchedulerAutonomousGateStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_autonomous_gate_input(input))
    }

    pub(super) fn compose_review_stage_input(
        &self,
        input: SchedulerReviewStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_review_input(input))
    }

    pub(super) fn compose_handoff_stage_input(
        &self,
        input: SchedulerHandoffStageInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_handoff_input(input))
    }

    pub(super) fn compose_metis_input(
        &self,
        input: SchedulerMetisConsultInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_metis_input(input))
    }

    pub(super) fn compose_draft_artifact(
        &self,
        input: SchedulerDraftArtifactInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_draft_artifact(input))
    }

    pub(super) fn compose_planning_artifact(
        &self,
        input: SchedulerPlanningArtifactInput<'_>,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.compose_planning_artifact(input))
    }

    pub(super) fn decorate_handoff_output(
        &self,
        content: String,
        decoration: SchedulerHandoffDecoration,
    ) -> String {
        self.preset_definition()
            .map(|definition| definition.decorate_handoff_output(content.clone(), decoration))
            .unwrap_or(content)
    }

    pub(super) fn workflow_todos_payload(&self) -> Option<serde_json::Value> {
        self.preset_definition()
            .map(|definition| definition.workflow_todos_payload())
    }

    pub(super) fn metis_agent_name(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.metis_agent_name())
    }

    pub(super) fn runtime_update_for_metis_review(
        &self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        self.preset_definition()
            .and_then(|definition| definition.runtime_update_for_metis_review(content))
    }

    pub(super) fn handoff_choice_payload(&self) -> Option<serde_json::Value> {
        self.preset_definition()
            .and_then(|definition| definition.handoff_choice_payload())
    }

    pub(super) fn default_handoff_choice(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.default_handoff_choice())
    }

    pub(super) fn parse_handoff_choice(&self, output: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.parse_handoff_choice(output))
    }

    pub(super) fn runtime_update_for_handoff_choice(
        &self,
        choice: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        self.preset_definition()
            .and_then(|definition| definition.runtime_update_for_handoff_choice(choice))
    }

    pub(super) fn momus_agent_name(&self) -> Option<&'static str> {
        self.preset_definition()
            .and_then(|definition| definition.momus_agent_name())
    }

    pub(super) fn max_momus_rounds(&self) -> Option<usize> {
        self.preset_definition()
            .and_then(|definition| definition.max_momus_rounds())
    }

    pub(super) fn momus_output_is_okay(&self, output: &str) -> bool {
        self.preset_definition()
            .map(|definition| definition.momus_output_is_okay(output))
            .unwrap_or(false)
    }

    pub(super) fn runtime_update_for_momus_review(
        &self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        self.preset_definition()
            .and_then(|definition| definition.runtime_update_for_momus_review(content))
    }

    pub(super) fn runtime_update_for_planned_output(
        &self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        self.preset_definition()
            .and_then(|definition| definition.runtime_update_for_planned_output(content))
    }

    pub(super) fn runtime_update_for_high_accuracy(
        &self,
        approved: bool,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        self.preset_definition()
            .and_then(|definition| definition.runtime_update_for_high_accuracy(approved))
    }

    pub(super) fn resolve_runtime_transition_target(
        &self,
        transitions: &[&SchedulerTransitionSpec],
        handoff_choice: Option<&str>,
        high_accuracy_approved: Option<bool>,
    ) -> Option<SchedulerTransitionTarget> {
        self.preset_definition().and_then(|definition| {
            definition.resolve_runtime_transition_target(
                transitions,
                handoff_choice,
                high_accuracy_approved,
            )
        })
    }

    pub(super) fn normalize_final_output(&self, output: &str) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.normalize_final_output(output))
    }

    pub(super) fn normalize_review_stage_output(
        &self,
        runtime: SchedulerPresetRuntimeFields<'_>,
        output: &str,
    ) -> Option<String> {
        self.preset_definition()
            .and_then(|definition| definition.normalize_review_stage_output(runtime, output))
    }

    pub(super) fn effect_dispatch_is_authoritative(&self) -> bool {
        self.preset_definition()
            .map(|definition| definition.effect_dispatch_is_authoritative())
            .unwrap_or(false)
    }

    pub(super) fn constrain_route_decision(&self, decision: RouteDecision) -> RouteDecision {
        self.preset_definition()
            .map(|definition| definition.constrain_route_decision(decision.clone()))
            .unwrap_or(decision)
    }
}

impl SchedulerProfilePlan {
    pub(super) fn artifact_relative_path(
        &self,
        kind: SchedulerArtifactKind,
        session_id: &str,
    ) -> Option<String> {
        self.preset_definition().and_then(|definition| match kind {
            SchedulerArtifactKind::Planning => {
                definition.planning_artifact_relative_path(session_id)
            }
            SchedulerArtifactKind::Draft => definition.draft_artifact_relative_path(session_id),
        })
    }
}
