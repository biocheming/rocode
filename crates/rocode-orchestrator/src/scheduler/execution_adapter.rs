use crate::agent_tree::{AgentTreeNode, AgentTreeOrchestrator, ChildExecutionMode};
use crate::skill_graph::{SkillGraphDefinition, SkillGraphOrchestrator};
use crate::traits::Orchestrator;
use crate::{OrchestratorContext, OrchestratorError, OrchestratorOutput};

use super::{
    execute_stage_agent, stage_agent_unbounded, SchedulerExecutionChildMode,
    SchedulerProfileOrchestrator, SchedulerProfilePlan, SchedulerStageKind, StageToolPolicy,
};

pub(super) struct SchedulerExecutionCapabilityAdapter<'a> {
    orchestrator: &'a SchedulerProfileOrchestrator,
    plan: &'a SchedulerProfilePlan,
    ctx: &'a OrchestratorContext,
}

impl<'a> SchedulerExecutionCapabilityAdapter<'a> {
    pub(super) fn new(
        orchestrator: &'a SchedulerProfileOrchestrator,
        plan: &'a SchedulerProfilePlan,
        ctx: &'a OrchestratorContext,
    ) -> Self {
        Self {
            orchestrator,
            plan,
            ctx,
        }
    }

    pub(super) async fn execute_agent_tree(
        &self,
        agent_tree: &AgentTreeNode,
        execution_input: &str,
        child_mode: SchedulerExecutionChildMode,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let mut tree =
            AgentTreeOrchestrator::new(agent_tree.clone(), self.orchestrator.tool_runner())
                .with_child_execution_mode(Self::child_execution_mode(child_mode));
        tree.execute(execution_input, self.ctx).await
    }

    pub(super) async fn execute_skill_graph(
        &self,
        skill_graph: &SkillGraphDefinition,
        execution_input: &str,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let mut graph =
            SkillGraphOrchestrator::new(skill_graph.clone(), self.orchestrator.tool_runner());
        if let Some((stage_name, stage_index)) = stage_context {
            graph.set_stage_context(stage_name, stage_index);
        }
        graph.execute(execution_input, self.ctx).await
    }

    pub(super) async fn execute_review_stage(
        &self,
        input: &str,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let profile_suffix = super::profile_prompt_suffix(self.plan);
        let prompt = self
            .plan
            .review_stage_prompt(&profile_suffix)
            .unwrap_or_else(|| {
                format!(
                    "You are the scheduler review layer.                      Audit the current result against the original request and return a tighter, evidence-based review.{}",
                    profile_suffix
                )
            });
        let stage_policy = self.plan.stage_policy(SchedulerStageKind::Review);
        execute_stage_agent(
            input,
            self.ctx,
            SchedulerProfileOrchestrator::stage_agent_from_policy(
                "scheduler-review",
                prompt,
                stage_policy,
            ),
            stage_policy.tool_policy,
            stage_context,
        )
        .await
    }

    pub(super) async fn execute_execution_fallback_stage(
        &self,
        input: &str,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        let profile_suffix = super::profile_prompt_suffix(self.plan);
        let prompt = self
            .plan
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
            self.ctx,
            stage_agent_unbounded("scheduler-execution", prompt),
            StageToolPolicy::AllowAll,
            stage_context,
        )
        .await
    }

    pub(super) async fn execute_execution_path(
        &self,
        execution_input: &str,
        child_mode: SchedulerExecutionChildMode,
        allow_execution_fallback: bool,
        stage_context: Option<(String, u32)>,
    ) -> Result<OrchestratorOutput, OrchestratorError> {
        if let Some(agent_tree) = &self.plan.agent_tree {
            self.execute_agent_tree(agent_tree, execution_input, child_mode)
                .await
        } else if let Some(skill_graph) = &self.plan.skill_graph {
            self.execute_skill_graph(skill_graph, execution_input, stage_context)
                .await
        } else if allow_execution_fallback {
            self.execute_execution_fallback_stage(execution_input, stage_context)
                .await
        } else {
            Err(Self::execution_unavailable_error(self.plan))
        }
    }

    fn child_execution_mode(mode: SchedulerExecutionChildMode) -> ChildExecutionMode {
        match mode {
            SchedulerExecutionChildMode::Parallel => ChildExecutionMode::Parallel,
            SchedulerExecutionChildMode::Sequential => ChildExecutionMode::Sequential,
        }
    }

    fn execution_unavailable_error(plan: &SchedulerProfilePlan) -> OrchestratorError {
        let orchestrator = plan.orchestrator.as_deref().unwrap_or("scheduler");
        OrchestratorError::Other(format!(
            "{orchestrator} execution requires an agent_tree or skill_graph"
        ))
    }
}
