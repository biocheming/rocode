use crate::agent_tree::{AgentTreeNode, AgentTreeOrchestrator};
use crate::output_metadata::{
    append_output_usage, continuation_targets, merge_output_metadata, output_usage,
    ContinuationTarget,
};
use crate::skill_graph::{SkillGraphDefinition, SkillGraphOrchestrator};
use crate::skill_tree::SkillTreeRequestPlan;
use crate::tool_runner::ToolRunner;
use crate::traits::Orchestrator;
use crate::{
    ModelRef, OrchestratorContext, OrchestratorError, OrchestratorOutput, SchedulerProfileConfig,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use super::execution::SchedulerExecutionService;
use super::execution_adapter::SchedulerExecutionCapabilityAdapter;
use super::execution_input as scheduler_execution_input;
use super::profile_state::SchedulerProfileState;
use super::prompt_support::build_capabilities_summary;

use super::{
    append_artifact_note, apply_route_decision, execute_scheduler_effect_dispatch,
    execute_scheduler_execution_stage_dispatch, execute_stage_agent, parse_route_decision,
    route_system_prompt, stage_agent, stage_agent_unbounded, validate_route_decision,
    AvailableAgentMeta, AvailableCategoryMeta, RouteDecision, RouteMode,
    SchedulerAdvisoryReviewInput, SchedulerEffectContext, SchedulerEffectDispatch,
    SchedulerEffectKind, SchedulerEffectMoment, SchedulerEffectProtocol,
    SchedulerExecutionGateDecision, SchedulerExecutionGateStatus, SchedulerExecutionStageDispatch,
    SchedulerExecutionWorkflowKind, SchedulerExecutionWorkflowPolicy, SchedulerFlowDefinition,
    SchedulerHandoffDecoration, SchedulerHandoffStageInput,
    SchedulerInterviewStageInput, SchedulerLoopBudget, SchedulerPlanStageInput,
    SchedulerPresetDefinition, SchedulerPresetEffectExecutor,
    SchedulerPresetExecutionStageExecutor, SchedulerPresetKind, SchedulerReviewStageInput,
    SchedulerStageCapabilities, SchedulerStageGraph, SchedulerStagePolicy,
    SchedulerSynthesisStageInput, SchedulerTransitionGraph, SchedulerTransitionTarget,
    SchedulerTransitionTrigger, StageToolPolicy,
};
#[cfg(test)]
use super::SchedulerFinalizationMode;

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

    pub fn event_name(self) -> &'static str {
        self.as_event_name()
    }

    pub fn from_event_name(value: &str) -> Option<Self> {
        match value {
            "request-analysis" => Some(Self::RequestAnalysis),
            "route" => Some(Self::Route),
            "interview" => Some(Self::Interview),
            "plan" => Some(Self::Plan),
            "delegation" => Some(Self::Delegation),
            "review" => Some(Self::Review),
            "execution-orchestration" => Some(Self::ExecutionOrchestration),
            "synthesis" => Some(Self::Synthesis),
            "handoff" => Some(Self::Handoff),
            _ => None,
        }
    }

    /// Whether this stage kind inherently delegates work and thus needs
    /// awareness of available skills, agents, and categories.
    pub fn needs_capabilities(self) -> bool {
        matches!(
            self,
            Self::Plan | Self::ExecutionOrchestration | Self::Delegation
        )
    }
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

    /// Resolve capabilities from plan-level config. Used as fallback when
    /// a stage that needs capabilities has no per-stage override.
    pub fn resolve_capabilities(&self) -> SchedulerStageCapabilities {
        SchedulerStageCapabilities {
            skill_list: self.skill_list.clone(),
            agents: self
                .available_agents
                .iter()
                .map(|a| a.name.clone())
                .collect(),
            categories: self
                .available_categories
                .iter()
                .map(|c| c.name.clone())
                .collect(),
        }
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

    fn execution_stage_dispatch(&self) -> SchedulerExecutionStageDispatch {
        self.preset_definition()
            .unwrap_or(SchedulerPresetKind::Sisyphus.definition())
            .execution_stage_dispatch()
    }

    pub(super) fn stage_execution_semantics(&self) -> Option<SchedulerExecutionWorkflowPolicy> {
        let workflow = self.execution_workflow_policy();
        match workflow.kind {
            SchedulerExecutionWorkflowKind::CoordinationLoop
            | SchedulerExecutionWorkflowKind::AutonomousLoop => Some(workflow),
            SchedulerExecutionWorkflowKind::Direct | SchedulerExecutionWorkflowKind::SinglePass => {
                None
            }
        }
    }

    pub(super) fn stage_policy(&self, stage: SchedulerStageKind) -> SchedulerStagePolicy {
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

    #[cfg(test)]
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
        if let Err(error) = SchedulerProfileOrchestrator::sync_runtime_draft_artifact(
            self.original_input,
            self.plan,
            self.state,
            self.ctx,
        ) {
            tracing::warn!(error = %error, stage = self.stage.as_event_name(), "scheduler effect failed to sync runtime draft artifact");
        }
        Ok(())
    }

    async fn register_workflow_todos(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .register_scheduler_workflow_todos(self.state, self.plan, self.ctx)
            .await;
        Ok(())
    }

    async fn request_advisory_review(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .request_capability_advisory_review(
                self.original_input,
                self.state,
                self.plan,
                self.ctx,
            )
            .await;
        Ok(())
    }

    async fn request_user_choice(&mut self) -> Result<(), OrchestratorError> {
        let choice = self
            .orchestrator
            .request_capability_user_choice(self.state, self.plan, self.ctx)
            .await;
        if let Some(update) = self.plan.runtime_update_for_user_choice(choice) {
            self.state.apply_runtime_update(update);
        }
        Ok(())
    }

    async fn run_approval_review_loop(&mut self) -> Result<(), OrchestratorError> {
        let approved = self
            .orchestrator
            .run_capability_approval_review_loop(
                self.original_input,
                self.state,
                self.plan,
                self.ctx,
            )
            .await;
        if let Some(update) = self.plan.runtime_update_for_review_gate(approved) {
            self.state.apply_runtime_update(update);
        }
        Ok(())
    }

    async fn delete_draft_artifact(&mut self) -> Result<(), OrchestratorError> {
        let _ = SchedulerProfileOrchestrator::delete_artifact(
            self.plan,
            SchedulerArtifactKind::Draft,
            self.state,
            self.ctx,
        )?;
        Ok(())
    }

    async fn decorate_final_output(
        &mut self,
        decoration: SchedulerHandoffDecoration,
    ) -> Result<(), OrchestratorError> {
        if let Some(output) = self.output.as_deref_mut() {
            output.content = self
                .plan
                .decorate_final_output(output.content.clone(), decoration);
        }
        Ok(())
    }
}

struct SchedulerExecutionStageAdapter<'a> {
    orchestrator: &'a SchedulerProfileOrchestrator,
    original_input: &'a str,
    state: &'a mut SchedulerProfileState,
    plan: &'a SchedulerProfilePlan,
    ctx: &'a OrchestratorContext,
}

#[async_trait]
impl<'a> SchedulerPresetExecutionStageExecutor for SchedulerExecutionStageAdapter<'a> {
    async fn execute_direct_stage(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .execute_direct_execution_workflow(self.original_input, self.state, self.plan, self.ctx)
            .await
    }

    async fn execute_single_pass_stage(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .execute_single_pass_execution_workflow(
                self.original_input,
                self.state,
                self.plan,
                self.ctx,
            )
            .await
    }

    async fn execute_coordination_loop_stage(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .execute_coordination_execution_workflow(
                self.original_input,
                self.state,
                self.plan,
                self.ctx,
            )
            .await
    }

    async fn execute_autonomous_loop_stage(&mut self) -> Result<(), OrchestratorError> {
        self.orchestrator
            .execute_autonomous_execution_workflow(
                self.original_input,
                self.state,
                self.plan,
                self.ctx,
            )
            .await
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

    pub(super) fn tool_runner(&self) -> ToolRunner {
        self.tool_runner.clone()
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
            advisory_review: state.preset_runtime.advisory_review.as_deref(),
            approval_feedback: state.preset_runtime.approval_review.as_deref(),
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

    pub(super) fn compose_execution_orchestration_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
    ) -> String {
        scheduler_execution_input::compose_execution_orchestration_input(
            original_input,
            state,
            plan,
        )
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
            advisory_review: state.preset_runtime.advisory_review.as_deref(),
            approval_feedback: state.preset_runtime.approval_review.as_deref(),
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
            approval_review: state.preset_runtime.approval_review.as_deref(),
            user_choice: state.preset_runtime.user_choice.as_deref(),
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

    async fn execute_delegation_stage(
        &self,
        input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
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
            stage_context,
        )
        .await
    }

    pub(super) async fn execute_review_stage(
        &self,
        input: &str,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        SchedulerExecutionCapabilityAdapter::new(self, plan, ctx)
            .execute_review_stage(input, stage_context)
            .await
    }

    async fn execute_direct_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        SchedulerExecutionService::new(self, original_input, state, plan, ctx)
            .run_direct()
            .await
    }

    pub(super) fn compose_coordination_verification_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
    ) -> String {
        scheduler_execution_input::compose_coordination_verification_input(
            original_input,
            state,
            plan,
            round,
            execution_output,
        )
    }

    pub(super) fn compose_coordination_gate_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> String {
        scheduler_execution_input::compose_coordination_gate_input(
            original_input,
            state,
            plan,
            round,
            execution_output,
            review_output,
        )
    }

    pub(super) fn compose_autonomous_verification_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
    ) -> String {
        scheduler_execution_input::compose_autonomous_verification_input(
            original_input,
            state,
            plan,
            round,
            execution_output,
        )
    }

    pub(super) fn compose_autonomous_gate_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        execution_output: &OrchestratorOutput,
        verification_output: Option<&OrchestratorOutput>,
    ) -> String {
        scheduler_execution_input::compose_autonomous_gate_input(
            original_input,
            state,
            plan,
            round,
            execution_output,
            verification_output,
        )
    }

    pub(super) fn compose_retry_input(
        &self,
        original_input: &str,
        state: &SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        round: usize,
        decision: &SchedulerExecutionGateDecision,
        previous_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> String {
        scheduler_execution_input::compose_retry_input(
            original_input,
            state,
            plan,
            round,
            decision,
            previous_output,
            review_output,
        )
    }

    async fn execute_coordination_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        SchedulerExecutionService::new(self, original_input, state, plan, ctx)
            .run_coordination_loop()
            .await
    }

    async fn execute_autonomous_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        SchedulerExecutionService::new(self, original_input, state, plan, ctx)
            .run_autonomous_loop()
            .await
    }

    pub(super) fn stage_agent_from_policy(
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

    async fn execute_single_pass_execution_workflow(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        SchedulerExecutionService::new(self, original_input, state, plan, ctx)
            .run_single_pass()
            .await
    }

    async fn execute_execution_stage(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> Result<(), OrchestratorError> {
        let mut adapter = SchedulerExecutionStageAdapter {
            orchestrator: self,
            original_input,
            state,
            plan,
            ctx,
        };
        execute_scheduler_execution_stage_dispatch(plan.execution_stage_dispatch(), &mut adapter)
            .await
    }

    fn finalize_output(&self, state: SchedulerProfileState) -> OrchestratorOutput {
        let artifact_path = state.preset_runtime.planning_artifact_path.clone();
        let content = self
            .plan
            .final_output_priority()
            .iter()
            .find_map(|source| state.final_output_content(*source))
            .unwrap_or_else(|| state.route.request_brief.clone());
        let content = self
            .plan
            .normalize_final_output(&content)
            .unwrap_or(content);
        let content = append_artifact_note(content, artifact_path.as_deref());

        let mut metadata = HashMap::new();
        if let Some(output) = state.execution.delegated.as_ref() {
            merge_output_metadata(&mut metadata, &output.metadata);
        }
        if let Some(output) = state.execution.reviewed.as_ref() {
            merge_output_metadata(&mut metadata, &output.metadata);
        }
        if let Some(output) = state.execution.handed_off.as_ref() {
            merge_output_metadata(&mut metadata, &output.metadata);
        }
        if let Some(output) = state.execution.synthesized.as_ref() {
            merge_output_metadata(&mut metadata, &output.metadata);
        }

        self.plan
            .extend_final_output_metadata(&state, artifact_path.as_deref(), &mut metadata);

        if !state.metrics.usage.is_zero() {
            append_output_usage(&mut metadata, &state.metrics.usage);
        }

        OrchestratorOutput {
            content,
            steps: state.metrics.total_steps,
            tool_calls_count: state.metrics.total_tool_calls,
            metadata,
            finish_reason: if state.is_cancelled {
                crate::runtime::events::FinishReason::Cancelled
            } else {
                crate::runtime::events::FinishReason::EndTurn
            },
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

        // Resolve per-stage capabilities:
        // 1. If the stage spec has explicit capabilities, use those.
        // 2. Otherwise, for stages that delegate work (Plan, ExecutionOrchestration,
        //    Delegation), inherit from plan-level config.
        // 3. For stages that don't delegate (RequestAnalysis, Route, Interview,
        //    Review, Synthesis, Handoff), capabilities is None.
        let capabilities = plan
            .stage_graph()
            .stage(stage)
            .and_then(|spec| spec.capabilities.clone())
            .or_else(|| {
                if stage.needs_capabilities() {
                    Some(plan.resolve_capabilities())
                } else {
                    None
                }
            });

        ctx.lifecycle_hook
            .on_scheduler_stage_start(
                &ctx.exec_ctx.agent_name,
                stage.as_event_name(),
                stage_index,
                capabilities.as_ref(),
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

    pub(super) fn retry_budget_exhausted_output(
        plan: &SchedulerProfilePlan,
        round: usize,
        max_rounds: usize,
        decision: &SchedulerExecutionGateDecision,
        previous_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> OrchestratorOutput {
        let retry_focus = decision
            .next_input
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                let summary = decision.summary.trim();
                if summary.is_empty() {
                    "collect the missing proof for the unresolved gap"
                } else {
                    summary
                }
            });
        let orchestrator = plan.orchestrator.as_deref().unwrap_or("scheduler");
        let mut content = format!(
            "## Delivery Summary\n- {orchestrator} exhausted its bounded retry budget after round {round}/{max_rounds}.\n\n**Retry Status**\n- Retry budget exhausted.\n\n**Retry Focus**\n- {retry_focus}"
        );
        if !decision.summary.trim().is_empty() {
            content.push_str(&format!(
                "\n\n**Blockers or Risks**\n- {}",
                decision.summary.trim()
            ));
        }
        if let Some(review_output) = review_output {
            if !review_output.content.trim().is_empty() {
                content.push_str(&format!(
                    "\n\n**Verification**\n{}",
                    review_output.content.trim()
                ));
            }
        } else if !previous_output.content.trim().is_empty() {
            content.push_str(&format!(
                "\n\n**Verification**\n- Last execution output is preserved below.\n\n{}",
                previous_output.content.trim()
            ));
        }

        let mut metadata = previous_output.metadata.clone();
        if let Some(review_output) = review_output {
            merge_output_metadata(&mut metadata, &review_output.metadata);
        }
        metadata.insert(
            "scheduler_retry_budget_exhausted".to_string(),
            serde_json::json!(true),
        );
        metadata.insert(
            "scheduler_retry_round".to_string(),
            serde_json::json!(round),
        );
        metadata.insert(
            "scheduler_retry_limit".to_string(),
            serde_json::json!(max_rounds),
        );

        OrchestratorOutput {
            content,
            steps: previous_output.steps + review_output.map(|output| output.steps).unwrap_or(0),
            tool_calls_count: previous_output.tool_calls_count
                + review_output
                    .map(|output| output.tool_calls_count)
                    .unwrap_or(0),
            metadata,
            finish_reason: crate::runtime::events::FinishReason::EndTurn,
        }
    }

    pub(super) fn gate_terminal_output(
        plan: &SchedulerProfilePlan,
        status: SchedulerExecutionGateStatus,
        decision: &SchedulerExecutionGateDecision,
        fallback_output: &OrchestratorOutput,
    ) -> Option<OrchestratorOutput> {
        plan.resolve_gate_terminal_content(status, decision, &fallback_output.content)
            .map(|content| OrchestratorOutput {
                content,
                ..fallback_output.clone()
            })
    }

    pub(super) fn record_output(state: &mut SchedulerProfileState, output: &OrchestratorOutput) {
        state.metrics.total_steps += output.steps;
        state.metrics.total_tool_calls += output.tool_calls_count;
        if let Some(usage) = output_usage(&output.metadata) {
            state.metrics.usage.accumulate(&usage);
        }
        if output.is_cancelled() {
            state.is_cancelled = true;
        }
    }

    pub(super) fn sync_preset_runtime_authority(
        plan: &SchedulerProfilePlan,
        state: &mut SchedulerProfileState,
        ctx: &OrchestratorContext,
    ) {
        plan.sync_runtime_authority(&mut state.preset_runtime, ctx);
        Self::sanitize_runtime_artifact_paths(plan, state, ctx);
    }

    fn sanitize_runtime_artifact_paths(
        plan: &SchedulerProfilePlan,
        state: &mut SchedulerProfileState,
        ctx: &OrchestratorContext,
    ) {
        if let Some(path) = state.preset_runtime.planning_artifact_path.clone() {
            if let Err(error) = plan.validate_runtime_artifact_path(&path, &ctx.exec_ctx) {
                tracing::warn!(error = %error, path = %path, orchestrator = ?plan.orchestrator, "scheduler planning artifact path rejected by runtime authority");
                state.preset_runtime.planning_artifact_path = None;
                state.preset_runtime.planned = None;
                state.preset_runtime.ground_truth_context = None;
            }
        }

        if let Some(path) = state.preset_runtime.draft_artifact_path.clone() {
            if let Err(error) = plan.validate_runtime_artifact_path(&path, &ctx.exec_ctx) {
                tracing::warn!(error = %error, path = %path, orchestrator = ?plan.orchestrator, "scheduler draft artifact path rejected by runtime authority");
                state.preset_runtime.draft_artifact_path = None;
                state.preset_runtime.draft_snapshot = None;
            }
        }
    }

    pub(super) fn retry_continuation_targets(
        previous_output: &OrchestratorOutput,
        review_output: Option<&OrchestratorOutput>,
    ) -> Vec<ContinuationTarget> {
        let mut metadata = previous_output.metadata.clone();
        if let Some(review_output) = review_output {
            merge_output_metadata(&mut metadata, &review_output.metadata);
        }
        continuation_targets(&metadata)
    }

    pub(super) fn render_retry_continuation_candidates(
        targets: &[ContinuationTarget],
    ) -> Option<String> {
        let rendered = targets
            .iter()
            .map(|target| {
                let mut parts = vec![format!("session_id: `{}`", target.session_id)];
                if let Some(agent_task_id) = target.agent_task_id.as_deref() {
                    parts.push(format!("agent_task_id: `{agent_task_id}`"));
                }
                if let Some(tool_name) = target.tool_name.as_deref() {
                    parts.push(format!("tool: `{tool_name}`"));
                }
                format!("- {}", parts.join(" | "))
            })
            .collect::<Vec<_>>()
            .join("\n");
        (!rendered.is_empty()).then_some(rendered)
    }

    async fn execute_orchestration_tool(
        tool_name: &str,
        arguments: serde_json::Value,
        plan: &SchedulerProfilePlan,
        state: &mut SchedulerProfileState,
        ctx: &OrchestratorContext,
    ) -> Result<crate::ToolOutput, OrchestratorError> {
        plan.validate_runtime_orchestration_tool(tool_name)
            .map_err(|error| OrchestratorError::ToolError {
                tool: tool_name.to_string(),
                error,
            })?;
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
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let agent = ctx
            .agent_resolver
            .resolve(name)
            .ok_or_else(|| OrchestratorError::AgentNotFound(name.to_string()))?;
        execute_stage_agent(input, ctx, agent, policy, stage_context).await
    }

    async fn register_scheduler_workflow_todos(
        &self,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) {
        if state.preset_runtime.workflow_todos_registered {
            return;
        }

        let Some(payload) = plan.workflow_todos_payload() else {
            return;
        };

        match Self::execute_orchestration_tool("todowrite", payload, plan, state, ctx).await {
            Ok(_) => {
                state.preset_runtime.workflow_todos_registered = true;
            }
            Err(error) => {
                tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "scheduler workflow todo registration failed; continuing");
            }
        }
    }

    async fn request_capability_advisory_review(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) {
        let Some(agent_name) = plan.advisory_agent_name() else {
            return;
        };
        let Some(advisory_input) =
            plan.compose_advisory_review_input(SchedulerAdvisoryReviewInput {
                goal: &state.route.request_brief,
                original_request: original_input,
                discussed: state.route.interviewed.as_deref(),
                draft_context: state.preset_runtime.draft_snapshot.as_deref(),
                research: state.route.routed.as_deref(),
            })
        else {
            return;
        };
        match self
            .execute_resolved_agent(
                agent_name,
                &advisory_input,
                ctx,
                StageToolPolicy::AllowReadOnly,
                None,
            )
            .await
        {
            Ok(output) => {
                Self::record_output(state, &output);
                if let Some(update) =
                    plan.runtime_update_for_advisory_review(output.content.clone())
                {
                    state.apply_runtime_update(update);
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "preset advisory review failed; continuing without advisory feedback");
            }
        }
    }

    async fn request_capability_user_choice(
        &self,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> String {
        let Some(payload) = plan.user_choice_payload() else {
            return String::new();
        };
        let default_choice = plan.default_user_choice().unwrap_or("").to_string();

        let answer = match Self::execute_orchestration_tool("question", payload, plan, state, ctx)
            .await
        {
            Ok(output) => plan
                .parse_user_choice(&output.output)
                .unwrap_or_else(|| default_choice.clone()),
            Err(error) => {
                tracing::warn!(error = %error, orchestrator = ?plan.orchestrator, "preset user choice prompt failed; defaulting to configured choice");
                default_choice
            }
        };

        if let Some(update) = plan.runtime_update_for_user_choice(answer.clone()) {
            state.apply_runtime_update(update);
        }
        answer
    }

    async fn run_capability_approval_review_loop(
        &self,
        original_input: &str,
        state: &mut SchedulerProfileState,
        plan: &SchedulerProfilePlan,
        ctx: &OrchestratorContext,
    ) -> bool {
        let Some(plan_path) = state.preset_runtime.planning_artifact_path.clone() else {
            tracing::warn!(orchestrator = ?plan.orchestrator, "preset approval review loop skipped because no plan artifact path was available");
            return false;
        };
        let Some(agent_name) = plan.approval_review_agent_name() else {
            return false;
        };
        let Some(max_rounds) = plan.max_approval_review_rounds() else {
            return false;
        };

        for round in 1..=max_rounds {
            let review = match self
                .execute_resolved_agent(agent_name, &plan_path, ctx, StageToolPolicy::AllowReadOnly, None)
                .await
            {
                Ok(output) => output,
                Err(error) => {
                    tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "preset approval review agent failed");
                    return false;
                }
            };
            Self::record_output(state, &review);
            if let Some(update) = plan.runtime_update_for_approval_review(review.content.clone()) {
                state.apply_runtime_update(update);
            }
            if let Err(error) = Self::sync_runtime_draft_artifact(original_input, plan, state, ctx)
            {
                tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to sync draft after approval review loop");
            }

            if plan.approval_review_is_accepted(&review.content) {
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
                        tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to persist regenerated plan after approval review loop");
                    }
                    if let Err(error) =
                        Self::sync_runtime_draft_artifact(original_input, plan, state, ctx)
                    {
                        tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "failed to sync draft after plan regeneration");
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, round, orchestrator = ?plan.orchestrator, "preset approval review loop failed to regenerate plan");
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
            user_choice: state.preset_runtime.user_choice.clone(),
            review_gate_approved: state.preset_runtime.review_gate_approved,
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
                        Self::sync_runtime_draft_artifact(original_input, plan, state, ctx)
                    {
                        tracing::warn!(error = %error, stage = stage.as_event_name(), "scheduler effect failed to sync preset draft artifact");
                    }
                }
                SchedulerEffectKind::RegisterWorkflowTodos => {
                    self.register_scheduler_workflow_todos(state, plan, ctx)
                        .await;
                }
                SchedulerEffectKind::RequestAdvisoryReview => {
                    self.request_capability_advisory_review(original_input, state, plan, ctx)
                        .await;
                }
                SchedulerEffectKind::RequestUserChoice
                | SchedulerEffectKind::RunApprovalReviewLoop
                | SchedulerEffectKind::DeleteDraftArtifact
                | SchedulerEffectKind::DecorateFinalOutput => {}
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
            state.preset_runtime.user_choice.as_deref(),
            state.preset_runtime.review_gate_approved,
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
        Self::sync_preset_runtime_authority(&self.plan, &mut state, ctx);
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
                        .execute_route_stage(
                            input,
                            &state.route.request_brief,
                            &resolved_plan,
                            ctx,
                            Some((
                                SchedulerStageKind::Route.as_event_name().to_string(),
                                stage_ordinal,
                            )),
                        )
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
                                        metadata: {
                                            let mut metadata = HashMap::new();
                                            if !state.metrics.usage.is_zero() {
                                                append_output_usage(
                                                    &mut metadata,
                                                    &state.metrics.usage,
                                                );
                                            }
                                            metadata
                                        },
                                        finish_reason: crate::runtime::events::FinishReason::EndTurn,
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
                        .execute_interview_stage(
                            input,
                            &state,
                            &resolved_plan,
                            ctx,
                            Some((
                                SchedulerStageKind::Interview.as_event_name().to_string(),
                                stage_ordinal,
                            )),
                        )
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
                        .execute_plan_stage(
                            input,
                            &state,
                            &resolved_plan,
                            ctx,
                            Some((
                                SchedulerStageKind::Plan.as_event_name().to_string(),
                                stage_ordinal,
                            )),
                        )
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
                            .execute_delegation_stage(
                                &delegation_input,
                                &resolved_plan,
                                ctx,
                                Some((
                                    SchedulerStageKind::Delegation.as_event_name().to_string(),
                                    stage_ordinal,
                                )),
                            )
                            .await?;
                        Self::record_output(&mut state, &output);
                        state.execution.delegated = Some(output);
                    }
                }
                SchedulerStageKind::ExecutionOrchestration => {
                    if state.route.request_brief.is_empty() {
                        state.route.request_brief = self.compose_request_analysis_input(input);
                    }

                    self.execute_execution_stage(input, &mut state, &resolved_plan, ctx)
                        .await?;

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
                        let review_stage_context = Some((
                            SchedulerStageKind::Review.as_event_name().to_string(),
                            stage_ordinal,
                        ));
                        let output = self
                            .execute_review_stage(&review_input, &resolved_plan, ctx, review_stage_context)
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
                            .execute_review_stage(
                                &review_input,
                                &resolved_plan,
                                ctx,
                                Some((
                                    SchedulerStageKind::Review.as_event_name().to_string(),
                                    stage_ordinal,
                                )),
                            )
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
                        .execute_synthesis_stage(
                            input,
                            &state,
                            &resolved_plan,
                            ctx,
                            Some((
                                SchedulerStageKind::Synthesis.as_event_name().to_string(),
                                stage_ordinal,
                            )),
                        )
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
                        .execute_handoff_stage(
                            input,
                            &state,
                            &resolved_plan,
                            ctx,
                            Some((
                                SchedulerStageKind::Handoff.as_event_name().to_string(),
                                stage_ordinal,
                            )),
                        )
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
            // ── Cancellation check: if any stage was cancelled, terminate scheduler ──
            if state.is_cancelled {
                tracing::info!(
                    stage = ?stage,
                    stage_idx,
                    "scheduler cancelled during stage; terminating"
                );
                break;
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

pub fn parse_execution_gate_decision(output: &str) -> Option<SchedulerExecutionGateDecision> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    for candidate in profile_json_candidates(trimmed) {
        if let Some(decision) = parse_execution_gate_candidate(&candidate) {
            return Some(decision);
        }
    }

    None
}

fn parse_execution_gate_candidate(candidate: &str) -> Option<SchedulerExecutionGateDecision> {
    if let Ok(decision) = serde_json::from_str::<SchedulerExecutionGateDecision>(candidate) {
        return Some(normalize_execution_gate_decision(decision));
    }

    let value = serde_json::from_str::<Value>(candidate).ok()?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| value.get("gate_decision").and_then(Value::as_str))
        .and_then(parse_execution_gate_status_token)?;

    let summary = first_non_empty_string(&[
        value.get("summary").and_then(Value::as_str),
        value.get("reasoning").and_then(Value::as_str),
        value.get("execution_fidelity").and_then(Value::as_str),
    ])
    .unwrap_or_default()
    .to_string();

    let next_input = first_non_empty_string(&[
        value.get("next_input").and_then(Value::as_str),
        joined_string_array(value.get("next_actions")).as_deref(),
    ])
    .map(str::to_string);

    let final_response = first_non_empty_string(&[
        value.get("final_response").and_then(Value::as_str),
        build_legacy_gate_details_markdown(&value).as_deref(),
    ])
    .map(str::to_string);

    Some(normalize_execution_gate_decision(
        SchedulerExecutionGateDecision {
            status,
            summary,
            next_input,
            final_response,
        },
    ))
}

fn parse_execution_gate_status_token(token: &str) -> Option<SchedulerExecutionGateStatus> {
    match token.trim().to_ascii_lowercase().as_str() {
        "done" | "complete" | "completed" | "finish" | "finished" => {
            Some(SchedulerExecutionGateStatus::Done)
        }
        "continue" | "retry" | "again" => Some(SchedulerExecutionGateStatus::Continue),
        "blocked" | "block" | "stop" => Some(SchedulerExecutionGateStatus::Blocked),
        _ => None,
    }
}

fn first_non_empty_string<'a>(candidates: &[Option<&'a str>]) -> Option<&'a str> {
    candidates
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn joined_string_array(value: Option<&Value>) -> Option<String> {
    let items = value?.as_array()?;
    let lines = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn build_legacy_gate_details_markdown(value: &Value) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(summary) = value
        .get("verification_summary")
        .and_then(Value::as_object)
        .filter(|summary| !summary.is_empty())
    {
        let mut lines = Vec::new();
        for (key, raw) in summary {
            let rendered = match raw {
                Value::String(text) => text.clone(),
                _ => raw.to_string(),
            };
            lines.push(format!("- {}: {}", key.replace('_', " "), rendered));
        }
        if !lines.is_empty() {
            sections.push(format!("### Verification Summary\n{}", lines.join("\n")));
        }
    }

    if let Some(task_status) = value
        .get("task_status")
        .and_then(Value::as_object)
        .filter(|status| !status.is_empty())
    {
        let mut lines = Vec::new();
        for (key, raw) in task_status {
            let rendered = raw.as_str().unwrap_or_default().trim();
            if !rendered.is_empty() {
                lines.push(format!("- {}: {}", key.replace('_', " "), rendered));
            }
        }
        if !lines.is_empty() {
            sections.push(format!("### Task Status\n{}", lines.join("\n")));
        }
    }

    if let Some(execution_fidelity) = value
        .get("execution_fidelity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("### Execution Fidelity\n{}", execution_fidelity));
    }

    if let Some(minor_issues) = joined_string_array(value.get("minor_issues")) {
        sections.push(format!("### Minor Issues\n{}", minor_issues));
    }

    if let Some(next_actions) = joined_string_array(value.get("next_actions")) {
        sections.push(format!("### Next Actions\n{}", next_actions));
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

pub fn normalize_execution_gate_decision(
    mut decision: SchedulerExecutionGateDecision,
) -> SchedulerExecutionGateDecision {
    decision.summary = decision.summary.trim().to_string();
    decision.next_input = decision
        .next_input
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    decision.final_response = decision
        .final_response
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if matches!(decision.status, SchedulerExecutionGateStatus::Continue)
        && decision.next_input.is_none()
    {
        let fallback = if decision.summary.is_empty() {
            "continue the bounded retry on the unresolved gap and collect concrete verification evidence"
                .to_string()
        } else {
            decision.summary.clone()
        };
        decision.next_input = Some(fallback);
    }

    decision
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

pub(super) fn skill_tree_context(plan: &SchedulerProfilePlan) -> Option<&str> {
    plan.skill_tree
        .as_ref()
        .map(|tree| tree.context_markdown.trim())
        .filter(|context| !context.is_empty())
}

pub(super) fn render_plan_snapshot(plan: &SchedulerProfilePlan) -> String {
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

pub(super) fn profile_prompt_suffix(plan: &SchedulerProfilePlan) -> String {
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
    use super::super::presets::prometheus_planning_stage_tool_policy;
    use super::super::profile_state::{
        SchedulerExecutionState, SchedulerMetricsState, SchedulerPresetRuntimeState,
        SchedulerRouteState,
    };
    use super::*;
    use crate::runtime::events::FinishReason;
    use crate::traits::{AgentResolver, ModelResolver, NoopLifecycleHook, ToolExecutor};
    use crate::{
        AgentDescriptor, DirectKind, ExecutionContext, ModelRef, Orchestrator, OrchestratorContext,
        ReviewMode, SchedulerEffectKind, SchedulerEffectMoment, SchedulerEffectSpec,
        SchedulerExecutionChildMode, SchedulerExecutionVerificationMode, SchedulerTransitionSpec,
        SchedulerTransitionTarget, SchedulerTransitionTrigger, ToolExecError, ToolOutput,
    };
    use async_trait::async_trait;
    use futures::stream;
    use std::collections::HashMap;
    use std::fs;
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

    fn runtime_execution_plan(orchestrator: &str) -> SchedulerProfilePlan {
        SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
            .with_orchestrator(orchestrator)
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
                finish_reason: FinishReason::EndTurn,
                }),
                handed_off: Some(OrchestratorOutput {
                    content: "handoff".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                finish_reason: FinishReason::EndTurn,
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
                ..Default::default()
            },
            is_cancelled: false,
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Plan Summary"));
        assert!(output.content.contains("**Recommended Next Step**"));
        assert!(output.content.contains("- handoff"));
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
                finish_reason: FinishReason::EndTurn,
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**Delegation Path**"));
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
                finish_reason: FinishReason::EndTurn,
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**Task Status**"));
        assert!(output.content.contains("**Verification**"));
        assert!(output.content.contains("**Gate Decision**"));
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
                finish_reason: FinishReason::EndTurn,
                }),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Delivery Summary"));
        assert!(output.content.contains("**Completion Status**"));
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
                finish_reason: FinishReason::EndTurn,
                }),
                reviewed: Some(OrchestratorOutput {
                    content: "task A verified".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                finish_reason: FinishReason::EndTurn,
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
            metadata: HashMap::from([(
                "continuationTargets".to_string(),
                serde_json::json!([{
                    "sessionId": "task_build_42",
                    "agentTaskId": "agent-task-42",
                    "toolName": "task_flow"
                }]),
            )]),
            finish_reason: FinishReason::EndTurn,
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
            metadata: HashMap::from([(
                "continuationTargets".to_string(),
                serde_json::json!([{
                    "sessionId": "task_build_42",
                    "agentTaskId": "agent-task-42",
                    "toolName": "task_flow"
                }]),
            )]),
            finish_reason: FinishReason::EndTurn,
        };
        let review = OrchestratorOutput {
            content: "task A verified, task B weak".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
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
    fn atlas_retry_input_uses_preset_authority() {
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
            metadata: HashMap::from([(
                "continuationTargets".to_string(),
                serde_json::json!([{
                    "sessionId": "task_build_42",
                    "agentTaskId": "agent-task-42",
                    "toolName": "task_flow"
                }]),
            )]),
            finish_reason: FinishReason::EndTurn,
        };
        let review = OrchestratorOutput {
            content: "task A verified, task B weak".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
        };
        let decision = SchedulerExecutionGateDecision {
            status: SchedulerExecutionGateStatus::Continue,
            summary: "task B still needs concrete verification".to_string(),
            next_input: Some("continue task B and verify the migration path".to_string()),
            final_response: None,
        };

        let input = orchestrator.compose_retry_input(
            "ship the migration cleanup plan",
            &state,
            &orchestrator.plan,
            2,
            &decision,
            &execution,
            Some(&review),
        );

        assert!(input.contains("## Stage\ncoordination-retry"));
        assert!(input.contains("Continuation Authority"));
        assert!(input.contains("active boulder state"));
        assert!(input.contains("Carry forward inherited notepad decisions"));
        assert!(input.contains("Preferred Continuation"));
        assert!(input.contains("task_build_42"));
        assert!(input.contains("agent-task-42"));
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
        finish_reason: FinishReason::EndTurn,
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
        finish_reason: FinishReason::EndTurn,
        };
        let verification = OrchestratorOutput {
            content: "targeted check passed".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
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
    fn hephaestus_done_gate_prefers_execution_output_when_final_response_missing() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("hephaestus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let execution = OrchestratorOutput {
            content: "fixed the diagnostics path and ran the targeted check".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
        };
        let decision = SchedulerExecutionGateDecision {
            status: SchedulerExecutionGateStatus::Done,
            summary: "verified".to_string(),
            next_input: None,
            final_response: None,
        };

        let output = SchedulerProfileOrchestrator::gate_terminal_output(
            &orchestrator.plan,
            SchedulerExecutionGateStatus::Done,
            &decision,
            &execution,
        )
        .expect("done gate should resolve execution output");

        assert_eq!(
            output.content,
            "fixed the diagnostics path and ran the targeted check"
        );
    }

    #[test]
    fn sisyphus_done_gate_prefers_execution_output_when_final_response_missing() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("sisyphus"),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let execution = OrchestratorOutput {
            content: "shipped the change and verified the targeted behavior".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
        };
        let decision = SchedulerExecutionGateDecision {
            status: SchedulerExecutionGateStatus::Done,
            summary: "verified".to_string(),
            next_input: None,
            final_response: None,
        };

        let output = SchedulerProfileOrchestrator::gate_terminal_output(
            &orchestrator.plan,
            SchedulerExecutionGateStatus::Done,
            &decision,
            &execution,
        )
        .expect("done gate should resolve execution output");

        assert_eq!(
            output.content,
            "shipped the change and verified the targeted behavior"
        );
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
        assert_eq!(constrained.mode, RouteMode::Orchestrate);
        assert_eq!(constrained.review_mode, Some(ReviewMode::Normal));
    }

    #[test]
    fn constrain_route_decision_forces_prometheus_direct_reply_back_into_workflow() {
        let decision = RouteDecision {
            mode: RouteMode::Direct,
            direct_kind: Some(DirectKind::Reply),
            direct_response: Some("Hi!".to_string()),
            preset: None,
            insert_plan_stage: None,
            review_mode: None,
            context_append: None,
            rationale_summary: "greeting".to_string(),
        };

        let constrained = SchedulerPresetKind::Prometheus
            .definition()
            .constrain_route_decision(decision);

        assert_eq!(constrained.mode, RouteMode::Orchestrate);
        assert_eq!(constrained.preset.as_deref(), Some("prometheus"));
        assert_eq!(constrained.review_mode, Some(ReviewMode::Normal));
        assert_eq!(constrained.direct_kind, None);
        assert_eq!(constrained.direct_response, None);
    }

    #[test]
    fn request_analysis_input_includes_prometheus_workflow_constraint() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );

        let input = orchestrator.compose_request_analysis_input("Plan the TUI workflow polish");

        assert!(input.contains("## Workflow Constraint"));
        assert!(input.contains("planner-only behavior"));
        assert!(input.contains("Do NOT convert this request into a direct reply"));
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
            prometheus_planning_stage_tool_policy()
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
            effect: SchedulerEffectKind::RequestAdvisoryReview,
        }));
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::BeforeTransition,
            effect: SchedulerEffectKind::RunApprovalReviewLoop,
        }));
        assert!(effects.effects.contains(&SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::DecorateFinalOutput,
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
                user_choice: Some("Start Work".to_string()),
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
                user_choice: Some("High Accuracy Review".to_string()),
                review_gate_approved: Some(false),
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
            SchedulerLoopBudget::StepLimit(1)
        );
    }

    #[test]
    fn stage_observability_exposes_policy_matrix_for_public_presets() {
        let prometheus_handoff =
            SchedulerPresetKind::Prometheus.stage_observability(SchedulerStageKind::Handoff);
        assert_eq!(prometheus_handoff.projection, "transcript");
        assert_eq!(
            prometheus_handoff.tool_policy,
            "restricted:prometheus-planning-artifacts"
        );
        assert_eq!(prometheus_handoff.loop_budget, "step-limit:1");

        let atlas_execution = SchedulerPresetKind::Atlas
            .stage_observability(SchedulerStageKind::ExecutionOrchestration);
        assert_eq!(atlas_execution.projection, "transcript");
        assert_eq!(atlas_execution.tool_policy, "allow-all");
        assert_eq!(atlas_execution.loop_budget, "unbounded");
    }

    #[test]
    fn normalize_execution_gate_decision_backfills_bounded_retry_focus() {
        let normalized = normalize_execution_gate_decision(SchedulerExecutionGateDecision {
            status: SchedulerExecutionGateStatus::Continue,
            summary: "verify the remaining diagnostics proof".to_string(),
            next_input: Some("   ".to_string()),
            final_response: Some("  ".to_string()),
        });

        assert_eq!(
            normalized.next_input.as_deref(),
            Some("verify the remaining diagnostics proof")
        );
        assert!(normalized.final_response.is_none());
    }

    #[test]
    fn parse_execution_gate_decision_accepts_legacy_atlas_gate_shape() {
        let output = r#"```json
{
  "gate_decision": "done",
  "reasoning": "All delegated work verified complete.",
  "verification_summary": {
    "total_tasks": 8,
    "completed": 8
  },
  "task_status": {
    "task_1": "done - verified"
  },
  "execution_fidelity": "correct - planner-only workflow preserved",
  "minor_issues": ["Task wording mismatch"],
  "next_actions": ["No further execution needed"]
}
```"#;

        let decision =
            parse_execution_gate_decision(output).expect("legacy gate decision should parse");
        assert_eq!(decision.status, SchedulerExecutionGateStatus::Done);
        assert_eq!(decision.summary, "All delegated work verified complete.");
        let final_response = decision
            .final_response
            .expect("legacy gate should synthesize detail section");
        assert!(final_response.contains("Verification Summary"));
        assert!(final_response.contains("Task Status"));
        assert!(final_response.contains("Execution Fidelity"));
        assert!(final_response.contains("Minor Issues"));
    }

    #[test]
    fn retry_budget_exhausted_output_marks_explicit_terminal_state() {
        let execution = OrchestratorOutput {
            content: "worker result".to_string(),
            steps: 2,
            tool_calls_count: 1,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
        };
        let verification = OrchestratorOutput {
            content: "verification evidence".to_string(),
            steps: 1,
            tool_calls_count: 0,
            metadata: HashMap::new(),
        finish_reason: FinishReason::EndTurn,
        };
        let output = SchedulerProfileOrchestrator::retry_budget_exhausted_output(
            &SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
                .with_orchestrator("atlas"),
            3,
            3,
            &SchedulerExecutionGateDecision {
                status: SchedulerExecutionGateStatus::Continue,
                summary: "task B still lacks proof".to_string(),
                next_input: None,
                final_response: None,
            },
            &execution,
            Some(&verification),
        );

        assert!(output
            .content
            .contains("exhausted its bounded retry budget"));
        assert!(output.content.contains("task B still lacks proof"));
        assert!(output.content.contains("verification evidence"));
        assert_eq!(
            output
                .metadata
                .get("scheduler_retry_budget_exhausted")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn sisyphus_effect_dispatch_uses_shared_scheduler_protocol() {
        let plan = SchedulerProfilePlan::new(vec![SchedulerStageKind::Review])
            .with_orchestrator("sisyphus");
        let dispatch = plan.effect_dispatch(
            SchedulerEffectKind::RequestAdvisoryReview,
            SchedulerEffectContext {
                planning_artifact_path: None,
                draft_artifact_path: None,
                user_choice: None,
                review_gate_approved: None,
                draft_exists: true,
            },
        );

        assert_eq!(dispatch, SchedulerEffectDispatch::RequestAdvisoryReview);
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
                review_gate_approved: None,
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
                advisory_review: Some(
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
                ..Default::default()
            },
            ..Default::default()
        };
        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("# Plan"));
        assert!(output
            .content
            .contains(".sisyphus/plans/plan-demo-session.md"));
    }

    #[test]
    fn finalize_output_normalizes_prometheus_review_into_handoff_delivery() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            execution: SchedulerExecutionState {
                reviewed: Some(OrchestratorOutput {
                    content: r#"## Plan Generated: scheduler

**Key Decisions Made**
- Keep Prometheus planner-only.

**Scope**
- IN: planner workflow
- OUT: code execution

**Guardrails Applied**
- Preserve slash palette.

**Auto-Resolved**
- None.

**Defaults Applied**
- Keep review enabled before handoff.

**Decisions Needed**
- None.

**Handoff Readiness**
- Ready for handoff once the reviewed plan is accepted.

**Review Notes**
- Plan is consistent."#
                        .to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                finish_reason: FinishReason::EndTurn,
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert!(output.content.contains("## Plan Summary"));
        assert!(output.content.contains("**Recommended Next Step**"));
        assert!(output.content.contains("Prometheus remains planner-only"));
    }

    #[test]
    fn finalize_output_emits_precise_prometheus_handoff_command_metadata() {
        let orchestrator = SchedulerProfileOrchestrator::new(
            planner_only_plan(),
            ToolRunner::new(Arc::new(NoopToolExecutor)),
        );
        let state = SchedulerProfileState {
            execution: SchedulerExecutionState {
                handed_off: Some(OrchestratorOutput {
                    content: "## Plan Summary\n- Ready.".to_string(),
                    steps: 1,
                    tool_calls_count: 0,
                    metadata: HashMap::new(),
                finish_reason: FinishReason::EndTurn,
                }),
                ..Default::default()
            },
            preset_runtime: SchedulerPresetRuntimeState {
                user_choice: Some("Start Work".to_string()),
                planning_artifact_path: Some(".sisyphus/plans/plan-demo-session.md".to_string()),
                ..Default::default()
            },
            metrics: SchedulerMetricsState {
                total_steps: 1,
                total_tool_calls: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let output = orchestrator.finalize_output(state);
        assert_eq!(
            output
                .metadata
                .get("scheduler_handoff_mode")
                .and_then(|value| value.as_str()),
            Some("atlas")
        );
        assert_eq!(
            output
                .metadata
                .get("scheduler_handoff_command")
                .and_then(|value| value.as_str()),
            Some("/start-work plan-demo-session")
        );
        assert_eq!(
            output
                .metadata
                .get("scheduler_handoff_plan_path")
                .and_then(|value| value.as_str()),
            Some(".sisyphus/plans/plan-demo-session.md")
        );
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

    #[test]
    fn atlas_runtime_authority_sync_loads_boulder_plan_snapshot() {
        let workdir = new_temp_workdir();
        let plan_path = workdir.join(".sisyphus/plans/demo.md");
        fs::create_dir_all(plan_path.parent().expect("plan parent")).expect("plan dir");
        fs::write(&plan_path, "- [ ] task A\n- [x] task B\n").expect("plan should write");
        fs::write(
            workdir.join(".sisyphus/boulder.json"),
            format!(
                r#"{{
  "active_plan": "{}",
  "started_at": "2026-03-09T00:00:00Z",
  "session_ids": ["ses-1", "ses-2"],
  "plan_name": "demo",
  "agent": "atlas",
  "worktree_path": "/tmp/demo-worktree"
}}"#,
                plan_path.display()
            ),
        )
        .expect("boulder should write");

        let mut state = SchedulerProfileState::default();
        let ctx = test_context(&workdir, "atlas-session", Vec::new());
        let plan = SchedulerProfilePlan::new(vec![SchedulerStageKind::ExecutionOrchestration])
            .with_orchestrator("atlas");
        SchedulerProfileOrchestrator::sync_preset_runtime_authority(&plan, &mut state, &ctx);

        assert_eq!(
            state.preset_runtime.planning_artifact_path.as_deref(),
            Some(".sisyphus/plans/demo.md")
        );
        assert!(state
            .preset_runtime
            .planned
            .as_deref()
            .unwrap_or_default()
            .contains("- [ ] task A"));
        let ground_truth = state
            .preset_runtime
            .ground_truth_context
            .as_deref()
            .unwrap_or_default();
        assert!(ground_truth.contains("boulder_state_path"));
        assert!(ground_truth.contains("tracked_sessions: `2`"));
        assert!(ground_truth.contains("/tmp/demo-worktree"));
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
            cancel_token: Arc::new(crate::runtime::events::NeverCancel),
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
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.as_str() == "todowrite")
                .count(),
            1
        );
        assert!(calls.iter().any(|call| call == "question"));
        assert!(output.content.contains("/start-work"));
        assert!(output.content.contains("Plan saved to:"));
        assert!(!draft_path.exists());

        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[tokio::test]
    async fn prometheus_runtime_rejects_non_planner_orchestration_tools_before_execution() {
        let workdir = new_temp_workdir();
        let tool_executor = Arc::new(RecordingToolExecutor::default());
        let ctx = test_context_with_executor(
            &workdir,
            "runtime-tool-session",
            Vec::new(),
            tool_executor.clone(),
        );
        let mut state = SchedulerProfileState::default();

        let error = SchedulerProfileOrchestrator::execute_orchestration_tool(
            "write",
            serde_json::json!({
                "file_path": ".sisyphus/plans/demo.md",
                "content": "# not allowed"
            }),
            &planner_only_plan(),
            &mut state,
            &ctx,
        )
        .await
        .expect_err("prometheus runtime should reject non-orchestration tools");

        match error {
            OrchestratorError::ToolError { tool, error } => {
                assert_eq!(tool, "write");
                assert!(error.contains("question, todowrite"));
            }
            other => panic!("unexpected error: {other}"),
        }

        assert!(tool_executor.calls.lock().await.is_empty());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[tokio::test]
    async fn sisyphus_runtime_rejects_non_runtime_tool_before_execution() {
        let workdir = new_temp_workdir();
        let tool_executor = Arc::new(RecordingToolExecutor::default());
        let ctx = test_context_with_executor(
            &workdir,
            "sisyphus-runtime-tool-session",
            Vec::new(),
            tool_executor.clone(),
        );
        let mut state = SchedulerProfileState::default();

        let error = SchedulerProfileOrchestrator::execute_orchestration_tool(
            "question",
            serde_json::json!({"questions": [{"question": "Continue?"}]}),
            &runtime_execution_plan("sisyphus"),
            &mut state,
            &ctx,
        )
        .await
        .expect_err("sisyphus runtime should reject question tool");

        match error {
            OrchestratorError::ToolError { tool, error } => {
                assert_eq!(tool, "question");
                assert!(error.contains("todowrite"));
            }
            other => panic!("unexpected error: {other}"),
        }

        assert!(tool_executor.calls.lock().await.is_empty());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[tokio::test]
    async fn atlas_runtime_rejects_non_runtime_tool_before_execution() {
        let workdir = new_temp_workdir();
        let tool_executor = Arc::new(RecordingToolExecutor::default());
        let ctx = test_context_with_executor(
            &workdir,
            "atlas-runtime-tool-session",
            Vec::new(),
            tool_executor.clone(),
        );
        let mut state = SchedulerProfileState::default();

        let error = SchedulerProfileOrchestrator::execute_orchestration_tool(
            "question",
            serde_json::json!({"questions": [{"question": "Continue?"}]}),
            &runtime_execution_plan("atlas"),
            &mut state,
            &ctx,
        )
        .await
        .expect_err("atlas runtime should reject question tool");

        match error {
            OrchestratorError::ToolError { tool, error } => {
                assert_eq!(tool, "question");
                assert!(error.contains("todowrite"));
            }
            other => panic!("unexpected error: {other}"),
        }

        assert!(tool_executor.calls.lock().await.is_empty());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[tokio::test]
    async fn hephaestus_runtime_rejects_non_runtime_tool_before_execution() {
        let workdir = new_temp_workdir();
        let tool_executor = Arc::new(RecordingToolExecutor::default());
        let ctx = test_context_with_executor(
            &workdir,
            "hephaestus-runtime-tool-session",
            Vec::new(),
            tool_executor.clone(),
        );
        let mut state = SchedulerProfileState::default();

        let error = SchedulerProfileOrchestrator::execute_orchestration_tool(
            "question",
            serde_json::json!({"questions": [{"question": "Continue?"}]}),
            &runtime_execution_plan("hephaestus"),
            &mut state,
            &ctx,
        )
        .await
        .expect_err("hephaestus runtime should reject question tool");

        match error {
            OrchestratorError::ToolError { tool, error } => {
                assert_eq!(tool, "question");
                assert!(error.contains("todowrite"));
            }
            other => panic!("unexpected error: {other}"),
        }

        assert!(tool_executor.calls.lock().await.is_empty());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[test]
    fn prometheus_runtime_rejects_persisting_artifact_outside_sisyphus_markdown_scope() {
        let workdir = new_temp_workdir();
        let ctx = test_context(&workdir, "persist-guard-session", Vec::new());
        let mut state = SchedulerProfileState::default();
        state.preset_runtime.planning_artifact_path = Some("notes/demo.md".to_string());

        let error = SchedulerProfileOrchestrator::persist_artifact(
            &planner_only_plan(),
            SchedulerArtifactKind::Planning,
            "# invalid plan location",
            &mut state,
            &ctx,
        )
        .expect_err("prometheus runtime should reject invalid artifact location");

        assert!(error
            .to_string()
            .contains("only reference markdown artifacts under .sisyphus"));
        assert!(!workdir.join("notes/demo.md").exists());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[test]
    fn sisyphus_runtime_rejects_scheduler_artifact_persistence() {
        let workdir = new_temp_workdir();
        let ctx = test_context(&workdir, "sisyphus-artifact-session", Vec::new());
        let mut state = SchedulerProfileState::default();
        state.preset_runtime.planning_artifact_path = Some(".sisyphus/plans/demo.md".to_string());

        let error = SchedulerProfileOrchestrator::persist_artifact(
            &runtime_execution_plan("sisyphus"),
            SchedulerArtifactKind::Planning,
            "# should not exist",
            &mut state,
            &ctx,
        )
        .expect_err("sisyphus runtime should reject scheduler artifacts");

        assert!(error
            .to_string()
            .contains("does not manage scheduler artifacts"));
        assert!(!workdir.join(".sisyphus/plans/demo.md").exists());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[test]
    fn hephaestus_runtime_rejects_scheduler_artifact_persistence() {
        let workdir = new_temp_workdir();
        let ctx = test_context(&workdir, "hephaestus-artifact-session", Vec::new());
        let mut state = SchedulerProfileState::default();
        state.preset_runtime.planning_artifact_path = Some(".sisyphus/plans/demo.md".to_string());

        let error = SchedulerProfileOrchestrator::persist_artifact(
            &runtime_execution_plan("hephaestus"),
            SchedulerArtifactKind::Planning,
            "# should not exist",
            &mut state,
            &ctx,
        )
        .expect_err("hephaestus runtime should reject scheduler artifacts");

        assert!(error
            .to_string()
            .contains("does not manage scheduler artifacts"));
        assert!(!workdir.join(".sisyphus/plans/demo.md").exists());
        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
    }

    #[test]
    fn atlas_runtime_authority_rejects_external_ground_truth_plan_path() {
        let workdir = new_temp_workdir();
        let external_dir = new_temp_workdir();
        let external_plan = external_dir.join("external.md");
        fs::write(&external_plan, "- [ ] outside").expect("external plan should exist");
        fs::create_dir_all(workdir.join(".sisyphus")).expect("boulder dir should exist");
        fs::write(
            workdir.join(".sisyphus/boulder.json"),
            format!(
                r#"{{
  "active_plan": "{}",
  "started_at": "2026-03-09T00:00:00Z",
  "session_ids": ["ses-1"],
  "plan_name": "external",
  "agent": "atlas"
}}"#,
                external_plan.display()
            ),
        )
        .expect("boulder should write");

        let ctx = test_context(&workdir, "atlas-ground-truth-session", Vec::new());
        let mut state = SchedulerProfileState::default();
        SchedulerProfileOrchestrator::sync_preset_runtime_authority(
            &runtime_execution_plan("atlas"),
            &mut state,
            &ctx,
        );

        assert!(state.preset_runtime.planning_artifact_path.is_none());
        assert!(state.preset_runtime.planned.is_none());
        assert!(state.preset_runtime.ground_truth_context.is_none());

        std::fs::remove_dir_all(&workdir).expect("temp workdir should clean up");
        std::fs::remove_dir_all(&external_dir).expect("external temp workdir should clean up");
    }

    #[test]
    fn prometheus_runtime_rejects_deleting_artifact_outside_sisyphus_markdown_scope() {
        let workdir = new_temp_workdir();
        let invalid_path = workdir.join("notes/demo.md");
        fs::create_dir_all(invalid_path.parent().expect("invalid path parent"))
            .expect("invalid path dir");
        fs::write(&invalid_path, "draft").expect("invalid draft should exist");

        let ctx = test_context(&workdir, "delete-guard-session", Vec::new());
        let mut state = SchedulerProfileState::default();
        state.preset_runtime.draft_artifact_path = Some("notes/demo.md".to_string());

        let error = SchedulerProfileOrchestrator::delete_artifact(
            &planner_only_plan(),
            SchedulerArtifactKind::Draft,
            &mut state,
            &ctx,
        )
        .expect_err("prometheus runtime should reject invalid artifact deletion");

        assert!(error
            .to_string()
            .contains("only reference markdown artifacts under .sisyphus"));
        assert!(invalid_path.exists());
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
