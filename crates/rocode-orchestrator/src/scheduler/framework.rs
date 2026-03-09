use async_trait::async_trait;

use super::{SchedulerStageKind, StageToolPolicy};
use crate::OrchestratorError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerSessionProjection {
    Hidden,
    Transcript,
}

impl SchedulerSessionProjection {
    pub fn is_visible(self) -> bool {
        matches!(self, Self::Transcript)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerLoopBudget {
    Unbounded,
    StepLimit(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerStagePolicy {
    pub session_projection: SchedulerSessionProjection,
    pub tool_policy: StageToolPolicy,
    pub loop_budget: SchedulerLoopBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerStageSpec {
    pub kind: SchedulerStageKind,
    pub policy: SchedulerStagePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerStageGraph {
    pub stages: Vec<SchedulerStageSpec>,
}

impl SchedulerStageGraph {
    pub fn new(stages: Vec<SchedulerStageSpec>) -> Self {
        Self { stages }
    }

    pub fn stage(&self, kind: SchedulerStageKind) -> Option<&SchedulerStageSpec> {
        self.stages.iter().find(|stage| stage.kind == kind)
    }

    pub fn stage_kinds(&self) -> Vec<SchedulerStageKind> {
        self.stages.iter().map(|stage| stage.kind).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerTransitionTrigger {
    OnSuccess,
    OnUserChoice(&'static str),
    OnHighAccuracyApproved,
    OnHighAccuracyBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerTransitionTarget {
    Stage(SchedulerStageKind),
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerTransitionSpec {
    pub from: SchedulerStageKind,
    pub trigger: SchedulerTransitionTrigger,
    pub to: SchedulerTransitionTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerTransitionGraph {
    pub transitions: Vec<SchedulerTransitionSpec>,
}

impl SchedulerTransitionGraph {
    pub fn new(transitions: Vec<SchedulerTransitionSpec>) -> Self {
        Self { transitions }
    }

    pub fn transitions_from(&self, stage: SchedulerStageKind) -> Vec<&SchedulerTransitionSpec> {
        self.transitions
            .iter()
            .filter(|transition| transition.from == stage)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerEffectMoment {
    OnEnter,
    OnSuccess,
    BeforeTransition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerEffectKind {
    EnsurePlanningArtifactPath,
    PersistPlanningArtifact,
    PersistDraftArtifact,
    SyncDraftArtifact,
    RegisterWorkflowTodos,
    ConsultMetis,
    AskHandoffChoice,
    RunMomusReviewLoop,
    DeleteDraftArtifact,
    AppendStartWorkGuidance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerEffectSpec {
    pub stage: SchedulerStageKind,
    pub moment: SchedulerEffectMoment,
    pub effect: SchedulerEffectKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerEffectProtocol {
    pub effects: Vec<SchedulerEffectSpec>,
}

impl SchedulerEffectProtocol {
    pub fn new(effects: Vec<SchedulerEffectSpec>) -> Self {
        Self { effects }
    }

    pub fn effects_for(
        &self,
        stage: SchedulerStageKind,
        moment: SchedulerEffectMoment,
    ) -> Vec<&SchedulerEffectSpec> {
        self.effects
            .iter()
            .filter(|effect| effect.stage == stage && effect.moment == moment)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerEffectContext {
    pub planning_artifact_path: Option<String>,
    pub draft_artifact_path: Option<String>,
    pub user_choice: Option<String>,
    pub high_accuracy_approved: Option<bool>,
    pub draft_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerHandoffDecoration {
    pub plan_path: Option<String>,
    pub draft_path: Option<String>,
    pub draft_deleted: bool,
    pub recommend_start_work: bool,
    pub high_accuracy_approved: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerPresetRuntimeFields<'a> {
    pub route_rationale_summary: Option<&'a str>,
    pub planning_artifact_path: Option<&'a str>,
    pub draft_artifact_path: Option<&'a str>,
    pub interviewed: Option<&'a str>,
    pub planned: Option<&'a str>,
    pub draft_snapshot: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub momus_review: Option<&'a str>,
    pub handoff_choice: Option<&'a str>,
    pub high_accuracy_approved: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerPresetRuntimeUpdate {
    Planned(String),
    MetisReview(String),
    MomusReview(String),
    HandoffChoice(String),
    HighAccuracyApproved(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerEffectDispatch {
    EnsurePlanningArtifactPath,
    PersistPlanningArtifact,
    SyncDraftArtifact,
    RegisterWorkflowTodos,
    ConsultMetis,
    AskHandoffChoice,
    RunMomusReviewLoop,
    DeleteDraftArtifact,
    AppendStartWorkGuidance(SchedulerHandoffDecoration),
    Skip,
}

#[async_trait]
pub trait SchedulerPresetEffectExecutor {
    async fn ensure_planning_artifact_path(&mut self) -> Result<(), OrchestratorError>;
    async fn persist_planning_artifact(&mut self) -> Result<(), OrchestratorError>;
    async fn sync_draft_artifact(&mut self) -> Result<(), OrchestratorError>;
    async fn register_workflow_todos(&mut self) -> Result<(), OrchestratorError>;
    async fn consult_metis(&mut self) -> Result<(), OrchestratorError>;
    async fn ask_handoff_choice(&mut self) -> Result<(), OrchestratorError>;
    async fn run_momus_review_loop(&mut self) -> Result<(), OrchestratorError>;
    async fn delete_draft_artifact(&mut self) -> Result<(), OrchestratorError>;
    async fn decorate_handoff_output(
        &mut self,
        decoration: SchedulerHandoffDecoration,
    ) -> Result<(), OrchestratorError>;
}

pub async fn execute_scheduler_effect_dispatch<E: SchedulerPresetEffectExecutor>(
    dispatch: SchedulerEffectDispatch,
    executor: &mut E,
) -> Result<(), OrchestratorError> {
    match dispatch {
        SchedulerEffectDispatch::EnsurePlanningArtifactPath => {
            executor.ensure_planning_artifact_path().await
        }
        SchedulerEffectDispatch::PersistPlanningArtifact => {
            executor.persist_planning_artifact().await
        }
        SchedulerEffectDispatch::SyncDraftArtifact => executor.sync_draft_artifact().await,
        SchedulerEffectDispatch::RegisterWorkflowTodos => executor.register_workflow_todos().await,
        SchedulerEffectDispatch::ConsultMetis => executor.consult_metis().await,
        SchedulerEffectDispatch::AskHandoffChoice => executor.ask_handoff_choice().await,
        SchedulerEffectDispatch::RunMomusReviewLoop => executor.run_momus_review_loop().await,
        SchedulerEffectDispatch::DeleteDraftArtifact => executor.delete_draft_artifact().await,
        SchedulerEffectDispatch::AppendStartWorkGuidance(decoration) => {
            executor.decorate_handoff_output(decoration).await
        }
        SchedulerEffectDispatch::Skip => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerExecutionWorkflowKind {
    Direct,
    SinglePass,
    CoordinationLoop,
    AutonomousLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerExecutionChildMode {
    Parallel,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerExecutionVerificationMode {
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerExecutionWorkflowPolicy {
    pub kind: SchedulerExecutionWorkflowKind,
    pub child_mode: SchedulerExecutionChildMode,
    pub allow_execution_fallback: bool,
    pub verification_mode: SchedulerExecutionVerificationMode,
    pub max_rounds: u32,
}

impl SchedulerExecutionWorkflowPolicy {
    pub const fn direct() -> Self {
        Self {
            kind: SchedulerExecutionWorkflowKind::Direct,
            child_mode: SchedulerExecutionChildMode::Parallel,
            allow_execution_fallback: true,
            verification_mode: SchedulerExecutionVerificationMode::Optional,
            max_rounds: 1,
        }
    }

    pub const fn single_pass() -> Self {
        Self {
            kind: SchedulerExecutionWorkflowKind::SinglePass,
            child_mode: SchedulerExecutionChildMode::Sequential,
            allow_execution_fallback: false,
            verification_mode: SchedulerExecutionVerificationMode::Optional,
            max_rounds: 1,
        }
    }

    pub const fn coordination_loop(
        child_mode: SchedulerExecutionChildMode,
        allow_execution_fallback: bool,
        verification_mode: SchedulerExecutionVerificationMode,
        max_rounds: u32,
    ) -> Self {
        Self {
            kind: SchedulerExecutionWorkflowKind::CoordinationLoop,
            child_mode,
            allow_execution_fallback,
            verification_mode,
            max_rounds,
        }
    }

    pub const fn autonomous_loop(
        child_mode: SchedulerExecutionChildMode,
        allow_execution_fallback: bool,
        verification_mode: SchedulerExecutionVerificationMode,
        max_rounds: u32,
    ) -> Self {
        Self {
            kind: SchedulerExecutionWorkflowKind::AutonomousLoop,
            child_mode,
            allow_execution_fallback,
            verification_mode,
            max_rounds,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerFinalizationMode {
    StandardSynthesis,
    PlannerHandoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerFlowDefinition {
    pub stage_graph: SchedulerStageGraph,
    pub transition_graph: SchedulerTransitionGraph,
    pub effect_protocol: SchedulerEffectProtocol,
    pub execution_workflow_policy: SchedulerExecutionWorkflowPolicy,
    pub finalization_mode: SchedulerFinalizationMode,
}
