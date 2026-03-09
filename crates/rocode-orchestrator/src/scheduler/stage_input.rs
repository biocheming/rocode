use crate::scheduler::prompt_context::{AvailableAgentMeta, AvailableCategoryMeta};

pub struct SchedulerDraftArtifactInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub current_plan: Option<&'a str>,
    pub momus_review: Option<&'a str>,
    pub handoff_choice: Option<&'a str>,
    pub planning_artifact_path: Option<&'a str>,
    pub draft_artifact_path: Option<&'a str>,
}

pub struct SchedulerPlanningArtifactInput<'a> {
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub planning_output: &'a str,
    pub planning_artifact_path: Option<&'a str>,
}

pub struct SchedulerInterviewStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub route_decision_json: Option<&'a str>,
    pub draft_artifact_path: Option<&'a str>,
    pub draft_context: Option<&'a str>,
    pub current_plan: &'a str,
    pub skill_tree_context: Option<&'a str>,
}

pub struct SchedulerPlanStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub route_decision_json: Option<&'a str>,
    pub route_output: Option<&'a str>,
    pub planning_artifact_path: Option<&'a str>,
    pub draft_artifact_path: Option<&'a str>,
    pub draft_context: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub momus_feedback: Option<&'a str>,
    pub current_plan: &'a str,
    pub skill_tree_context: Option<&'a str>,
    pub available_agents: &'a [AvailableAgentMeta],
    pub available_categories: &'a [AvailableCategoryMeta],
    pub skill_list: &'a [String],
}

pub struct SchedulerExecutionOrchestrationStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub planning_output: Option<&'a str>,
    pub skill_tree_context: Option<&'a str>,
    pub available_agents: &'a [AvailableAgentMeta],
    pub available_categories: &'a [AvailableCategoryMeta],
    pub skill_list: &'a [String],
}

pub struct SchedulerSynthesisStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub current_plan: &'a str,
    pub route_decision_json: Option<&'a str>,
    pub planning_output: Option<&'a str>,
    pub delegation_output: Option<&'a str>,
    pub review_output: Option<&'a str>,
    pub saved_planning_artifact: Option<&'a str>,
}

pub struct SchedulerCoordinationVerificationStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub round: usize,
    pub execution_output: &'a str,
    pub planning_output: Option<&'a str>,
    pub skill_tree_context: Option<&'a str>,
}

pub struct SchedulerAutonomousVerificationStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub current_plan: &'a str,
    pub round: usize,
    pub execution_output: &'a str,
}

pub struct SchedulerCoordinationGateStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub current_plan: &'a str,
    pub round: usize,
    pub execution_output: &'a str,
    pub verification_output: Option<&'a str>,
}

pub struct SchedulerAutonomousGateStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub current_plan: &'a str,
    pub round: usize,
    pub execution_output: &'a str,
    pub verification_output: Option<&'a str>,
}

pub struct SchedulerReviewStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub draft_context: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub execution_plan: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub momus_feedback: Option<&'a str>,
    pub saved_planning_artifact: Option<&'a str>,
    pub active_skills_markdown: Option<&'a str>,
    pub delegation_output: Option<&'a str>,
}

pub struct SchedulerHandoffStageInput<'a> {
    pub original_request: &'a str,
    pub request_brief: &'a str,
    pub current_plan: &'a str,
    pub draft_context: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub planning_output: Option<&'a str>,
    pub review_output: Option<&'a str>,
    pub momus_review: Option<&'a str>,
    pub user_choice: Option<&'a str>,
    pub saved_planning_artifact: Option<&'a str>,
}

pub struct SchedulerMetisConsultInput<'a> {
    pub goal: &'a str,
    pub original_request: &'a str,
    pub discussed: Option<&'a str>,
    pub draft_context: Option<&'a str>,
    pub research: Option<&'a str>,
}
