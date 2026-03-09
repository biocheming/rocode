use crate::agent_tree::{AgentTreeNode, AgentTreeOrchestrator, ChildExecutionMode};
use crate::skill_graph::{SkillGraphDefinition, SkillGraphOrchestrator};
use crate::skill_tree::SkillTreeRequestPlan;
use crate::tool_runner::ToolRunner;
use crate::traits::Orchestrator;
use crate::{
    ModelRef, OrchestratorContext, OrchestratorError, OrchestratorOutput, SchedulerProfileConfig,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::profile_state::SchedulerProfileState;
use super::prompt_support::build_capabilities_summary;

use super::{
    append_artifact_note, apply_route_decision, execute_scheduler_effect_dispatch,
    execute_stage_agent, parse_route_decision, route_system_prompt, stage_agent,
    stage_agent_unbounded, validate_route_decision, AvailableAgentMeta, AvailableCategoryMeta,
    RouteDecision, RouteMode, SchedulerAutonomousGateStageInput,
    SchedulerAutonomousVerificationStageInput, SchedulerCoordinationGateStageInput,
    SchedulerCoordinationVerificationStageInput, SchedulerEffectContext, SchedulerEffectDispatch,
    SchedulerEffectKind, SchedulerEffectMoment, SchedulerEffectProtocol,
    SchedulerExecutionChildMode, SchedulerExecutionOrchestrationStageInput,
    SchedulerExecutionVerificationMode, SchedulerExecutionWorkflowKind,
    SchedulerExecutionWorkflowPolicy, SchedulerFinalizationMode, SchedulerFlowDefinition,
    SchedulerHandoffDecoration, SchedulerHandoffStageInput, SchedulerInterviewStageInput,
    SchedulerLoopBudget, SchedulerMetisConsultInput, SchedulerPlanStageInput,
    SchedulerPresetDefinition, SchedulerPresetEffectExecutor, SchedulerPresetKind,
    SchedulerReviewStageInput, SchedulerStageGraph, SchedulerStagePolicy,
    SchedulerSynthesisStageInput, SchedulerTransitionGraph, SchedulerTransitionTarget,
    SchedulerTransitionTrigger, StageToolPolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulerStageKind {
    RequestAnalysis,
    Route,
    Interview,
    Plan,
    Delegation,
    Review,
    ExecutionOrchestration,
    Synthesis,
    Handoff,
}

impl SchedulerStageKind {
    fn as_event_name(self) -> &'static str {
        match self {
            SchedulerStageKind::RequestAnalysis => "request-analysis",
            SchedulerStageKind::Route => "route",
            SchedulerStageKind::Interview => "interview",
            SchedulerStageKind::Plan => "plan",
            SchedulerStageKind::Delegation => "delegation",
            SchedulerStageKind::Review => "review",
            SchedulerStageKind::ExecutionOrchestration => "execution-orchestration",
            SchedulerStageKind::Synthesis => "synthesis",
            SchedulerStageKind::Handoff => "handoff",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ExecutionGateStatus {
    Done,
    Continue,
    Blocked,
}

#[derive(Debug, Clone, Deserialize)]
struct ExecutionGateDecision {
    status: ExecutionGateStatus,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    next_input: Option<String>,
    #[serde(default)]
    final_response: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SchedulerProfilePlan {
    pub profile_name: Option<String>,
    pub orchestrator: Option<String>,
    pub description: Option<String>,
    pub model: Option<ModelRef>,
    pub stages: Vec<SchedulerStageKind>,
    pub skill_list: Vec<String>,
    pub agent_tree: Option<AgentTreeNode>,
    pub skill_graph: Option<SkillGraphDefinition>,
    pub skill_tree: Option<SkillTreeRequestPlan>,
    pub available_agents: Vec<AvailableAgentMeta>,
    pub available_categories: Vec<AvailableCategoryMeta>,
}

impl SchedulerProfilePlan {
    pub fn new(stages: Vec<SchedulerStageKind>) -> Self {
        Self {
            profile_name: None,
            orchestrator: None,
            description: None,
            model: None,
            stages,
            skill_list: Vec::new(),
            agent_tree: None,
            skill_graph: None,
            skill_tree: None,
            available_agents: Vec::new(),
            available_categories: Vec::new(),
        }
    }

    pub fn from_profile_config(
        profile_name: Option<String>,
        default_stages: Vec<SchedulerStageKind>,
        profile: &SchedulerProfileConfig,
    ) -> Self {
        let stages = if profile.stages.is_empty() {
            default_stages
        } else {
            profile.stages.clone()
        };

        Self {
            profile_name,
            orchestrator: profile.orchestrator.clone(),
            description: profile.description.clone(),
            model: profile.model.clone(),
            stages,
            skill_list: profile.skill_list.clone(),
            agent_tree: profile.agent_tree.clone(),
            skill_graph: profile.skill_graph.clone(),
            skill_tree: profile.skill_tree.clone(),
            available_agents: profile.available_agents.clone(),
            available_categories: profile.available_categories.clone(),
        }
    }

    pub fn with_orchestrator(mut self, orchestrator: impl Into<String>) -> Self {
        self.orchestrator = Some(orchestrator.into());
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_skill_list(mut self, skill_list: Vec<String>) -> Self {
        self.skill_list = skill_list;
        self
    }

    pub fn with_agent_tree(mut self, agent_tree: AgentTreeNode) -> Self {
        self.agent_tree = Some(agent_tree);
        self
    }

    pub fn with_skill_graph(mut self, skill_graph: SkillGraphDefinition) -> Self {
        self.skill_graph = Some(skill_graph);
        self
    }

    pub fn with_skill_tree(mut self, skill_tree: SkillTreeRequestPlan) -> Self {
        self.skill_tree = Some(skill_tree);
        self
    }

    pub fn has_execution_path(&self) -> bool {
        self.agent_tree.is_some()
            || self.skill_graph.is_some()
            || self
                .stages
                .iter()
                .any(|stage| !matches!(stage, SchedulerStageKind::RequestAnalysis))
    }

    pub(super) fn preset_definition(&self) -> Option<SchedulerPresetDefinition> {
        self.orchestrator
            .as_deref()
            .and_then(|value| SchedulerPresetKind::from_str(value).ok())
            .map(SchedulerPresetKind::definition)
    }

    pub fn execution_workflow_policy(&self) -> SchedulerExecutionWorkflowPolicy {
        self.flow_definition().execution_workflow_policy
    }

    fn stage_execution_semantics(&self) -> Option<SchedulerExecutionWorkflowPolicy> {
        let workflow = self.execution_workflow_policy();
        match workflow.kind {
            SchedulerExecutionWorkflowKind::CoordinationLoop
            | SchedulerExecutionWorkflowKind::AutonomousLoop => Some(workflow),
            SchedulerExecutionWorkflowKind::Direct | SchedulerExecutionWorkflowKind::SinglePass => {
                None
            }
        }
    }

    fn stage_policy(&self, stage: SchedulerStageKind) -> SchedulerStagePolicy {
        self.preset_definition()
            .map(|definition| definition.stage_policy(stage))
            .unwrap_or(
                SchedulerPresetKind::Sisyphus
                    .definition()
                    .stage_policy(stage),
            )
    }

    pub fn flow_definition(&self) -> SchedulerFlowDefinition {
        self.preset_definition()
            .unwrap_or(SchedulerPresetKind::Sisyphus.definition())
            .flow_definition(&self.stages)
    }

    fn stage_graph(&self) -> SchedulerStageGraph {
        self.flow_definition().stage_graph
    }

    pub fn transition_graph(&self) -> SchedulerTransitionGraph {
        self.flow_definition().transition_graph
    }

    pub fn effect_protocol(&self) -> SchedulerEffectProtocol {
        self.flow_definition().effect_protocol
    }

    pub fn effect_dispatch(
        &self,
        effect: SchedulerEffectKind,
        context: SchedulerEffectContext,
    ) -> SchedulerEffectDispatch {
        self.preset_definition()
            .unwrap_or(SchedulerPresetKind::Sisyphus.definition())
            .resolve_effect_dispatch(effect, context)
    }

    fn finalization_mode(&self) -> SchedulerFinalizationMode {
        self.flow_definition().finalization_mode
    }
}

struct SchedulerEffectAdapter<'a, 'o> {
    orchestrator: &'a SchedulerProfileOrchestrator,
    original_input: &'a str,
    state: &'a mut SchedulerProfileState,
    plan: &'a SchedulerProfilePlan,
    output: &'a mut Option<&'o mut OrchestratorOutput>,
    ctx: &'a OrchestratorContext,
    stage: SchedulerStageKind,
}

#[async_trait]
impl<'a, 'o> SchedulerPresetEffectExecutor for SchedulerEffectAdapter<'a, 'o> {
    async fn ensure_planning_artifact_path(&mut self) -> Result<(), OrchestratorError> {
        let _ = SchedulerProfileOrchestrator::ensure_planning_artifact_path(
            self.plan, self.state, self.ctx,
        );
        Ok(())
    }

    async fn persist_planning_artifact(&mut self) -> Result<(), OrchestratorError> {
        if let Some(output) = self.output.as_ref() {
            SchedulerProfileOrchestrator::persist_planning_artifact(
                self.plan,
                &output.content,
                self.state,
                self.ctx,
            )?;
        }
        Ok(())
    }

    async fn sync_draft_artifact(&mut self) -> Result<(), OrchestratorError> {
        if let Err(error) = SchedulerProfileOrchestrator::sync_preset_draft_artifact(
            self.original_input,
            self.plan,
            self.state,
            self.ctx,
        ) {
            tracing::warn!(error = %error, stage = self.stage.as_event_name(), "scheduler effect failed to sync preset draft artifact");
        }
        Ok(())
    }

    async fn register_workflow_todos(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .register_scheduler_workflow_todos(self.state, self.plan, self.ctx)
            .await;
        Ok(())
    }

    async fn consult_metis(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .consult_preset_metis(self.original_input, self.state, self.plan, self.ctx)
            .await;
        Ok(())
    }

    async fn ask_handoff_choice(&mut self) -> Result<(), OrchestratorError> {
        let choice = self
            .orchestrator
            .ask_preset_handoff_choice(self.state, self.plan, self.ctx)
            .await;
        if let Some(update) = self.plan.runtime_update_for_handoff_choice(choice) {
            self.state.apply_runtime_update(update);
        }
        Ok(())
    }

    async fn run_momus_review_loop(&mut self) -> Result<(), OrchestratorError> {
        let approved = self
            .orchestrator
            .run_preset_momus_loop(self.original_input, self.state, self.plan, self.ctx)
            .await;
        if let Some(update) = self.plan.runtime_update_for_high_accuracy(approved) {
            self.state.apply_runtime_update(update);
        }
        Ok(())
    }

    async fn delete_draft_artifact(&mut self) -> Result<(), OrchestratorError> {
        let _ = SchedulerProfileOrchestrator::delete_artifact(
            SchedulerArtifactKind::Draft,
            self.state,
            self.ctx,
        )?;
        Ok(())
    }

    async fn decorate_handoff_output(
        &mut self,
        decoration: SchedulerHandoffDecoration,
    ) -> Result<(), OrchestratorError> {
        if let Some(output) = self.output.as_deref_mut() {
            output.content = self
                .plan
                .decorate_handoff_output(output.content.clone(), decoration);
        }
        Ok(())
    }
}

pub struct SchedulerProfileOrchestrator {
    plan: SchedulerProfilePlan,
    tool_runner: ToolRunner,
}

impl SchedulerProfileOrchestrator {
    pub fn new(plan: SchedulerProfilePlan, tool_runner: ToolRunner) -> Self {
        Self { plan, tool_runner }
    }

    fn compose_request_analysis_input(&self, input: &str) -> String {
        let mut sections = Vec::new();
        sections.push(
            "## Stage
request-analysis"
                .to_string(),
        );
        sections.push(format!(
            "## User Request
{input}"
        ));

        if let Some(profile_name) = &self.plan.profile_name {
            sections.push(format!(
                "## Profile Name
{profile_name}"
            ));
        }

        if let Some(description) = &self.plan.description {
            let description = description.trim();
            if !description.is_empty() {
                sections.push(format!(
                    "## Profile Description
{description}"
                ));
            }
        }

        if !self.plan.skill_list.is_empty() {
            sections.push(format!(
                "## Active Skills
{}",
                markdown_list(&self.plan.skill_list)
            ));
        }

        if let Some(skill_tree) = &self.plan.skill_tree {
            let context = skill_tree.context_markdown.trim();
            if !context.is_empty() {
                sections.push(format!(
                    "## Skill Tree Context
{context}"
                ));
            }
        }

        if let Some(route_constraint) = self.plan.route_constraint_note() {
            sections.push(format!(
                "## Workflow Constraint
{route_constraint}"
            ));
        }

        sections.push(
            "## Orchestrator Intent
Freeze the request context once, then route the request into the right workflow and preserve the same semantic goal across planning, execution, review, or handoff stages. If the active preset is Prometheus, preserve planner-only behavior and keep the session on the reviewed-plan path rather than execution."
                .to_string(),
        );

        sections.join(
            "

",
        )
    }

    fn compose_route_input(
        &self,
        original_input: &str,
        request_brief: &str,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let mut sections = Vec::new();
        sections.push("## Stage\nroute".to_string());
        sections.push(format!("## Original Request\n{original_input}"));
        sections.push(format!("## Request Brief\n{request_brief}"));
        sections.push(format!("## Current Plan\n{}", render_plan_snapshot(plan)));
        sections.push(
            "## Routing Goal
Choose the best request-scoped orchestration path across ROCode presets, then return a bounded RouteDecision JSON. Prefer planner-only handoff workflows when the request needs upfront clarification and a reviewed plan instead of execution."
                .to_string(),
        );
        if let Some(route_constraint) = plan.route_constraint_note() {
            sections.push(route_constraint.to_string());
        }
        if let Some(context) = skill_tree_context(plan) {
            sections.push(format!("## Skill Tree Context\n{context}"));
        }
        let capabilities = build_capabilities_summary(
            &plan.available_agents,
            &plan.available_categories,
            &plan.skill_list,
        );
        if !capabilities.is_empty() {
            sections.push(format!("## System Capabilities\n{capabilities}"));
        }
        sections.join("\n\n")
    }

    fn compose_interview_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let route_decision_json = state.route.route_decision.as_ref().map(|route_decision| {
            serde_json::to_string_pretty(route_decision)
                .unwrap_or_else(|_| route_decision.rationale_summary.clone())
        });
        if let Some(composed) = plan.compose_interview_stage_input(SchedulerInterviewStageInput {
            original_request: original_input,
            request_brief: &state.route.request_brief,
            route_decision_json: route_decision_json.as_deref(),
            draft_artifact_path: state.preset_runtime.draft_artifact_path.as_deref(),
            draft_context: state.preset_runtime.draft_snapshot.as_deref(),
            current_plan: &render_plan_snapshot(plan),
            skill_tree_context: skill_tree_context(plan),
        }) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
interview"
                .to_string(),
        );
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Decision
{}",
                serde_json::to_string_pretty(route_decision)
                    .unwrap_or_else(|_| route_decision.rationale_summary.clone())
            ));
        }
        sections.push(format!(
            "## Current Plan
{}",
            render_plan_snapshot(plan)
        ));
        if let Some(context) = skill_tree_context(plan) {
            sections.push(format!(
                "## Skill Tree Context
{context}"
            ));
        }
        sections.push(
            "## Interview Charter
Resolve discoverable unknowns with read-only inspection first. Ask only when a remaining preference or tradeoff materially changes the plan. Return a planning-oriented interview brief."
                .to_string(),
        );
        sections.join(
            "

",
        )
    }

    fn compose_plan_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let route_decision_json = state.route.route_decision.as_ref().map(|route_decision| {
            serde_json::to_string_pretty(route_decision)
                .unwrap_or_else(|_| route_decision.rationale_summary.clone())
        });
        if let Some(composed) = plan.compose_plan_stage_input(SchedulerPlanStageInput {
            original_request: original_input,
            request_brief: &state.route.request_brief,
            route_decision_json: route_decision_json.as_deref(),
            route_output: state.route.routed.as_deref(),
            planning_artifact_path: state.preset_runtime.planning_artifact_path.as_deref(),
            draft_artifact_path: state.preset_runtime.draft_artifact_path.as_deref(),
            draft_context: state.preset_runtime.draft_snapshot.as_deref(),
            interview_output: state.route.interviewed.as_deref(),
            metis_review: state.preset_runtime.metis_review.as_deref(),
            momus_feedback: state.preset_runtime.momus_review.as_deref(),
            current_plan: &render_plan_snapshot(plan),
            skill_tree_context: skill_tree_context(plan),
            available_agents: &plan.available_agents,
            available_categories: &plan.available_categories,
            skill_list: &plan.skill_list,
        }) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
plan"
                .to_string(),
        );
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Decision
{}",
                serde_json::to_string_pretty(route_decision)
                    .unwrap_or_else(|_| route_decision.rationale_summary.clone())
            ));
        }
        if let Some(routed) = state.route.routed.as_deref() {
            sections.push(format!(
                "## Route Output
{routed}"
            ));
        }
        sections.push(format!(
            "## Current Plan
{}",
            render_plan_snapshot(plan)
        ));
        if let Some(context) = skill_tree_context(plan) {
            sections.push(format!(
                "## Skill Tree Context
{context}"
            ));
        }
        let capabilities = build_capabilities_summary(
            &plan.available_agents,
            &plan.available_categories,
            &plan.skill_list,
        );
        if !capabilities.is_empty() {
            sections.push(format!(
                "## System Capabilities
{capabilities}"
            ));
        }
        sections.push(
            "## Planner Charter
Produce a concrete execution plan only. No file edits, no claims that work is already done. State assumptions, phases, verification, and risks."
                .to_string(),
        );
        sections.join(
            "

",
        )
    }

    fn compose_delegation_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let mut sections = Vec::new();
        sections.push("## Stage\ndelegation".to_string());
        sections.push(format!("## Original Request\n{original_input}"));
        sections.push(format!("## Request Brief\n{}", state.route.request_brief));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Summary\n{}",
                route_decision.rationale_summary
            ));
        }
        if let Some(plan_output) = state.preset_runtime.planned.as_deref() {
            sections.push(format!("## Execution Plan\n{plan_output}"));
        }
        if let Some(context) = skill_tree_context(plan) {
            sections.push(format!("## Skill Tree Context\n{context}"));
        }
        let charter = plan.delegation_charter().unwrap_or_else(|| {
            "## Execution Charter
\
                 Execute the task according to the frozen request goal. \
                 Use the execution plan when present, but do not drift from the original request."
                .to_string()
        });
        sections.push(charter);
        sections.join("\n\n")
    }

    fn compose_execution_orchestration_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let skill_tree_context_value = skill_tree_context(plan);
        if let Some(composed) = plan.compose_execution_orchestration_stage_input(
            SchedulerExecutionOrchestrationStageInput {
                original_request: original_input,
                request_brief: &state.route.request_brief,
                route_summary: state
                    .route
                    .route_decision
                    .as_ref()
                    .map(|route_decision| route_decision.rationale_summary.as_str()),
                planning_output: state.preset_runtime.planned.as_deref(),
                skill_tree_context: skill_tree_context_value.as_deref(),
                available_agents: &plan.available_agents,
                available_categories: &plan.available_categories,
                skill_list: &plan.skill_list,
            },
        ) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push("## Stage\nexecution-orchestration".to_string());
        sections.push(format!("## Original Request\n{original_input}"));
        sections.push(format!("## Request Brief\n{}", state.route.request_brief));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Summary\n{}",
                route_decision.rationale_summary
            ));
        }
        if let Some(plan_output) = state.preset_runtime.planned.as_deref() {
            sections.push(format!("## Planning Output\n{plan_output}"));
        }
        if let Some(context) = skill_tree_context_value {
            sections.push(format!("## Skill Tree Context\n{context}"));
        }
        let profile_suffix = profile_prompt_suffix(plan);
        let charter = plan
            .execution_orchestration_charter(&profile_suffix)
            .unwrap_or_else(|| {
                "## Coordination Charter
\
                 Coordinate the execution graph or worker tree, preserve task boundaries, \
                 and aggregate a single execution result."
                    .to_string()
            });
        sections.push(charter.to_string());
        sections.join("\n\n")
    }

    fn compose_review_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let active_skills_markdown =
            (!plan.skill_list.is_empty()).then(|| markdown_list(&plan.skill_list));
        if let Some(composed) = plan.compose_review_stage_input(SchedulerReviewStageInput {
            original_request: original_input,
            request_brief: &state.route.request_brief,
            route_summary: state
                .route
                .route_decision
                .as_ref()
                .map(|route_decision| route_decision.rationale_summary.as_str()),
            draft_context: state.preset_runtime.draft_snapshot.as_deref(),
            interview_output: state.route.interviewed.as_deref(),
            execution_plan: state.preset_runtime.planned.as_deref(),
            metis_review: state.preset_runtime.metis_review.as_deref(),
            momus_feedback: state.preset_runtime.momus_review.as_deref(),
            saved_planning_artifact: state.preset_runtime.planning_artifact_path.as_deref(),
            active_skills_markdown: active_skills_markdown.as_deref(),
            delegation_output: state
                .execution
                .delegated
                .as_ref()
                .map(|output| output.content.as_str()),
        }) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
review"
                .to_string(),
        );
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Summary
{}",
                route_decision.rationale_summary
            ));
        }
        if !plan.skill_list.is_empty() {
            sections.push(format!(
                "## Active Skills
{}",
                markdown_list(&plan.skill_list)
            ));
        }
        if let Some(delegated) = &state.execution.delegated {
            sections.push(format!(
                "## Delegation Output
{}",
                delegated.content
            ));
        }
        sections.push(
            "## Review Charter
Review the delegated result against the original task. Tighten the result without changing the task objective."
                .to_string(),
        );
        sections.join(
            "

",
        )
    }

    fn compose_handoff_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        if let Some(composed) = plan.compose_handoff_stage_input(SchedulerHandoffStageInput {
            original_request: original_input,
            request_brief: &state.route.request_brief,
            current_plan: &render_plan_snapshot(plan),
            draft_context: state.preset_runtime.draft_snapshot.as_deref(),
            interview_output: state.route.interviewed.as_deref(),
            planning_output: state.preset_runtime.planned.as_deref(),
            review_output: state
                .execution
                .reviewed
                .as_ref()
                .map(|output| output.content.as_str()),
            momus_review: state.preset_runtime.momus_review.as_deref(),
            user_choice: state.preset_runtime.handoff_choice.as_deref(),
            saved_planning_artifact: state.preset_runtime.planning_artifact_path.as_deref(),
        }) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
handoff"
                .to_string(),
        );
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Current Plan
{}",
            render_plan_snapshot(plan)
        ));
        sections.push(
            "## Handoff Charter
End this workflow with a reviewed planning handoff. Do not claim code execution was performed. Make the next recommended action explicit."
                .to_string(),
        );
        sections.join(
            "

",
        )
    }

    fn compose_synthesis_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        let current_plan = render_plan_snapshot(plan);
        let route_decision_json = state.route.route_decision.as_ref().map(|route_decision| {
            serde_json::to_string_pretty(route_decision)
                .unwrap_or_else(|_| route_decision.rationale_summary.clone())
        });
        if let Some(composed) = plan.compose_synthesis_stage_input(SchedulerSynthesisStageInput {
            original_request: original_input,
            request_brief: &state.route.request_brief,
            current_plan: &current_plan,
            route_decision_json: route_decision_json.as_deref(),
            planning_output: state.preset_runtime.planned.as_deref(),
            delegation_output: state
                .execution
                .delegated
                .as_ref()
                .map(|output| output.content.as_str()),
            review_output: state
                .execution
                .reviewed
                .as_ref()
                .map(|output| output.content.as_str()),
            saved_planning_artifact: state.preset_runtime.planning_artifact_path.as_deref(),
        }) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
synthesis"
                .to_string(),
        );
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Current Plan
{current_plan}"
        ));
        if let Some(route_decision) = &state.route.route_decision {
            sections.push(format!(
                "## Route Decision
{}",
                serde_json::to_string_pretty(route_decision)
                    .unwrap_or_else(|_| route_decision.rationale_summary.clone())
            ));
        }
        if let Some(plan_output) = state.preset_runtime.planned.as_deref() {
            sections.push(format!(
                "## Planning Output
{plan_output}"
            ));
        }
        if let Some(delegated) = &state.execution.delegated {
            sections.push(format!(
                "## Delegation Output
{}",
                delegated.content
            ));
        }
        if let Some(reviewed) = &state.execution.reviewed {
            sections.push(format!(
                "## Review Output
{}",
                reviewed.content
            ));
        }
        if let Some(artifact_path) = state.preset_runtime.planning_artifact_path.as_deref() {
            sections.push(format!(
                "## Saved Planning Artifact
{artifact_path}"
            ));
        }
        sections.push(
            "## Synthesis Charter
Produce the final user-facing answer. Prefer reviewed output when present, otherwise delegated output. Preserve concrete results, unresolved risks, and next actions."
                .to_string(),
        );
        sections.join(
            "

",
        )
    }

    async fn execute_agent_tree(
        &self,
        agent_tree: &AgentTreeNode,
        execution_input: &str,
        ctx: &OrchestratorContext,
        child_mode: ChildExecutionMode,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let mut tree = AgentTreeOrchestrator::new(agent_tree.clone(), self.tool_runner.clone())
            .with_child_execution_mode(child_mode);
        tree.execute(execution_input, ctx).await
    }

    async fn execute_skill_graph(
        &self,
        skill_graph: &SkillGraphDefinition,
        execution_input: &str,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let mut graph = SkillGraphOrchestrator::new(skill_graph.clone(), self.tool_runner.clone());
        graph.execute(execution_input, ctx).await
    }

    async fn execute_delegation_stage(
        &self,
        input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .delegation_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler delegation executor. \
                     Execute the frozen request goal faithfully. \
                     Use ROCode tools directly, and delegate only when it clearly helps. \
                     Return concrete execution results only.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Delegation);
        execute_stage_agent(
            input,
            ctx,
            Self::stage_agent_from_policy("scheduler-delegation", prompt, stage_policy),
            stage_policy.tool_policy,
            None,
        )
        .await
    }

    async fn execute_review_stage(
        &self,
        input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .review_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler review layer.                      Audit the current result against the original request and return a tighter, evidence-based review.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Review);
        execute_stage_agent(
            input,
            ctx,
            Self::stage_agent_from_policy("scheduler-review", prompt, stage_policy),
            stage_policy.tool_policy,
            None,
        )
        .await
    }

    async fn execute_execution_fallback_stage(
        &self,
        input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .execution_fallback_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler execution fallback. \
                     Execute the current request directly with ROCode tools and return the concrete result.{}",
                    profile_suffix
                )
            });
        execute_stage_agent(
            input,
            ctx,
            stage_agent_unbounded("scheduler-execution", prompt),
            StageToolPolicy::AllowAll,
            None,
        )
        .await
    }

    async fn execute_coordination_verification(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        round: usize,
        execution_output: &OrchestratorOutput,
    ) -> Result<Option<OrchestratorOutput>, OrchestratorError> {
        let workflow = plan.stage_execution_semantics().ok_or_else(|| {
            OrchestratorError::Other("coordination semantics unavailable".to_string())
        })?;

        let graph_verification_available = plan.agent_tree.is_some() && plan.skill_graph.is_some();
        let verification_output = if graph_verification_available {
            let verification_input = self.compose_coordination_verification_input(
                original_input,
                state,
                plan,
                round,
                execution_output,
            );
            Some(
                self.execute_skill_graph(
                    plan.skill_graph
                        .as_ref()
                        .expect("graph verifier should exist"),
                    &verification_input,
                    ctx,
                )
                .await?,
            )
        } else if matches!(
            workflow.verification_mode,
            SchedulerExecutionVerificationMode::Required
        ) {
            let verification_input = self.compose_coordination_verification_input(
                original_input,
                state,
                plan,
                round,
                execution_output,
            );
            Some(
                self.execute_review_stage(&verification_input, plan, ctx)
                    .await?,
            )
        } else {
            None
        };

        if let Some(output) = &verification_output {
            Self::record_output(state, output);
            state.execution.reviewed = Some(output.clone());
        }

        Ok(verification_output)
    }

    fn coordination_execution_unavailable_error(plan: &SchedulerProfilePlan) -> OrchestratorError {
        let orchestrator = plan.orchestrator.as_deref().unwrap_or("atlas");
        OrchestratorError::Other(format!(
            "{orchestrator} execution requires an agent_tree or skill_graph"
        ))
    }

    fn executor_execution_unavailable_error(plan: &SchedulerProfilePlan) -> OrchestratorError {
        let orchestrator = plan.orchestrator.as_deref().unwrap_or("hephaestus");
        OrchestratorError::Other(format!(
            "{orchestrator} execution requires an agent_tree or skill_graph"
        ))
    }

    fn child_execution_mode(mode: SchedulerExecutionChildMode) -> ChildExecutionMode {
        match mode {
            SchedulerExecutionChildMode::Parallel => ChildExecutionMode::Parallel,
            SchedulerExecutionChildMode::Sequential => ChildExecutionMode::Sequential,
        }
    }

    fn gate_completion_output(
        decision: &ExecutionGateDecision,
        fallback_output: &OrchestratorOutput,
    ) -> Option<OrchestratorOutput> {
        decision
            .final_response
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| OrchestratorOutput {
                content: value.to_string(),
                ..fallback_output.clone()
            })
    }

    fn gate_blocked_output(
        decision: &ExecutionGateDecision,
        fallback_output: &OrchestratorOutput,
    ) -> Option<OrchestratorOutput> {
        let blocked = decision
            .final_response
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| decision.summary.clone());
        (!blocked.trim().is_empty()).then(|| OrchestratorOutput {
            content: blocked,
            ..fallback_output.clone()
        })
    }

    fn verification_required(plan: &SchedulerProfilePlan) -> bool {
        matches!(
            plan.stage_execution_semantics()
                .map(|workflow| workflow.verification_mode),
            Some(SchedulerExecutionVerificationMode::Required)
        )
    }

    fn compose_coordination_verification_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
    ) -> String {
        let skill_tree_context_value = skill_tree_context(plan);
        if let Some(composed) = plan.compose_coordination_verification_stage_input(
            SchedulerCoordinationVerificationStageInput {
                original_request: original_input,
                request_brief: &state.route.request_brief,
                round,
                execution_output: execution_output.content.as_str(),
                planning_output: state.preset_runtime.planned.as_deref(),
                skill_tree_context: skill_tree_context_value,
            },
        ) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
coordination-verification"
                .to_string(),
        );
        sections.push(format!(
            "## Round
{round}"
        ));
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Execution Output
{}",
            execution_output.content
        ));
        if let Some(plan_output) = state.preset_runtime.planned.as_deref() {
            sections.push(format!(
                "## Planning Output
{plan_output}"
            ));
        }
        if let Some(context) = skill_tree_context_value {
            sections.push(format!(
                "## Skill Tree Context
{context}"
            ));
        }
        sections.push(
            plan.coordination_verification_charter()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    "## Verification Charter
                     Verify worker outputs against the original request.                      Confirm completion, identify missing work, and surface blockers                      without redoing the implementation."
                        .to_string()
                }),
        );
        sections.join(
            "

",
        )
    }

    fn compose_coordination_gate_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> String {
        let current_plan = render_plan_snapshot(plan);
        if let Some(composed) =
            plan.compose_coordination_gate_stage_input(SchedulerCoordinationGateStageInput {
                original_request: original_input,
                request_brief: &state.route.request_brief,
                current_plan: &current_plan,
                round,
                execution_output: execution_output.content.as_str(),
                verification_output: review_output.map(|output| output.content.as_str()),
            })
        {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
coordination-gate"
                .to_string(),
        );
        sections.push(format!(
            "## Round
{round}"
        ));
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Execution Output
{}",
            execution_output.content
        ));
        if let Some(review_output) = review_output {
            sections.push(format!(
                "## Verification Output
{}",
                review_output.content
            ));
        }
        sections.push(format!(
            "## Current Plan
{current_plan}"
        ));
        sections.push(
            plan.coordination_gate_contract()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    r#"## Coordination Decision Contract
Return JSON only: {"status":"done|continue|blocked","summary":"short summary","next_input":"optional next round task","final_response":"optional final coordinator response"}.
Use continue only when there is concrete unfinished work for another worker round."#
                        .to_string()
                }),
        );
        sections.join(
            "

",
        )
    }

    fn compose_autonomous_verification_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
    ) -> String {
        let current_plan = render_plan_snapshot(plan);
        if let Some(composed) = plan.compose_autonomous_verification_stage_input(
            SchedulerAutonomousVerificationStageInput {
                original_request: original_input,
                request_brief: &state.route.request_brief,
                current_plan: &current_plan,
                round,
                execution_output: execution_output.content.as_str(),
            },
        ) {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
autonomous-verification"
                .to_string(),
        );
        sections.push(format!(
            "## Round
{round}"
        ));
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Executor Output
{}",
            execution_output.content
        ));
        sections.push(format!(
            "## Current Plan
{current_plan}"
        ));
        sections.push(
            plan.autonomous_verification_charter()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    "## Verification Charter
                     Audit the executor output before completion. Confirm what is done,                      what evidence is present, and what remains uncertain.                      Prefer concrete verification notes over stylistic critique."
                        .to_string()
                }),
        );
        sections.join(
            "

",
        )
    }

    fn compose_autonomous_gate_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
        verification_output: Option<&OrchestratorOutput>,
    ) -> String {
        let current_plan = render_plan_snapshot(plan);
        if let Some(composed) =
            plan.compose_autonomous_gate_stage_input(SchedulerAutonomousGateStageInput {
                original_request: original_input,
                request_brief: &state.route.request_brief,
                current_plan: &current_plan,
                round,
                execution_output: execution_output.content.as_str(),
                verification_output: verification_output.map(|output| output.content.as_str()),
            })
        {
            return composed;
        }

        let mut sections = Vec::new();
        sections.push(
            "## Stage
autonomous-finish-gate"
                .to_string(),
        );
        sections.push(format!(
            "## Round
{round}"
        ));
        sections.push(format!(
            "## Original Request
{original_input}"
        ));
        sections.push(format!(
            "## Request Brief
{}",
            state.route.request_brief
        ));
        sections.push(format!(
            "## Executor Output
{}",
            execution_output.content
        ));
        if let Some(verification) = verification_output {
            sections.push(format!(
                "## Verification Output
{}",
                verification.content
            ));
        }
        sections.push(format!(
            "## Current Plan
{current_plan}"
        ));
        sections.push(
            plan.autonomous_gate_contract()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    r#"## Finish Gate Contract
Return JSON only: {"status":"done|continue|blocked","summary":"short summary","next_input":"optional retry brief","final_response":"optional final response"}.
Prefer done when the output already satisfies the request.
Use continue only when a bounded retry should materially improve the result."#
                        .to_string()
                }),
        );
        sections.join(
            "

",
        )
    }

    fn compose_retry_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        decision: &ExecutionGateDecision,
        previous_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> String {
        let mut sections = Vec::new();
        sections.push("## Retry Request".to_string());
        sections.push(format!("## Original Request\n{original_input}"));
        sections.push(format!("## Request Brief\n{}", state.route.request_brief));
        sections.push(format!("## Previous Attempt\n{}", previous_output.content));
        if let Some(review_output) = review_output {
            sections.push(format!("## Verification Notes\n{}", review_output.content));
        }
        sections.push(format!("## Retry Summary\n{}", decision.summary));
        if let Some(next_input) = decision
            .next_input
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sections.push(format!("## Retry Focus\n{next_input}"));
        }
        sections.join("\n\n")
    }

    async fn execute_coordination_gate(
        &self,
        gate_input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(OrchestratorOutput, Option<ExecutionGateDecision>), OrchestratorError> {
        let prompt = plan
            .coordination_gate_prompt()
            .map(str::to_string)
            .unwrap_or_else(|| {
                "You are the coordination gate. Decide whether the coordinator is done, needs another worker round, or is blocked. Return JSON only, never prose outside JSON.".to_string()
            });
        let output = execute_stage_agent(
            gate_input,
            ctx,
            stage_agent_unbounded("scheduler-coordination-gate", prompt),
            StageToolPolicy::DisableAll,
            None,
        )
        .await?;
        let decision = parse_execution_gate_decision(&output.content);
        Ok((output, decision))
    }

    async fn execute_autonomous_verification_stage(
        &self,
        verification_input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let prompt = plan
            .autonomous_verification_charter()
            .map(str::to_string)
            .unwrap_or_else(|| {
                "You are the verification layer. Audit the executor result with read-only reasoning and return a concise verification note: completed evidence, missing evidence, and residual risks.".to_string()
            });
        execute_stage_agent(
            verification_input,
            ctx,
            stage_agent_unbounded("scheduler-autonomous-verification", prompt),
            StageToolPolicy::AllowReadOnly,
            None,
        )
        .await
    }

    async fn execute_autonomous_gate(
        &self,
        gate_input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(OrchestratorOutput, Option<ExecutionGateDecision>), OrchestratorError> {
        let prompt = plan
            .autonomous_gate_prompt()
            .map(str::to_string)
            .unwrap_or_else(|| {
                "You are the finish gate. Judge whether the executor output is complete, needs one more bounded retry, or is blocked. Return JSON only, never prose outside JSON.".to_string()
            });
        let output = execute_stage_agent(
            gate_input,
            ctx,
            stage_agent_unbounded("scheduler-autonomous-gate", prompt),
            StageToolPolicy::DisableAll,
            None,
        )
        .await?;
        let decision = parse_execution_gate_decision(&output.content);
        Ok((output, decision))
    }

    async fn execute_coordination_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        let workflow = plan.stage_execution_semantics().ok_or_else(|| {
            OrchestratorError::Other("coordination execution semantics unavailable".to_string())
        })?;
        let max_rounds = workflow.max_rounds.max(1) as usize;
        let mut execution_input =
            self.compose_execution_orchestration_input(original_input, state, plan);

        for round in 1..=max_rounds {
            let execution_output = if let Some(agent_tree) = &plan.agent_tree {
                self.execute_agent_tree(
                    agent_tree,
                    &execution_input,
                    ctx,
                    Self::child_execution_mode(workflow.child_mode),
                )
                .await?
            } else if let Some(skill_graph) = &plan.skill_graph {
                self.execute_skill_graph(skill_graph, &execution_input, ctx)
                    .await?
            } else if workflow.allow_execution_fallback {
                self.execute_execution_fallback_stage(&execution_input, plan, ctx)
                    .await?
            } else {
                return Err(Self::coordination_execution_unavailable_error(plan));
            };
            Self::record_output(state, &execution_output);
            state.execution.delegated = Some(execution_output.clone());

            let verification_output = self
                .execute_coordination_verification(
                    original_input,
                    state,
                    plan,
                    ctx,
                    round,
                    &execution_output,
                )
                .await?;

            let gate_input = self.compose_coordination_gate_input(
                original_input,
                state,
                plan,
                round,
                &execution_output,
                verification_output.as_ref(),
            );
            let (gate_output, decision) = self
                .execute_coordination_gate(&gate_input, plan, ctx)
                .await?;
            Self::record_output(state, &gate_output);

            let Some(decision) = decision else {
                tracing::warn!(
                    round,
                    "coordination gate returned no parseable decision; stopping after current round"
                );
                break;
            };

            match decision.status {
                ExecutionGateStatus::Done => {
                    if let Some(output) = Self::gate_completion_output(&decision, &execution_output)
                    {
                        state.execution.reviewed = Some(output);
                    }
                    break;
                }
                ExecutionGateStatus::Blocked => {
                    if let Some(output) = Self::gate_blocked_output(&decision, &execution_output) {
                        state.execution.reviewed = Some(output);
                    }
                    break;
                }
                ExecutionGateStatus::Continue if round < max_rounds => {
                    execution_input = self.compose_retry_input(
                        original_input,
                        state,
                        &decision,
                        &execution_output,
                        verification_output.as_ref(),
                    );
                }
                ExecutionGateStatus::Continue => break,
            }
        }

        Ok(())
    }

    async fn execute_autonomous_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        let workflow = plan.stage_execution_semantics().ok_or_else(|| {
            OrchestratorError::Other("executor execution semantics unavailable".to_string())
        })?;
        let max_rounds = workflow.max_rounds.max(1) as usize;
        let mut execution_input =
            self.compose_execution_orchestration_input(original_input, state, plan);

        for round in 1..=max_rounds {
            let execution_output = if let Some(agent_tree) = &plan.agent_tree {
                self.execute_agent_tree(
                    agent_tree,
                    &execution_input,
                    ctx,
                    Self::child_execution_mode(workflow.child_mode),
                )
                .await?
            } else if let Some(skill_graph) = &plan.skill_graph {
                self.execute_skill_graph(skill_graph, &execution_input, ctx)
                    .await?
            } else if workflow.allow_execution_fallback {
                self.execute_execution_fallback_stage(&execution_input, plan, ctx)
                    .await?
            } else {
                return Err(Self::executor_execution_unavailable_error(plan));
            };
            Self::record_output(state, &execution_output);
            state.execution.delegated = Some(execution_output.clone());

            let verification_result = self
                .execute_autonomous_verification_stage(
                    &self.compose_autonomous_verification_input(
                        original_input,
                        state,
                        plan,
                        round,
                        &execution_output,
                    ),
                    plan,
                    ctx,
                )
                .await;
            let verification_output = match verification_result {
                Ok(output) => {
                    Self::record_output(state, &output);
                    state.execution.reviewed = Some(output.clone());
                    Some(output)
                }
                Err(err) if Self::verification_required(plan) => {
                    return Err(OrchestratorError::Other(format!(
                        "{} verification is required before finish gate: {err}",
                        plan.orchestrator.as_deref().unwrap_or("executor")
                    )));
                }
                Err(err) => {
                    tracing::warn!(error = %err, round, "autonomous verification stage failed; continuing to finish gate");
                    None
                }
            };

            let gate_input = self.compose_autonomous_gate_input(
                original_input,
                state,
                plan,
                round,
                &execution_output,
                verification_output.as_ref(),
            );
            let (gate_output, decision) =
                self.execute_autonomous_gate(&gate_input, plan, ctx).await?;
            Self::record_output(state, &gate_output);

            let Some(decision) = decision else {
                tracing::warn!(
                    round,
                    "autonomous gate returned no parseable decision; stopping after current round"
                );
                break;
            };

            match decision.status {
                ExecutionGateStatus::Done => {
                    if let Some(output) = Self::gate_completion_output(&decision, &execution_output)
                    {
                        state.execution.delegated = Some(output);
                        state.execution.reviewed = None;
                    }
                    break;
                }
                ExecutionGateStatus::Blocked => {
                    if let Some(output) = Self::gate_blocked_output(&decision, &execution_output) {
                        state.execution.delegated = Some(output);
                        state.execution.reviewed = None;
                    }
                    break;
                }
                ExecutionGateStatus::Continue if round < max_rounds => {
                    execution_input = self.compose_retry_input(
                        original_input,
                        state,
                        &decision,
                        &execution_output,
                        verification_output.as_ref(),
                    );
                }
                ExecutionGateStatus::Continue => break,
            }
        }

        Ok(())
    }

    fn stage_agent_from_policy(
        name: &str,
        system_prompt: String,
        policy: SchedulerStagePolicy,
    ) -> crate::AgentDescriptor {
        match policy.loop_budget {
            SchedulerLoopBudget::Unbounded => stage_agent_unbounded(name, system_prompt),
            SchedulerLoopBudget::StepLimit(max_steps) => {
                stage_agent(name, system_prompt, max_steps)
            }
        }
    }

    async fn execute_sisyphus_execution_orchestration(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        let execution_input =
            self.compose_execution_orchestration_input(original_input, state, plan);
        let prompt = plan
            .execution_orchestration_charter("")
            .unwrap_or_else(|| {
                "You are the scheduler execution orchestrator. Execute the task faithfully and return concrete results only.".to_string()
            });
        let output = execute_stage_agent(
            &execution_input,
            ctx,
            stage_agent_unbounded("sisyphus-executor", prompt),
            StageToolPolicy::AllowAll,
            None,
        )
        .await?;
        Self::record_output(state, &output);
        state.execution.delegated = Some(output);
        Ok(())
    }

    fn finalize_output(&self, state: SchedulerProfileState) -> OrchestratorOutput {
        let artifact_path = state.preset_runtime.planning_artifact_path.clone();
        let content = match self.plan.finalization_mode() {
            SchedulerFinalizationMode::PlannerHandoff => state
                .execution
                .handed_off
                .as_ref()
                .map(|output| output.content.clone())
                .or_else(|| {
                    state
                        .execution
                        .reviewed
                        .as_ref()
                        .map(|output| output.content.clone())
                })
                .or(state.preset_runtime.planned.clone())
                .or_else(|| {
                    state
                        .execution
                        .synthesized
                        .as_ref()
                        .map(|output| output.content.clone())
                })
                .or_else(|| {
                    state
                        .execution
                        .delegated
                        .as_ref()
                        .map(|output| output.content.clone())
                }),
            SchedulerFinalizationMode::StandardSynthesis => state
                .execution
                .synthesized
                .as_ref()
                .map(|output| output.content.clone())
                .or_else(|| {
                    state
                        .execution
                        .reviewed
                        .as_ref()
                        .map(|output| output.content.clone())
                })
                .or_else(|| {
                    state
                        .execution
                        .delegated
                        .as_ref()
                        .map(|output| output.content.clone())
                })
                .or_else(|| {
                    state
                        .execution
                        .handed_off
                        .as_ref()
                        .map(|output| output.content.clone())
                })
                .or(state.preset_runtime.planned.clone()),
        }
        .or(state.route.routed.clone())
        .unwrap_or_else(|| state.route.request_brief.clone());
        let content = self
            .plan
            .normalize_final_output(&content)
            .unwrap_or(content);
        let content = append_artifact_note(content, artifact_path.as_deref());

        let mut metadata = HashMap::new();

        // When Prometheus handoff choice is "Start Work", emit handoff metadata
        // so the frontend can auto-switch to Atlas mode.
        if matches!(
            self.plan.finalization_mode(),
            SchedulerFinalizationMode::PlannerHandoff
        ) {
            if let Some(ref choice) = state.preset_runtime.handoff_choice {
                if choice == "Start Work" {
                    metadata.insert(
                        "scheduler_handoff_mode".to_string(),
                        serde_json::json!("atlas"),
                    );
                    if let Some(ref path) = artifact_path {
                        metadata.insert(
                            "scheduler_handoff_plan_path".to_string(),
                            serde_json::json!(path),
                        );
                    }
                }
            }
        }

        OrchestratorOutput {
            content,
            steps: state.metrics.total_steps,
            tool_calls_count: state.metrics.total_tool_calls,
            metadata,
        }
    }

    async fn emit_stage_start(
        plan: &SchedulerProfilePlan,
        stage: SchedulerStageKind,
        stage_index: u32,
        ctx: &OrchestratorContext,
    ) {
        if !plan
            .stage_graph()
            .stage(stage)
            .map(|spec| spec.policy.session_projection.is_visible())
            .unwrap_or(false)
        {
            return;
        }
        ctx.lifecycle_hook
            .on_scheduler_stage_start(
                &ctx.exec_ctx.agent_name,
                stage.as_event_name(),
                stage_index,
                &ctx.exec_ctx,
            )
            .await;
    }

    async fn emit_stage_end(
        plan: &SchedulerProfilePlan,
        stage: SchedulerStageKind,
        stage_index: u32,
        output: &OrchestratorOutput,
        ctx: &OrchestratorContext,
    ) {
        if !plan
            .stage_graph()
            .stage(stage)
            .map(|spec| spec.policy.session_projection.is_visible())
            .unwrap_or(false)
            || output.content.trim().is_empty()
        {
            return;
        }
        let stage_total = plan
            .stage_graph()
            .stages
            .iter()
            .filter(|spec| spec.policy.session_projection.is_visible())
            .count() as u32;
        ctx.lifecycle_hook
            .on_scheduler_stage_end(
                &ctx.exec_ctx.agent_name,
                stage.as_event_name(),
                stage_index,
                stage_total,
                &output.content,
                &ctx.exec_ctx,
            )
            .await;
    }

    fn execution_stage_output(state: &SchedulerProfileState) -> Option<&OrchestratorOutput> {
        state
            .execution
            .delegated
            .as_ref()
            .or(state.execution.reviewed.as_ref())
    }

    fn record_output(state: &mut SchedulerProfileState, output: &OrchestratorOutput) {
        state.metrics.total_steps += output.steps;
        state.metrics.total_tool_calls += output.tool_calls_count;
    }

    async fn execute_orchestration_tool(
        tool_name: &str,
        arguments: serde_json::Value,
        state: &mut SchedulerProfileState,
        ctx: &OrchestratorContext,
    ) -> Result<crate::ToolOutput, OrchestratorError> {
        let output = ctx
            .tool_executor
            .execute(tool_name, arguments, &ctx.exec_ctx)
            .await
            .map_err(|error| OrchestratorError::ToolError {
                tool: tool_name.to_string(),
                error: error.to_string(),
            })?;
        state.metrics.total_tool_calls += 1;

        if output.is_error {
            return Err(OrchestratorError::ToolError {
                tool: tool_name.to_string(),
                error: output.output.clone(),
            });
        }

        Ok(output)
    }

    #[cfg(test)]
    fn plan_start_work_command(plan_path: Option<&str>) -> String {
        crate::scheduler::plan_start_work_command(plan_path)
    }

    async fn execute_resolved_agent(
        &self,
        name: &str,
        input: &str,
        ctx: &OrchestratorContext,
        policy: StageToolPolicy,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let agent = ctx
            .agent_resolver
            .resolve(name)
            .ok_or_else(|| OrchestratorError::AgentNotFound(name.to_string()))?;
        execute_stage_agent(input, ctx, agent, policy, None).await
    }

    async fn register_scheduler_workflow_todos(
        &self,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) {
        let Some(payload) = plan.workflow_todos_payload() else {
            return;
        };

        if let Err(error) = Self::execute_orchestration_tool("todowrite", payload, state, ctx).await
        {
            tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "scheduler workflow todo registration failed; continuing");
        }
    }

    async fn consult_preset_metis(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) {
        let Some(agent_name) = plan.metis_agent_name() else {
            return;
        };
        let Some(metis_input) = plan.compose_metis_input(SchedulerMetisConsultInput {
            goal: &state.route.request_brief,
            original_request: original_input,
            discussed: state.route.interviewed.as_deref(),
            draft_context: state.preset_runtime.draft_snapshot.as_deref(),
            research: state.route.routed.as_deref(),
        }) else {
            return;
        };
        match self
            .execute_resolved_agent(
                agent_name,
                &metis_input,
                ctx,
                StageToolPolicy::AllowReadOnly,
            )
            .await
        {
            Ok(output) => {
                Self::record_output(state, &output);
                if let Some(update) = plan.runtime_update_for_metis_review(output.content.clone()) {
                    state.apply_runtime_update(update);
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "preset metis consultation failed; continuing without metis review");
            }
        }
    }

    async fn ask_preset_handoff_choice(
        &self,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> String {
        let Some(payload) = plan.handoff_choice_payload() else {
            return String::new();
        };
        let default_choice = plan.default_handoff_choice().unwrap_or("").to_string();

        let answer = match Self::execute_orchestration_tool("question", payload, state, ctx).await {
            Ok(output) => plan
                .parse_handoff_choice(&output.output)
                .unwrap_or_else(|| default_choice.clone()),
            Err(error) => {
                tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "preset handoff choice prompt failed; defaulting to configured choice");
                default_choice
            }
        };

        if let Some(update) = plan.runtime_update_for_handoff_choice(answer.clone()) {
            state.apply_runtime_update(update);
        }
        answer
    }

    async fn run_preset_momus_loop(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> bool {
        let Some(plan_path) = state.preset_runtime.planning_artifact_path.clone() else {
            tracing::warn!(orchestrator = ?plan.orchestrator, "preset review loop skipped because no plan artifact path was available");
            return false;
        };
        let Some(agent_name) = plan.momus_agent_name() else {
            return false;
        };
        let Some(max_rounds) = plan.max_momus_rounds() else {
            return false;
        };

        for round in 1..=max_rounds {
            let review = match self
                .execute_resolved_agent(agent_name, &plan_path, ctx, StageToolPolicy::AllowReadOnly)
                .await
            {
                Ok(output) => output,
                Err(error) => {
                    tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "preset review loop agent failed");
                    return false;
                }
            };
            Self::record_output(state, &review);
            if let Some(update) = plan.runtime_update_for_momus_review(review.content.clone()) {
                state.apply_runtime_update(update);
            }
            if let Err(error) = Self::sync_preset_draft_artifact(original_input, plan, state, ctx) {
                tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to sync draft after preset review loop");
            }

            if plan.momus_output_is_okay(&review.content) {
                return true;
            }

            if round == max_rounds {
                break;
            }

            match self
                .execute_plan_stage(original_input, state, plan, ctx, None)
                .await
            {
                Ok(output) => {
                    Self::record_output(state, &output);
                    if let Some(update) =
                        plan.runtime_update_for_planned_output(output.content.clone())
                    {
                        state.apply_runtime_update(update);
                    }
                    if let Err(error) =
                        Self::persist_planning_artifact(plan, &output.content, state, ctx)
                    {
                        tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to persist regenerated plan after preset review loop");
                    }
                    if let Err(error) =
                        Self::sync_preset_draft_artifact(original_input, plan, state, ctx)
                    {
                        tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to sync draft after plan regeneration");
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "preset review loop failed to regenerate plan");
                    return false;
                }
            }
        }

        false
    }

    fn scheduler_effect_context(
        state: &SchedulerProfileState,
        ctx: &OrchestratorContext,
    ) -> SchedulerEffectContext {
        let draft_exists = state
            .preset_runtime
            .draft_artifact_path
            .as_deref()
            .map(|path| Path::new(&ctx.exec_ctx.workdir).join(path).exists())
            .unwrap_or(false);
        SchedulerEffectContext {
            planning_artifact_path: state.preset_runtime.planning_artifact_path.clone(),
            draft_artifact_path: state.preset_runtime.draft_artifact_path.clone(),
            user_choice: state.preset_runtime.handoff_choice.clone(),
            high_accuracy_approved: state.preset_runtime.high_accuracy_approved,
            draft_exists,
        }
    }

    async fn run_stage_effects(
        &self,
        stage: SchedulerStageKind,
        moment: SchedulerEffectMoment,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        mut output: Option<&mut OrchestratorOutput>,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        for effect in plan.effect_protocol().effects_for(stage, moment) {
            let dispatch =
                plan.effect_dispatch(effect.effect, Self::scheduler_effect_context(state, ctx));
            if dispatch != SchedulerEffectDispatch::Skip || plan.effect_dispatch_is_authoritative()
            {
                {
                    let mut adapter = SchedulerEffectAdapter {
                        orchestrator: self,
                        original_input,
                        state,
                        plan,
                        output: &mut output,
                        ctx,
                        stage,
                    };
                    execute_scheduler_effect_dispatch(dispatch, &mut adapter).await?;
                }
                continue;
            }

            match effect.effect {
                SchedulerEffectKind::EnsurePlanningArtifactPath => {
                    let _ = Self::ensure_planning_artifact_path(plan, state, ctx);
                }
                SchedulerEffectKind::PersistPlanningArtifact => {
                    if let Some(output) = output.as_ref() {
                        Self::persist_planning_artifact(plan, &output.content, state, ctx)?;
                    }
                }
                SchedulerEffectKind::PersistDraftArtifact
                | SchedulerEffectKind::SyncDraftArtifact => {
                    if let Err(error) =
                        Self::sync_preset_draft_artifact(original_input, plan, state, ctx)
                    {
                        tracing::warn!(error = %error, stage = stage.as_event_name(), "scheduler effect failed to sync preset draft artifact");
                    }
                }
                SchedulerEffectKind::RegisterWorkflowTodos => {
                    self.register_scheduler_workflow_todos(state, plan, ctx)
                        .await;
                }
                SchedulerEffectKind::ConsultMetis => {
                    self.consult_preset_metis(original_input, state, plan, ctx)
                        .await;
                }
                SchedulerEffectKind::AskHandoffChoice
                | SchedulerEffectKind::RunMomusReviewLoop
                | SchedulerEffectKind::DeleteDraftArtifact
                | SchedulerEffectKind::AppendStartWorkGuidance => {}
            }
        }
        Ok(())
    }

    fn resolve_transition_target(
        &self,
        stage: SchedulerStageKind,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> SchedulerTransitionTarget {
        let transition_graph = plan.transition_graph();
        let transitions = transition_graph.transitions_from(stage);

        if let Some(target) = plan.resolve_runtime_transition_target(
            &transitions,
            state.preset_runtime.handoff_choice.as_deref(),
            state.preset_runtime.high_accuracy_approved,
        ) {
            return target;
        }

        transitions
            .iter()
            .find(|transition| transition.trigger == SchedulerTransitionTrigger::OnSuccess)
            .map(|transition| transition.to)
            .unwrap_or(SchedulerTransitionTarget::Finish)
    }

    fn next_stage_index(
        &self,
        stage: SchedulerStageKind,
        current_index: usize,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> Option<usize> {
        match self.resolve_transition_target(stage, state, plan) {
            SchedulerTransitionTarget::Finish => None,
            SchedulerTransitionTarget::Stage(target) => plan
                .stages
                .iter()
                .position(|candidate| *candidate == target)
                .or_else(|| {
                    let fallback = current_index + 1;
                    (fallback < plan.stages.len()).then_some(fallback)
                }),
        }
    }

    async fn execute_route_stage(
        &self,
        original_input: &str,
        request_brief: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<(OrchestratorOutput, RouteDecision), OrchestratorError> {
        let input = self.compose_route_input(original_input, request_brief, plan);
        let stage_policy = plan.stage_policy(SchedulerStageKind::Route);
        let output = execute_stage_agent(
            &input,
            ctx,
            Self::stage_agent_from_policy(
                "scheduler-route",
                route_system_prompt().to_string(),
                stage_policy,
            ),
            stage_policy.tool_policy,
            stage_context,
        )
        .await?;
        let decision = parse_route_decision(&output.content).ok_or_else(|| {
            OrchestratorError::Other(
                "route stage did not return a valid RouteDecision JSON".to_string(),
            )
        })?;
        validate_route_decision(&decision).map_err(|error| {
            OrchestratorError::Other(format!(
                "route stage returned invalid RouteDecision: {error}"
            ))
        })?;
        Ok((output, decision))
    }

    async fn execute_interview_stage(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let input = self.compose_interview_input(original_input, state, plan);
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .interview_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler interview layer. Clarify the request enough for planning with read-only inspection first, then return a concise planning brief.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Interview);
        execute_stage_agent(
            &input,
            ctx,
            Self::stage_agent_from_policy("scheduler-interview", prompt, stage_policy),
            stage_policy.tool_policy,
            stage_context,
        )
        .await
    }

    async fn execute_plan_stage(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let input = self.compose_plan_input(original_input, state, plan);
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .plan_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are ROCode's planning stage. \
                     Ask the planning questions internally, inspect the codebase with read-only tools when needed, \
                     and return a concrete execution plan. Keep the output practical: assumptions, ordered steps, \
                     verification, and risks. Never claim the task is already implemented.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Plan);
        execute_stage_agent(
            &input,
            ctx,
            Self::stage_agent_from_policy("scheduler-plan", prompt, stage_policy),
            stage_policy.tool_policy,
            stage_context,
        )
        .await
    }

    async fn execute_handoff_stage(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let input = self.compose_handoff_input(original_input, state, plan);
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .handoff_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler handoff layer. Produce a concise next-step handoff without claiming work was executed beyond the evidence in prior stages.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Handoff);
        execute_stage_agent(
            &input,
            ctx,
            Self::stage_agent_from_policy("scheduler-handoff", prompt, stage_policy),
            stage_policy.tool_policy,
            stage_context,
        )
        .await
    }

    async fn execute_synthesis_stage(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let input = self.compose_synthesis_input(original_input, state, plan);
        let profile_suffix = profile_prompt_suffix(plan);
        let prompt = plan
            .synthesis_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the final synthesis layer for ROCode's scheduler. \
                     Merge prior stage outputs into a single final response for the user. \
                     Keep the answer faithful to actual stage results. \
                     Do not invent edits, tool calls, or conclusions. \
                     If there are remaining risks or follow-ups, state them clearly.{}",
                    profile_suffix
                )
            });
        let stage_policy = plan.stage_policy(SchedulerStageKind::Synthesis);
        execute_stage_agent(
            &input,
            ctx,
            Self::stage_agent_from_policy("scheduler-synthesis", prompt, stage_policy),
            stage_policy.tool_policy,
            stage_context,
        )
        .await
    }
}

#[async_trait]
impl Orchestrator for SchedulerProfileOrchestrator {
    async fn execute(
        &mut self,
        input: &str,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        if !self.plan.has_execution_path() {
            return Err(OrchestratorError::Other(
                "scheduler profile requires at least one execution dimension".to_string(),
            ));
        }

        let mut resolved_plan = self.plan.clone();
        let mut state = SchedulerProfileState::default();
        if Self::resolve_artifact_relative_path(
            &resolved_plan,
            SchedulerArtifactKind::Draft,
            &ctx.exec_ctx.session_id,
        )
        .is_some()
        {
            let _ = Self::ensure_artifact_path(
                &resolved_plan,
                SchedulerArtifactKind::Draft,
                &mut state,
                ctx,
            );
            state.preset_runtime.draft_snapshot = Self::load_artifact_snapshot(
                &resolved_plan,
                SchedulerArtifactKind::Draft,
                &mut state,
                ctx,
            );
        }
        let mut stage_idx = 0usize;

        while stage_idx < resolved_plan.stages.len() {
            let stage = resolved_plan.stages[stage_idx];
            let stage_ordinal = stage_idx as u32 + 1;
            Self::emit_stage_start(&resolved_plan, stage, stage_ordinal, ctx).await;
            match stage {
                SchedulerStageKind::RequestAnalysis => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                }
                SchedulerStageKind::Route => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    match self
                        .execute_route_stage(input, &state.route.request_brief, &resolved_plan, ctx, Some((SchedulerStageKind::Route.as_event_name().to_string(), stage_ordinal)))
                        .await
                    {
                        Ok((output, decision)) => {
                            Self::record_output(&mut state, &output);
                            Self::emit_stage_end(
                                &resolved_plan,
                                stage,
                                stage_ordinal,
                                &output,
                                ctx,
                            )
                            .await;
                            state.route.routed = Some(output.content.clone());

                            tracing::info!(
                                mode = ?decision.mode,
                                preset = decision.preset.as_deref().unwrap_or("<inherit>"),
                                rationale = %decision.rationale_summary,
                                "route stage resolved request-scoped plan"
                            );

                            let decision = resolved_plan.constrain_route_decision(decision);

                            match decision.mode {
                                RouteMode::Direct => {
                                    let reply = decision
                                        .direct_response
                                        .clone()
                                        .filter(|s| !s.trim().is_empty())
                                        .unwrap_or_else(|| output.content.clone());

                                    state.route.route_decision = Some(decision);
                                    state.route.direct_response = Some(reply.clone());

                                    return Ok(OrchestratorOutput {
                                        content: reply,
                                        steps: state.metrics.total_steps,
                                        tool_calls_count: state.metrics.total_tool_calls,
                                        metadata: HashMap::new(),
                                    });
                                }
                                RouteMode::Orchestrate => {
                                    apply_route_decision(&mut resolved_plan, stage_idx, &decision);
                                    state.route.route_decision = Some(decision);
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "route stage failed; keeping original plan");
                        }
                    }
                }
                SchedulerStageKind::Interview => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    self.run_stage_effects(
                        stage,
                        SchedulerEffectMoment::OnEnter,
                        input,
                        &mut state,
                        &resolved_plan,
                        None,
                        ctx,
                    )
                    .await?;
                    match self
                        .execute_interview_stage(input, &state, &resolved_plan, ctx, Some((SchedulerStageKind::Interview.as_event_name().to_string(), stage_ordinal)))
                        .await
                    {
                        Ok(output) => {
                            Self::record_output(&mut state, &output);
                            state.route.interviewed = Some(output.content.clone());
                            self.run_stage_effects(
                                stage,
                                SchedulerEffectMoment::OnSuccess,
                                input,
                                &mut state,
                                &resolved_plan,
                                None,
                                ctx,
                            )
                            .await?;
                            Self::emit_stage_end(
                                &resolved_plan,
                                stage,
                                stage_ordinal,
                                &output,
                                ctx,
                            )
                            .await;
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "interview stage failed; continuing without explicit interview brief");
                        }
                    }
                }
                SchedulerStageKind::Plan => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    self.run_stage_effects(
                        stage,
                        SchedulerEffectMoment::OnEnter,
                        input,
                        &mut state,
                        &resolved_plan,
                        None,
                        ctx,
                    )
                    .await?;
                    match self
                        .execute_plan_stage(input, &state, &resolved_plan, ctx, Some((SchedulerStageKind::Plan.as_event_name().to_string(), stage_ordinal)))
                        .await
                    {
                        Ok(output) => {
                            Self::record_output(&mut state, &output);
                            if let Some(update) = resolved_plan
                                .runtime_update_for_planned_output(output.content.clone())
                            {
                                state.apply_runtime_update(update);
                            }
                            let mut effect_output = output.clone();
                            self.run_stage_effects(
                                stage,
                                SchedulerEffectMoment::OnSuccess,
                                input,
                                &mut state,
                                &resolved_plan,
                                Some(&mut effect_output),
                                ctx,
                            )
                            .await?;
                            Self::emit_stage_end(
                                &resolved_plan,
                                stage,
                                stage_ordinal,
                                &effect_output,
                                ctx,
                            )
                            .await;
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "plan stage failed; continuing without explicit plan");
                        }
                    }
                }
                SchedulerStageKind::Delegation => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    let delegation_input =
                        self.compose_delegation_input(input, &state, &resolved_plan);
                    if let Some(agent_tree) = &resolved_plan.agent_tree {
                        let mut tree = AgentTreeOrchestrator::new(
                            agent_tree.clone(),
                            self.tool_runner.clone(),
                        );
                        let output = tree.execute(&delegation_input, ctx).await?;
                        Self::record_output(&mut state, &output);
                        state.execution.delegated = Some(output);
                    } else {
                        let output = self
                            .execute_delegation_stage(&delegation_input, &resolved_plan, ctx)
                            .await?;
                        Self::record_output(&mut state, &output);
                        state.execution.delegated = Some(output);
                    }
                }
                SchedulerStageKind::ExecutionOrchestration => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }

                    let workflow = resolved_plan.execution_workflow_policy();
                    match workflow.kind {
                        SchedulerExecutionWorkflowKind::SinglePass => {
                            self.execute_sisyphus_execution_orchestration(
                                input,
                                &mut state,
                                &resolved_plan,
                                ctx,
                            )
                            .await?;
                        }
                        SchedulerExecutionWorkflowKind::Direct => {
                            let execution_input = self.compose_execution_orchestration_input(
                                input,
                                &state,
                                &resolved_plan,
                            );
                            if let Some(agent_tree) = &resolved_plan.agent_tree {
                                let output = self
                                    .execute_agent_tree(
                                        agent_tree,
                                        &execution_input,
                                        ctx,
                                        Self::child_execution_mode(workflow.child_mode),
                                    )
                                    .await?;
                                Self::record_output(&mut state, &output);
                                state.execution.delegated = Some(output);
                            } else if let Some(skill_graph) = &resolved_plan.skill_graph {
                                let output = self
                                    .execute_skill_graph(skill_graph, &execution_input, ctx)
                                    .await?;
                                Self::record_output(&mut state, &output);
                                state.execution.delegated = Some(output);
                            } else {
                                let output = self
                                    .execute_execution_fallback_stage(
                                        &execution_input,
                                        &resolved_plan,
                                        ctx,
                                    )
                                    .await?;
                                Self::record_output(&mut state, &output);
                                state.execution.delegated = Some(output);
                            }
                        }
                        SchedulerExecutionWorkflowKind::CoordinationLoop => {
                            self.execute_coordination_execution_workflow(
                                input,
                                &mut state,
                                &resolved_plan,
                                ctx,
                            )
                            .await?;
                        }
                        SchedulerExecutionWorkflowKind::AutonomousLoop => {
                            self.execute_autonomous_execution_workflow(
                                input,
                                &mut state,
                                &resolved_plan,
                                ctx,
                            )
                            .await?;
                        }
                    }

                    if let Some(output) = Self::execution_stage_output(&state).cloned() {
                        Self::emit_stage_end(&resolved_plan, stage, stage_ordinal, &output, ctx)
                            .await;
                    }
                }
                SchedulerStageKind::Review => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    let review_input = self.compose_review_input(input, &state, &resolved_plan);
                    if Self::resolve_artifact_relative_path(
                        &resolved_plan,
                        SchedulerArtifactKind::Draft,
                        &ctx.exec_ctx.session_id,
                    )
                    .is_some()
                    {
                        let output = self
                            .execute_review_stage(&review_input, &resolved_plan, ctx)
                            .await?;
                        Self::record_output(&mut state, &output);
                        let mut normalized = output.clone();
                        normalized.content = resolved_plan
                            .normalize_review_stage_output(
                                state.preset_runtime_fields(),
                                &output.content,
                            )
                            .unwrap_or_else(|| output.content.clone());
                        Self::emit_stage_end(
                            &resolved_plan,
                            stage,
                            stage_ordinal,
                            &normalized,
                            ctx,
                        )
                        .await;
                        state.execution.reviewed = Some(normalized);
                    } else if let Some(skill_graph) = &resolved_plan.skill_graph {
                        let mut graph = SkillGraphOrchestrator::new(
                            skill_graph.clone(),
                            self.tool_runner.clone(),
                        );
                        let output = graph.execute(&review_input, ctx).await?;
                        Self::record_output(&mut state, &output);
                        Self::emit_stage_end(&resolved_plan, stage, stage_ordinal, &output, ctx)
                            .await;
                        state.execution.reviewed = Some(output);
                    } else if state.execution.delegated.is_some() {
                        let output = self
                            .execute_review_stage(&review_input, &resolved_plan, ctx)
                            .await?;
                        Self::record_output(&mut state, &output);
                        Self::emit_stage_end(&resolved_plan, stage, stage_ordinal, &output, ctx)
                            .await;
                        state.execution.reviewed = Some(output);
                    }
                }
                SchedulerStageKind::Synthesis => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    match self
                        .execute_synthesis_stage(input, &state, &resolved_plan, ctx, Some((SchedulerStageKind::Synthesis.as_event_name().to_string(), stage_ordinal)))
                        .await
                    {
                        Ok(output) => {
                            Self::record_output(&mut state, &output);
                            Self::emit_stage_end(
                                &resolved_plan,
                                stage,
                                stage_ordinal,
                                &output,
                                ctx,
                            )
                            .await;
                            state.execution.synthesized = Some(output);
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "synthesis stage failed; falling back to prior stage output");
                        }
                    }
                }
                SchedulerStageKind::Handoff => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }
                    self.run_stage_effects(
                        stage,
                        SchedulerEffectMoment::OnEnter,
                        input,
                        &mut state,
                        &resolved_plan,
                        None,
                        ctx,
                    )
                    .await?;
                    self.run_stage_effects(
                        stage,
                        SchedulerEffectMoment::BeforeTransition,
                        input,
                        &mut state,
                        &resolved_plan,
                        None,
                        ctx,
                    )
                    .await?;
                    match self
                        .execute_handoff_stage(input, &state, &resolved_plan, ctx, Some((SchedulerStageKind::Handoff.as_event_name().to_string(), stage_ordinal)))
                        .await
                    {
                        Ok(output) => {
                            Self::record_output(&mut state, &output);
                            let mut effect_output = output.clone();
                            self.run_stage_effects(
                                stage,
                                SchedulerEffectMoment::OnSuccess,
                                input,
                                &mut state,
                                &resolved_plan,
                                Some(&mut effect_output),
                                ctx,
                            )
                            .await?;
                            Self::emit_stage_end(
                                &resolved_plan,
                                stage,
                                stage_ordinal,
                                &effect_output,
                                ctx,
                            )
                            .await;
                            state.execution.handed_off = Some(effect_output);
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "handoff stage failed; falling back to prior stage output");
                        }
                    }
                }
            }
            match self.next_stage_index(stage, stage_idx, &state, &resolved_plan) {
                Some(next_stage) => stage_idx = next_stage,
                None => break,
            }
        }

        Ok(self.finalize_output(state))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SchedulerArtifactKind {
    Planning,
    Draft,
}

fn parse_execution_gate_decision(output: &str) -> Option<ExecutionGateDecision> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    for candidate in profile_json_candidates(trimmed) {
        if let Ok(decision) = serde_json::from_str::<ExecutionGateDecision>(&candidate) {
            return Some(decision);
        }
    }

    None
}

fn profile_json_candidates(output: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    for marker in ["```json", "```JSON", "```"] {
        let mut remaining = output;
        while let Some(start) = remaining.find(marker) {
            let after = &remaining[start + marker.len()..];
            if let Some(end) = after.find("```") {
                let candidate = after[..end].trim();
                if !candidate.is_empty() {
                    candidates.push(candidate.to_string());
                }
                remaining = &after[end + 3..];
            } else {
                break;
            }
        }
    }

    if let Some((start, end)) = profile_find_balanced_json_object(output) {
        let candidate = output[start..end].trim();
        if !candidate.is_empty() {
            candidates.push(candidate.to_string());
        }
    }

    if candidates.is_empty() {
        candidates.push(trimmed_or_original(output));
    }

    candidates
}

fn trimmed_or_original(output: &str) -> String {
    output.trim().to_string()
}

fn profile_find_balanced_json_object(input: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    return start.map(|s| (s, idx + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}

fn markdown_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn skill_tree_context(plan: &SchedulerProfilePlan) -> Option<&str> {
    plan.skill_tree
        .as_ref()
        .map(|tree| tree.context_markdown.trim())
        .filter(|context| !context.is_empty())
}

fn render_plan_snapshot(plan: &SchedulerProfilePlan) -> String {
    let mut lines = Vec::new();
    if let Some(profile_name) = &plan.profile_name {
        lines.push(format!("profile: {profile_name}"));
    }
    if let Some(orchestrator) = &plan.orchestrator {
        lines.push(format!("orchestrator: {orchestrator}"));
    }
    if let Some(description) = plan
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("description: {description}"));
    }
    lines.push(format!("stages: {}", render_stage_sequence(&plan.stages)));
    if !plan.skill_list.is_empty() {
        lines.push(format!("skills: {}", plan.skill_list.join(", ")));
    }
    if let Some(agent_tree) = &plan.agent_tree {
        lines.push(format!("root-agent: {}", agent_tree.agent.name));
    }
    if plan.skill_graph.is_some() {
        lines.push("review-graph: enabled".to_string());
    }
    lines.join("\n")
}

fn render_stage_sequence(stages: &[SchedulerStageKind]) -> String {
    stages
        .iter()
        .map(|stage| match stage {
            SchedulerStageKind::RequestAnalysis => "request-analysis",
            SchedulerStageKind::Route => "route",
            SchedulerStageKind::Interview => "interview",
            SchedulerStageKind::Plan => "plan",
            SchedulerStageKind::Delegation => "delegation",
            SchedulerStageKind::Review => "review",
            SchedulerStageKind::ExecutionOrchestration => "execution-orchestration",
            SchedulerStageKind::Synthesis => "synthesis",
            SchedulerStageKind::Handoff => "handoff",
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn profile_prompt_suffix(plan: &SchedulerProfilePlan) -> String {
    let mut sections = Vec::new();

    if let Some(profile_name) = plan
        .profile_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Profile: {profile_name}"));
    }

    if let Some(orchestrator) = plan
        .orchestrator
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Orchestrator: {orchestrator}"));
    }

    if let Some(description) = plan
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Description: {description}"));
    }

    if !plan.skill_list.is_empty() {
        sections.push(format!(
            "Active Skills:\n{}",
            markdown_list(&plan.skill_list)
        ));
    }

    let capabilities = build_capabilities_summary(
        &plan.available_agents,
        &plan.available_categories,
        &plan.skill_list,
    );
    if !capabilities.is_empty() {
        sections.push(capabilities);
    }

    if let Some(context) = skill_tree_context(plan) {
        sections.push(format!("Skill Tree Context:\n{context}"));
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Scheduler Profile Context\n{}",
            sections.join("\n\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::profile_state::{
        SchedulerExecutionState, SchedulerMetricsState, SchedulerPresetRuntimeState,
        SchedulerRouteState,
    };
    use super::*;
    use crate::traits::{AgentResolver, ModelResolver, NoopLifecycleHook, ToolExecutor};
    use crate::{
        AgentDescriptor, ExecutionContext, ModelRef, Orchestrator, OrchestratorContext, ReviewMode,
        SchedulerEffectKind, SchedulerEffectMoment, SchedulerEffectSpec, SchedulerTransitionSpec,
        SchedulerTransitionTarget, SchedulerTransitionTrigger, ToolExecError, ToolOutput,
    };
    use async_trait::async_trait;
    use futures::stream;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Mutex;

    fn planner_only_plan() -> SchedulerProfilePlan {
        SchedulerProfilePlan::new(vec![
            SchedulerStageKind::RequestAnalysis,
            SchedulerStageKind::Route,
            SchedulerStageKind::Interview,
            SchedulerStageKind::Plan,
            SchedulerStageKind::Review,
            SchedulerStageKind::Handoff,
        ])
        .with_orchestrator("prometheus")
    }

    #[test]
    fn atlas_workflow_policy_uses_coordination_loop_with_required_verification() {
        let workflow = SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
            .with_orchestrator("atlas")
            .execution_workflow_policy();

        assert_eq!(
            workflow.kind,
            SchedulerExecutionWorkflowKind::CoordinationLoop
        );
        assert_eq!(workflow.child_mode, SchedulerExecutionChildMode::Parallel);
        assert!(workflow.allow_execution_fallback);
        assert_eq!(
            workflow.verification_mode,
            SchedulerExecutionVerificationMode::Required
        );
        assert_eq!(workflow.max_rounds, 3);
    }

    #[test]
    fn sisyphus_workflow_policy_uses_single_pass_scheduler_loop() {
        let workflow = SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
            .with_orchestrator("sisyphus")
            .execution_workflow_policy();

        assert_eq!(workflow.kind, SchedulerExecutionWorkflowKind::SinglePass);
        assert_eq!(workflow.max_rounds, 1);
    }

    #[test]
    fn hephaestus_workflow_policy_uses_autonomous_loop_with_fallback() {
        let workflow = SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
            .with_orchestrator("hephaestus")
            .execution_workflow_policy();

        assert_eq!(
            workflow.kind,
            SchedulerExecutionWorkflowKind::AutonomousLoop
        );
        assert_eq!(workflow.child_mode, SchedulerExecutionChildMode::Sequential);
        assert!(workflow.allow_execution_fallback);
        assert_eq!(
            workflow.verification_mode,
            SchedulerExecutionVerificationMode::Required
        );
        assert_eq!(workflow.max_rounds, 3);
    }

    #[test]
    fn finalize_output_prefers_handoff_over_review_and_plan() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "brief".to_string(),
                ..Default::default()
            },
            execution: SchedulerExecutionState {
                reviewed: Some(OrchestratorOutput {
                    content: "review".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                handed_off: Some(OrchestratorOutput {
                    content: "handoff".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                ..Default::default()
            },
            preset_runtime: SchedulerPresetRuntimeState {
                planned: Some("plan".to_string()),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 3,
                total_tool_calls: 2,
            },
        };

        let output = orchestrator.finalize_output(state);
        assert_eq!(output.content, "handoff");
        assert_eq!(output.steps, 3);
        assert_eq!(output.tool_calls_count, 2);
    }

    #[test]
    fn finalize_output_normalizes_sisyphus_delivery_shape() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("sisyphus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            execution: SchedulerExecutionState {
                delegated: Some(OrchestratorOutput {
                    content: "Shipped the change and verified the targeted behavior.".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**Execution Outcome**"));
        assert!(output.content.contains("**Verification**"));
    }

    #[test]
    fn finalize_output_normalizes_atlas_delivery_shape() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::Synthesis])
                .with_orchestrator("atlas"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            execution: SchedulerExecutionState {
                synthesized: Some(OrchestratorOutput {
                    content: "Task A done. Task B verified.".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**Task Status**"));
        assert!(output.content.contains("**Verification**"));
    }

    #[test]
    fn finalize_output_normalizes_hephaestus_delivery_shape() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("hephaestus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            execution: SchedulerExecutionState {
                delegated: Some(OrchestratorOutput {
                    content: "Fixed the diagnostics path and ran the targeted check.".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**What Changed**"));
        assert!(output.content.contains("**Verification**"));
    }

    #[test]
    fn atlas_synthesis_input_uses_preset_authority() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::Synthesis])
                .with_orchestrator("atlas"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "Coordinate remaining tasks".to_string(),
                route_decision: Some(RouteDecision {
                    mode: RouteMode::Orchestrate,
                    direct_kind: None,
                    direct_response: None,
                    preset: Some("atlas".to_string()),
                    insert_plan_stage: None,
                    review_mode: None,
                    context_append: None,
                    rationale_summary: "coordination-heavy task list".to_string(),
                }),
                ..Default::default()
            },
            execution: SchedulerExecutionState {
                delegated: Some(OrchestratorOutput {
                    content: "worker claims task A done".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                reviewed: Some(OrchestratorOutput {
                    content: "task A verified".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                }),
                ..Default::default()
            },
            preset_runtime: SchedulerPresetRuntimeState {
                planned: Some(
                    "- task A
- task B"
                        .to_string(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };

        let input = orchestrator.compose_synthesis_input(
            "ship the migration cleanup plan",
            &state,
            &orchestrator.plan,
        );
        assert!(input.contains("## Delivery Summary"));
        assert!(input.contains("prefer reviewed verification over worker claims"));
    }

    #[test]
    fn atlas_coordination_verification_input_uses_preset_authority() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("atlas"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "Coordinate remaining tasks".to_string(),
                ..Default::default()
            },
            preset_runtime: SchedulerPresetRuntimeState {
                planned: Some(
                    "- task A
- task B"
                        .to_string(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        let execution = OrchestratorOutput {
            content: "worker round output".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };

        let input = orchestrator.compose_coordination_verification_input(
            "ship the migration cleanup plan",
            &state,
            &orchestrator.plan,
            2,
            &execution,
        );
        assert!(input.contains("Audit each Atlas task item individually"));
        assert!(input.contains("task boundary"));
    }

    #[test]
    fn atlas_coordination_gate_input_uses_preset_authority() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("atlas"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "Coordinate remaining tasks".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let execution = OrchestratorOutput {
            content: "worker round output".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };
        let review = OrchestratorOutput {
            content: "task A verified, task B weak".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };

        let input = orchestrator.compose_coordination_gate_input(
            "ship the migration cleanup plan",
            &state,
            &orchestrator.plan,
            2,
            &execution,
            Some(&review),
        );
        assert!(input.contains("Judge completion by task boundary"));
        assert!(input.contains("weakly-verified task items"));
    }

    #[test]
    fn hephaestus_autonomous_verification_input_uses_preset_authority() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("hephaestus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "Autonomously fix the diagnostics path".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let execution = OrchestratorOutput {
            content: "fixed the path".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };

        let input = orchestrator.compose_autonomous_verification_input(
            "fix the failing lsp diagnostics path",
            &state,
            &orchestrator.plan,
            1,
            &execution,
        );
        assert!(input.contains("proof of EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY"));
        assert!(input.contains("changed artifacts"));
    }

    #[test]
    fn hephaestus_autonomous_gate_input_uses_preset_authority() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("hephaestus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                request_brief: "Autonomously fix the diagnostics path".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let execution = OrchestratorOutput {
            content: "fixed the path".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };
        let verification = OrchestratorOutput {
            content: "targeted check passed".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        };

        let input = orchestrator.compose_autonomous_gate_input(
            "fix the failing lsp diagnostics path",
            &state,
            &orchestrator.plan,
            1,
            &execution,
            Some(&verification),
        );
        assert!(input.contains("proved completion"));
        assert!(input.contains("bounded retry"));
        assert!(input.contains("**What Changed**"));
    }

    #[test]
    fn render_stage_sequence_includes_interview_and_handoff() {
        let rendered = render_stage_sequence(&[
            SchedulerStageKind::RequestAnalysis,
            SchedulerStageKind::Route,
            SchedulerStageKind::Interview,
            SchedulerStageKind::Plan,
            SchedulerStageKind::Review,
            SchedulerStageKind::Handoff,
        ]);

        assert_eq!(
            rendered,
            "request-analysis -> route -> interview -> plan -> review -> handoff"
        );
    }

    #[test]
    fn constrain_route_decision_keeps_prometheus_workflow() {
        let decision = RouteDecision {
            mode: RouteMode::Orchestrate,
            direct_kind: None,
            direct_response: None,
            preset: Some("sisyphus".to_string()),
            insert_plan_stage: Some(false),
            review_mode: Some(ReviewMode::Skip),
            context_append: None,
            rationale_summary: "switch presets".to_string(),
        };

        let constrained = SchedulerPresetKind::Prometheus
            .definition()
            .constrain_route_decision(decision);

        assert_eq!(constrained.preset.as_deref(), Some("prometheus"));
        assert_eq!(constrained.review_mode, Some(ReviewMode::Normal));
    }

    #[test]
    fn request_analysis_input_includes_prometheus_workflow_constraint() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );

        let input = orchestrator.compose_request_analysis_input("Plan the TUI workflow polish");

        assert!(input.contains("## Workflow Constraint"));
        assert!(input.contains("preserve the Prometheus workflow"));
        assert!(input.contains("planner-only behavior"));
    }

    #[test]
    fn prometheus_stage_graph_exposes_planner_workflow_and_handoff_finalization() {
        let plan = planner_only_plan();
        let graph = plan.stage_graph();
        assert_eq!(
            graph.stage_kinds(),
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::Route,
                SchedulerStageKind::Interview,
                SchedulerStageKind::Plan,
                SchedulerStageKind::Review,
                SchedulerStageKind::Handoff,
            ]
        );
        let review = graph
            .stage(SchedulerStageKind::Review)
            .expect("review stage spec");
        assert_eq!(
            review.policy.tool_policy,
            StageToolPolicy::PrometheusPlanning
        );
        assert_eq!(review.policy.loop_budget, SchedulerLoopBudget::Unbounded);
        assert_eq!(
            plan.finalization_mode(),
            SchedulerFinalizationMode::PlannerHandoff
        );
    }

    #[test]
    fn prometheus_flow_definition_exposes_handoff_loop_back_to_plan() {
        let plan = planner_only_plan();
        let transitions = plan.transition_graph();
        assert!(transitions.transitions.contains(&SchedulerTransitionSpec {
            from: SchedulerStageKind::Interview,
            trigger: SchedulerTransitionTrigger::OnSuccess,
            to: SchedulerTransitionTarget::Stage(SchedulerStageKind::Plan),
        }));
        assert!(transitions.transitions.contains(&SchedulerTransitionSpec {
            from: SchedulerStageKind::Handoff,
            trigger: SchedulerTransitionTrigger::OnUserChoice("High Accuracy Review"),
            to: SchedulerTransitionTarget::Stage(SchedulerStageKind::Plan),
        }));
        assert!(transitions.transitions.contains(&SchedulerTransitionSpec {
            from: SchedulerStageKind::Handoff,
            trigger: SchedulerTransitionTrigger::OnUserChoice("Start Work"),
            to: SchedulerTransitionTarget::Finish,
        }));
    }

    #[test]
    fn prometheus_effect_protocol_exposes_artifact_and_handoff_effects() {
        let plan = planner_only_plan();
        let effects = plan.effect_protocol();
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Interview,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::SyncDraftArtifact,
        }));
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::ConsultMetis,
        }));
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::BeforeTransition,
            effect: SchedulerEffectKind::RunMomusReviewLoop,
        }));
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::AppendStartWorkGuidance,
        }));
    }

    #[test]
    fn prometheus_runtime_transition_finishes_on_start_work_choice() {
        let plan = planner_only_plan();
        let orchestrator = SchedulerProfileOrchestrator::new(
            plan.clone(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let handoff_index = plan
            .stages
            .iter()
            .position(|stage| *stage == SchedulerStageKind::Handoff)
            .expect("handoff stage index");
        let state = SchedulerProfileState {
            preset_runtime: SchedulerPresetRuntimeState {
                handoff_choice: Some("Start Work".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            orchestrator.next_stage_index(
                SchedulerStageKind::Handoff,
                handoff_index,
                &state,
                &plan,
            ),
            None,
        );
    }

    #[test]
    fn prometheus_runtime_transition_loops_back_to_plan_when_high_accuracy_blocked() {
        let plan = planner_only_plan();
        let orchestrator = SchedulerProfileOrchestrator::new(
            plan.clone(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let handoff_index = plan
            .stages
            .iter()
            .position(|stage| *stage == SchedulerStageKind::Handoff)
            .expect("handoff stage index");
        let plan_index = plan
            .stages
            .iter()
            .position(|stage| *stage == SchedulerStageKind::Plan)
            .expect("plan stage index");
        let state = SchedulerProfileState {
            preset_runtime: SchedulerPresetRuntimeState {
                handoff_choice: Some("High Accuracy Review".to_string()),
                high_accuracy_approved: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(
            orchestrator.next_stage_index(
                SchedulerStageKind::Handoff,
                handoff_index,
                &state,
                &plan,
            ),
            Some(plan_index),
        );
    }

    #[test]
    fn synthesis_stage_is_projected_to_session_for_public_presets() {
        let plan = SchedulerProfilePlan::new(vec![SchedulerStageKind::Synthesis])
            .with_orchestrator("sisyphus");
        let graph = plan.stage_graph();
        let synthesis = graph
            .stage(SchedulerStageKind::Synthesis)
            .expect("synthesis stage spec");
        assert!(synthesis.policy.session_projection.is_visible());
        assert_eq!(synthesis.policy.tool_policy, StageToolPolicy::DisableAll);
        assert_eq!(
            synthesis.policy.loop_budget,
            SchedulerLoopBudget::Unbounded
        );
    }

    #[test]
    fn sisyphus_effect_dispatch_uses_shared_scheduler_protocol() {
        let plan = SchedulerProfilePlan::new(vec![SchedulerStageKind::Review])
            .with_orchestrator("sisyphus");
        let dispatch = plan.effect_dispatch(
            SchedulerEffectKind::ConsultMetis,
            SchedulerEffectContext {
                planning_artifact_path: None,
                draft_artifact_path: None,
                user_choice: None,
                high_accuracy_approved: None,
                draft_exists: true,
            },
        );

        assert_eq!(dispatch, SchedulerEffectDispatch::ConsultMetis);
    }

    #[test]
    fn atlas_effect_dispatch_uses_shared_scheduler_protocol() {
        let plan =
            SchedulerProfilePlan::new(vec![SchedulerStageKind::Review]).with_orchestrator("atlas");
        let dispatch = plan.effect_dispatch(
            SchedulerEffectKind::PersistPlanningArtifact,
            SchedulerEffectContext {
                planning_artifact_path: Some("artifact.md".to_string()),
                draft_artifact_path: None,
                user_choice: None,
                high_accuracy_approved: None,
                draft_exists: true,
            },
        );

        assert_eq!(dispatch, SchedulerEffectDispatch::PersistPlanningArtifact);
    }

    #[test]
    fn plan_start_work_command_uses_plan_name_when_available() {
        assert_eq!(
            SchedulerProfileOrchestrator::plan_start_work_command(Some(
                ".sisyphus/plans/plan-demo-session.md"
            )),
            "/start-work plan-demo-session"
        );
        assert_eq!(
            SchedulerProfileOrchestrator::plan_start_work_command(None),
            "/start-work"
        );
    }

    #[test]
    fn normalize_prometheus_review_output_preserves_structured_model_output() {
        let state = SchedulerProfileState::default();
        let review_output = "## Plan Generated: plan-demo-session

**Key Decisions Made**
- Keep planner-only flow.

**Scope**
- IN: Use the reviewed plan.
- OUT: Code execution.

**Guardrails Applied**
- None.

**Auto-Resolved**
- None.

**Defaults Applied**
- None.

**Decisions Needed**
- None.

**Handoff Readiness**
- Ready for handoff.

**Review Notes**
- None.";

        let normalized = SchedulerPresetKind::Prometheus
            .definition()
            .normalize_review_stage_output(state.preset_runtime_fields(), review_output)
            .expect("prometheus normalization");

        assert_eq!(normalized, review_output);
    }

    #[test]
    fn normalize_prometheus_review_output_emits_omo_style_sections() {
        let state = SchedulerProfileState {
            route: SchedulerRouteState {
                route_decision: Some(RouteDecision {
                    mode: RouteMode::Orchestrate,
                    direct_kind: None,
                    direct_response: None,
                    preset: Some("prometheus".to_string()),
                    insert_plan_stage: None,
                    review_mode: Some(ReviewMode::Normal),
                    context_append: None,
                    rationale_summary: "Planner-only handoff stays in Prometheus.".to_string(),
                }),
                interviewed: Some(
                    "Confirmed TUI input scope and no execution in this phase.".to_string(),
                ),
                ..Default::default()
            },
            preset_runtime: SchedulerPresetRuntimeState {
                planned: Some(
                    "# Plan

- [DECISION NEEDED: pick the final scrollbar style]"
                        .to_string(),
                ),
                planning_artifact_path: Some(".sisyphus/plans/plan-demo-session.md".to_string()),
                metis_review: Some(
                    "- Preserve planner-only boundaries
- Keep handoff explicit"
                        .to_string(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };

        let review = SchedulerPresetKind::Prometheus
            .definition()
            .normalize_review_stage_output(
                state.preset_runtime_fields(),
                "Review found one unresolved choice.",
            )
            .expect("prometheus normalization");

        assert!(review.contains("## Plan Generated: plan-demo-session"));
        assert!(review.contains("**Defaults Applied**"));
        assert!(review.contains("**Decisions Needed**"));
        assert!(review.contains("**Auto-Resolved**"));
        assert!(review.contains("**Handoff Readiness**"));
        assert!(review.contains("DECISION NEEDED"));
    }

    #[test]
    fn finalize_output_appends_prometheus_artifact_note() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            preset_runtime: SchedulerPresetRuntimeState {
                planned: Some(
                    "# Plan

- Step 1"
                        .to_string(),
                ),
                planning_artifact_path: Some(".sisyphus/plans/plan-demo-session.md".to_string()),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
            },
            ..Default::default()
        };
        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("# Plan"));
        assert!(output
            .content
            .contains(".sisyphus/plans/plan-demo-session.md"));
    }

    struct TestAgentResolver;

    #[async_trait]
    impl AgentResolver for TestAgentResolver {
        fn resolve(&self, name: &str) -> Option<AgentDescriptor> {
            match name {
                "metis" | "momus" => Some(AgentDescriptor {
                    name: name.to_string(),
                    system_prompt: Some(format!("You are {name}.")),
                    model: None,
                    max_steps: Some(4),
                    temperature: Some(0.1),
                    allowed_tools: Vec::new(),
                }),
                _ => None,
            }
        }
    }

    struct TestModelResolver {
        streams: Mutex<Vec<rocode_provider::StreamResult>>,
    }

    impl TestModelResolver {
        fn new(streams: Vec<rocode_provider::StreamResult>) -> Self {
            Self {
                streams: Mutex::new(streams),
            }
        }
    }

    #[async_trait]
    impl ModelResolver for TestModelResolver {
        async fn chat_stream(
            &self,
            _model: Option<&ModelRef>,
            _messages: Vec<rocode_provider::Message>,
            _tools: Vec<rocode_provider::ToolDefinition>,
            _exec_ctx: &ExecutionContext,
        ) -> Result<rocode_provider::StreamResult, OrchestratorError> {
            self.streams
                .lock()
                .await
                .pop()
                .ok_or_else(|| OrchestratorError::Other("missing test stream".to_string()))
        }
    }

    fn stream_from_text(text: &str) -> rocode_provider::StreamResult {
        Box::pin(stream::iter(vec![
            Ok::<_, rocode_provider::ProviderError>(rocode_provider::StreamEvent::TextDelta(
                text.to_string(),
            )),
            Ok::<_, rocode_provider::ProviderError>(rocode_provider::StreamEvent::Done),
        ]))
    }

    fn new_temp_workdir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("rocode-prometheus-profile-{unique}"));
        std::fs::create_dir_all(&path).expect("temp workdir should create");
        path
    }

    fn test_context(
        workdir: &Path,
        session_id: &str,
        streams: Vec<rocode_provider::StreamResult>,
    ) -> OrchestratorContext {
        test_context_with_executor(workdir, session_id, streams, Arc::new(NoopToolExecutor))
    }

    fn test_context_with_executor(
        workdir: &Path,
        session_id: &str,
        streams: Vec<rocode_provider::StreamResult>,
        tool_executor: Arc<dyn ToolExecutor>,
    ) -> OrchestratorContext {
        OrchestratorContext {
            agent_resolver: Arc::new(TestAgentResolver),
            model_resolver: Arc::new(TestModelResolver::new(streams)),
            tool_executor: tool_executor.clone(),
            lifecycle_hook: Arc::new(NoopLifecycleHook),
            exec_ctx: ExecutionContext {
                session_id: session_id.to_string(),
                workdir: workdir.display().to_string(),
                agent_name: "prometheus".to_string(),
                metadata: HashMap::new(),
            },
        }
    }

    #[tokio::test]
    async fn prometheus_plan_stage_persists_markdown_artifact() {
        let workdir = new_temp_workdir();
        let session_id = "test-session";
        let expected_relative = SchedulerPresetKind::Prometheus
            .definition()
            .planning_artifact_relative_path(session_id)
            .expect("prometheus plan artifact path");
        let expected_path = workdir.join(&expected_relative);
        let context = test_context(
            &workdir,
            session_id,
            vec![
                stream_from_text("# Plan\n\n- Normalize Ctrl+H\n- Rebind help"),
                stream_from_text("## Metis Review\n- Guardrail: keep help binding reachable"),
            ],
        );
        let runner = ToolRunner::new(Arc::new(NoopToolExecutor));
        let mut orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::Plan])
                .with_orchestrator("prometheus"),
            runner,
        );

        let output = orchestrator
            .execute("Fix backspace popup", &context)
            .await
            .expect("prometheus plan should succeed");

        let artifact =
            std::fs::read_to_string(&expected_path).expect("prometheus plan artifact should exist");
        assert!(artifact.contains("## TL;DR"));
        assert!(artifact.contains("## Verification Strategy"));
        assert!(artifact.contains("## TODOs"));
        assert!(artifact.contains("Normalize Ctrl+H"));
        assert!(artifact.contains("Rebind help"));
        assert!(output.content.contains("Plan saved to:"));
        assert!(output.content.contains(&expected_relative));
        assert_eq!(output.steps, 2);

        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[tokio::test]
    async fn prometheus_interview_stage_persists_draft_artifact() {
        let workdir = new_temp_workdir();
        let session_id = "draft-session";
        let expected_relative = SchedulerPresetKind::Prometheus
            .definition()
            .draft_artifact_relative_path(session_id)
            .expect("prometheus draft artifact path");
        let expected_path = workdir.join(&expected_relative);
        let context = test_context(
            &workdir,
            session_id,
            vec![stream_from_text(
                "## Interview Brief
- Goal: fix backspace behavior",
            )],
        );
        let runner = ToolRunner::new(Arc::new(NoopToolExecutor));
        let mut orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::Interview])
                .with_orchestrator("prometheus"),
            runner,
        );

        orchestrator
            .execute("Fix backspace popup", &context)
            .await
            .expect("prometheus interview should succeed");

        let artifact = std::fs::read_to_string(&expected_path)
            .expect("prometheus draft artifact should exist");
        assert!(artifact.contains("# Draft:"));
        assert!(artifact.contains("## Requirements (confirmed)"));
        assert!(artifact.contains("## Open Questions"));
        assert!(artifact.contains("fix backspace behavior"));

        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[derive(Default)]
    struct RecordingToolExecutor {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ToolExecutor for RecordingToolExecutor {
        async fn execute(
            &self,
            tool_name: &str,
            _arguments: serde_json::Value,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Result<ToolOutput, ToolExecError> {
            self.calls.lock().await.push(tool_name.to_string());
            match tool_name {
                "todowrite" => Ok(ToolOutput {
                    output: "todos updated".to_string(),
                    is_error: false,
                    title: None,
                    metadata: None,
                }),
                "question" => Ok(ToolOutput {
                    output: r#"{"answers":["Start Work"]}"#.to_string(),
                    is_error: false,
                    title: None,
                    metadata: None,
                }),
                other => Err(ToolExecError::ExecutionError(format!(
                    "unexpected tool call: {other}"
                ))),
            }
        }

        async fn list_ids(&self) -> Vec<String> {
            Vec::new()
        }

        async fn list_definitions(
            &self,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Vec<rocode_provider::ToolDefinition> {
            Vec::new()
        }
    }

    #[tokio::test]
    async fn prometheus_handoff_registers_todos_and_guides_start_work() {
        let workdir = new_temp_workdir();
        let session_id = "handoff-session";
        let draft_relative = SchedulerPresetKind::Prometheus
            .definition()
            .draft_artifact_relative_path(session_id)
            .expect("prometheus draft path");
        let draft_path = workdir.join(&draft_relative);
        let tool_executor = Arc::new(RecordingToolExecutor::default());
        let context = test_context_with_executor(
            &workdir,
            session_id,
            vec![
                stream_from_text(
                    "## Handoff
Ready for execution.",
                ),
                stream_from_text(
                    "## Review
Plan looks consistent.",
                ),
                stream_from_text(
                    "# Plan

- Fix input handling",
                ),
                stream_from_text(
                    "## Metis Review
- Keep scope tight",
                ),
                stream_from_text(
                    "## Interview Brief
- Confirm TUI behavior",
                ),
                stream_from_text(
                    r#"{"mode":"orchestrate","preset":"prometheus","rationale_summary":"Stay in planner workflow"}"#,
                ),
            ],
            tool_executor.clone(),
        );
        let runner = ToolRunner::new(tool_executor.clone());
        let mut orchestrator = SchedulerProfileOrchestrator::new(planner_only_plan(), runner);

        let output = orchestrator
            .execute("Fix the TUI backspace flow", &context)
            .await
            .expect("prometheus flow should succeed");

        let calls = tool_executor.calls.lock().await.clone();
        assert!(calls.iter().any(|call| call == "todowrite"));
        assert!(calls.iter().any(|call| call == "question"));
        assert!(output.content.contains("/start-work"));
        assert!(output.content.contains("Plan saved to:"));
        assert!(!draft_path.exists());

        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[derive(Default)]
    struct NoopToolExecutor;

    #[async_trait]
    impl ToolExecutor for NoopToolExecutor {
        async fn execute(
            &self,
            _tool_name: &str,
            _arguments: serde_json::Value,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Result<ToolOutput, ToolExecError> {
            Err(ToolExecError::ExecutionError("unused in tests".to_string()))
        }

        async fn list_ids(&self) -> Vec<String> {
            Vec::new()
        }

        async fn list_definitions(
            &self,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Vec<rocode_provider::ToolDefinition> {
            Vec::new()
        }
    }
}
