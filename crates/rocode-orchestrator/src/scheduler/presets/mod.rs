mod atlas;
mod hephaestus;
mod prometheus;
mod sisyphus;

pub use atlas::*;
pub use hephaestus::*;
pub use prometheus::*;
pub use sisyphus::*;

use super::super::tool_runner::ToolRunner;
use super::{
    SchedulerAutonomousGateStageInput, SchedulerAutonomousVerificationStageInput,
    SchedulerCoordinationGateStageInput, SchedulerCoordinationVerificationStageInput,
    SchedulerDraftArtifactInput, SchedulerEffectContext, SchedulerEffectDispatch,
    SchedulerEffectKind, SchedulerEffectMoment, SchedulerEffectProtocol, SchedulerEffectSpec,
    SchedulerExecutionChildMode, SchedulerExecutionOrchestrationStageInput,
    SchedulerExecutionVerificationMode, SchedulerExecutionWorkflowPolicy,
    SchedulerFinalizationMode, SchedulerFlowDefinition, SchedulerHandoffDecoration,
    SchedulerHandoffStageInput, SchedulerInterviewStageInput, SchedulerLoopBudget,
    SchedulerMetisConsultInput, SchedulerPlanStageInput, SchedulerPlanningArtifactInput,
    SchedulerPresetKind, SchedulerPresetMetadata, SchedulerPresetRuntimeFields,
    SchedulerPresetRuntimeUpdate, SchedulerProfileConfig, SchedulerProfileOrchestrator,
    SchedulerProfilePlan, SchedulerReviewStageInput, SchedulerSessionProjection,
    SchedulerStageGraph, SchedulerStageKind, SchedulerStagePolicy, SchedulerStageSpec,
    SchedulerSynthesisStageInput, SchedulerTransitionGraph, SchedulerTransitionSpec,
    SchedulerTransitionTarget, SchedulerTransitionTrigger, StageToolPolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerPresetDefinition {
    pub kind: SchedulerPresetKind,
    pub metadata: SchedulerPresetMetadata,
    pub default_stages: &'static [SchedulerStageKind],
}

impl SchedulerPresetDefinition {
    pub fn default_stage_kinds(self) -> Vec<SchedulerStageKind> {
        self.default_stages.to_vec()
    }

    pub fn post_route_stage_kinds(self) -> Vec<SchedulerStageKind> {
        self.default_stages
            .iter()
            .copied()
            .filter(|stage| {
                !matches!(
                    stage,
                    SchedulerStageKind::RequestAnalysis | SchedulerStageKind::Route
                )
            })
            .collect()
    }

    pub fn stage_policy(self, stage: SchedulerStageKind) -> SchedulerStagePolicy {
        let session_projection = match stage {
            SchedulerStageKind::RequestAnalysis => SchedulerSessionProjection::Hidden,
            _ => SchedulerSessionProjection::Transcript,
        };

        let (tool_policy, loop_budget) = match stage {
            SchedulerStageKind::RequestAnalysis => (
                StageToolPolicy::DisableAll,
                SchedulerLoopBudget::StepLimit(1),
            ),
            SchedulerStageKind::Route => (
                StageToolPolicy::AllowReadOnly,
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::Interview => (
                match self.kind {
                    SchedulerPresetKind::Prometheus => StageToolPolicy::PrometheusPlanning,
                    _ => StageToolPolicy::AllowReadOnly,
                },
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::Plan => (
                match self.kind {
                    SchedulerPresetKind::Prometheus => StageToolPolicy::PrometheusPlanning,
                    _ => StageToolPolicy::AllowReadOnly,
                },
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::Delegation => (
                StageToolPolicy::AllowAll,
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::Review => (
                match self.kind {
                    SchedulerPresetKind::Prometheus => StageToolPolicy::PrometheusPlanning,
                    _ => StageToolPolicy::AllowReadOnly,
                },
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::ExecutionOrchestration => {
                (StageToolPolicy::AllowAll, SchedulerLoopBudget::Unbounded)
            }
            SchedulerStageKind::Synthesis => (
                StageToolPolicy::DisableAll,
                SchedulerLoopBudget::Unbounded,
            ),
            SchedulerStageKind::Handoff => (
                match self.kind {
                    SchedulerPresetKind::Prometheus => StageToolPolicy::PrometheusPlanning,
                    _ => StageToolPolicy::DisableAll,
                },
                SchedulerLoopBudget::Unbounded,
            ),
        };

        SchedulerStagePolicy {
            session_projection,
            tool_policy,
            loop_budget,
        }
    }

    pub fn stage_graph(self, stages: &[SchedulerStageKind]) -> SchedulerStageGraph {
        SchedulerStageGraph::new(
            stages
                .iter()
                .copied()
                .map(|kind| SchedulerStageSpec {
                    kind,
                    policy: self.stage_policy(kind),
                })
                .collect(),
        )
    }

    pub fn transition_graph(self, stages: &[SchedulerStageKind]) -> SchedulerTransitionGraph {
        let mut transitions = stages
            .windows(2)
            .map(|window| SchedulerTransitionSpec {
                from: window[0],
                trigger: SchedulerTransitionTrigger::OnSuccess,
                to: SchedulerTransitionTarget::Stage(window[1]),
            })
            .collect::<Vec<_>>();

        if let Some(last) = stages.last().copied() {
            transitions.push(SchedulerTransitionSpec {
                from: last,
                trigger: SchedulerTransitionTrigger::OnSuccess,
                to: SchedulerTransitionTarget::Finish,
            });
        }

        match self.kind {
            SchedulerPresetKind::Prometheus => prometheus_transition_graph(transitions),
            _ => SchedulerTransitionGraph::new(transitions),
        }
    }

    pub fn effect_protocol(self, stages: &[SchedulerStageKind]) -> SchedulerEffectProtocol {
        match self.kind {
            SchedulerPresetKind::Prometheus => prometheus_effect_protocol(stages),
            SchedulerPresetKind::Sisyphus => shared_execution_workflow_effect_protocol(
                stages,
                &[SchedulerStageKind::ExecutionOrchestration],
            ),
            SchedulerPresetKind::Atlas => shared_execution_workflow_effect_protocol(
                stages,
                &[
                    SchedulerStageKind::ExecutionOrchestration,
                    SchedulerStageKind::Synthesis,
                ],
            ),
            SchedulerPresetKind::Hephaestus => shared_execution_workflow_effect_protocol(
                stages,
                &[SchedulerStageKind::ExecutionOrchestration],
            ),
        }
    }

    pub fn resolve_effect_dispatch(
        self,
        effect: SchedulerEffectKind,
        context: SchedulerEffectContext,
    ) -> SchedulerEffectDispatch {
        match self.kind {
            SchedulerPresetKind::Prometheus => resolve_prometheus_effect_dispatch(effect, context),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => default_effect_dispatch(effect, context),
        }
    }

    pub fn effect_dispatch_is_authoritative(self) -> bool {
        matches!(self.kind, SchedulerPresetKind::Prometheus)
    }

    pub fn route_constraint_note(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(
                "## Route Constraint
This session is already running under the explicit Prometheus planner profile. You may decide direct vs orchestrate, but when orchestrating you must preserve the Prometheus workflow. Do not reroute this session to Sisyphus, Atlas, Hephaestus, or any other preset.",
            ),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn constrain_route_decision(
        self,
        decision: crate::scheduler::RouteDecision,
    ) -> crate::scheduler::RouteDecision {
        match self.kind {
            SchedulerPresetKind::Prometheus
                if matches!(decision.mode, crate::scheduler::RouteMode::Orchestrate) =>
            {
                let mut constrained = decision;
                constrained.preset = Some("prometheus".to_string());
                if constrained.review_mode == Some(crate::scheduler::ReviewMode::Skip) {
                    constrained.review_mode = Some(crate::scheduler::ReviewMode::Normal);
                }
                constrained
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => decision,
        }
    }

    pub fn normalize_review_stage_output(
        self,
        runtime: SchedulerPresetRuntimeFields<'_>,
        output: &str,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(normalize_prometheus_review_stage_output(
                prometheus_review_state_snapshot(runtime),
                output,
            )),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn normalize_final_output(self, output: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Sisyphus => Some(normalize_sisyphus_final_output(output)),
            SchedulerPresetKind::Atlas => Some(normalize_atlas_final_output(output)),
            SchedulerPresetKind::Hephaestus => Some(normalize_hephaestus_final_output(output)),
            SchedulerPresetKind::Prometheus => None,
        }
    }

    pub fn runtime_update_for_metis_review(
        self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        match self.kind {
            SchedulerPresetKind::Prometheus => {
                Some(SchedulerPresetRuntimeUpdate::MetisReview(content))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn runtime_update_for_handoff_choice(
        self,
        choice: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        match self.kind {
            SchedulerPresetKind::Prometheus => {
                Some(SchedulerPresetRuntimeUpdate::HandoffChoice(choice))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn runtime_update_for_momus_review(
        self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        match self.kind {
            SchedulerPresetKind::Prometheus => {
                Some(SchedulerPresetRuntimeUpdate::MomusReview(content))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn runtime_update_for_planned_output(
        self,
        content: String,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(SchedulerPresetRuntimeUpdate::Planned(content)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn runtime_update_for_high_accuracy(
        self,
        approved: bool,
    ) -> Option<SchedulerPresetRuntimeUpdate> {
        match self.kind {
            SchedulerPresetKind::Prometheus => {
                Some(SchedulerPresetRuntimeUpdate::HighAccuracyApproved(approved))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn execution_workflow_policy(self) -> SchedulerExecutionWorkflowPolicy {
        match self.kind {
            SchedulerPresetKind::Prometheus => SchedulerExecutionWorkflowPolicy::direct(),
            SchedulerPresetKind::Sisyphus => SchedulerExecutionWorkflowPolicy::single_pass(),
            SchedulerPresetKind::Atlas => SchedulerExecutionWorkflowPolicy::coordination_loop(
                SchedulerExecutionChildMode::Parallel,
                true,
                SchedulerExecutionVerificationMode::Required,
                3,
            ),
            SchedulerPresetKind::Hephaestus => SchedulerExecutionWorkflowPolicy::autonomous_loop(
                SchedulerExecutionChildMode::Sequential,
                true,
                SchedulerExecutionVerificationMode::Required,
                3,
            ),
        }
    }

    pub fn flow_definition(self, stages: &[SchedulerStageKind]) -> SchedulerFlowDefinition {
        SchedulerFlowDefinition {
            stage_graph: self.stage_graph(stages),
            transition_graph: self.transition_graph(stages),
            effect_protocol: self.effect_protocol(stages),
            execution_workflow_policy: self.execution_workflow_policy(),
            finalization_mode: self.finalization_mode(),
        }
    }

    pub fn finalization_mode(self) -> SchedulerFinalizationMode {
        match self.kind {
            SchedulerPresetKind::Prometheus => SchedulerFinalizationMode::PlannerHandoff,
            _ => SchedulerFinalizationMode::StandardSynthesis,
        }
    }

    pub fn delegation_charter(self, plan: &SchedulerProfilePlan) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Sisyphus => Some(build_sisyphus_dynamic_prompt(
                &plan.available_agents,
                &plan.available_categories,
                &plan.skill_list,
            )),
            SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn execution_orchestration_charter(
        self,
        plan: &SchedulerProfilePlan,
        profile_suffix: &str,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Sisyphus => Some(build_sisyphus_dynamic_prompt(
                &plan.available_agents,
                &plan.available_categories,
                &plan.skill_list,
            )),
            SchedulerPresetKind::Atlas => Some(format!(
                "{}{}",
                build_atlas_dynamic_prompt(
                    &plan.available_agents,
                    &plan.available_categories,
                    &plan.skill_list,
                ),
                profile_suffix,
            )),
            SchedulerPresetKind::Hephaestus => Some(format!(
                "{}{}",
                build_hephaestus_dynamic_prompt(
                    &plan.available_agents,
                    &plan.available_categories,
                    &plan.skill_list,
                ),
                profile_suffix,
            )),
            SchedulerPresetKind::Prometheus => None,
        }
    }

    pub fn review_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(prometheus_review_prompt(profile_suffix)),
            SchedulerPresetKind::Sisyphus | SchedulerPresetKind::Atlas => Some(format!(
                "You are the scheduler review layer for {} mode.                  Audit the delegated result against the original request, tighten weak claims,                  and keep the answer faithful to evidence.                  Use read-only tools only when they materially improve verification.{}",
                self.kind.as_str(),
                profile_suffix,
            )),
            SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn interview_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(prometheus_interview_prompt(profile_suffix)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn plan_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(prometheus_plan_prompt(profile_suffix)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn handoff_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(prometheus_handoff_prompt(profile_suffix)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_interview_input(
        self,
        input: SchedulerInterviewStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_interview_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_plan_input(self, input: SchedulerPlanStageInput<'_>) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_plan_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_execution_orchestration_input(
        self,
        input: SchedulerExecutionOrchestrationStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Sisyphus => {
                Some(compose_sisyphus_execution_orchestration_input(input))
            }
            SchedulerPresetKind::Atlas => Some(compose_atlas_execution_orchestration_input(input)),
            SchedulerPresetKind::Hephaestus => {
                Some(compose_hephaestus_execution_orchestration_input(input))
            }
            SchedulerPresetKind::Prometheus => None,
        }
    }

    pub fn compose_synthesis_input(
        self,
        input: SchedulerSynthesisStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(compose_atlas_synthesis_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_coordination_verification_input(
        self,
        input: SchedulerCoordinationVerificationStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Atlas => {
                Some(compose_atlas_coordination_verification_input(input))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_coordination_gate_input(
        self,
        input: SchedulerCoordinationGateStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(compose_atlas_coordination_gate_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_autonomous_verification_input(
        self,
        input: SchedulerAutonomousVerificationStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Hephaestus => {
                Some(compose_hephaestus_autonomous_verification_input(input))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas => None,
        }
    }

    pub fn compose_autonomous_gate_input(
        self,
        input: SchedulerAutonomousGateStageInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Hephaestus => {
                Some(compose_hephaestus_autonomous_gate_input(input))
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas => None,
        }
    }

    pub fn compose_review_input(self, input: SchedulerReviewStageInput<'_>) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_review_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_handoff_input(self, input: SchedulerHandoffStageInput<'_>) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_handoff_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_metis_input(self, input: SchedulerMetisConsultInput<'_>) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_metis_input(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn metis_agent_name(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some("metis"),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn handoff_choice_payload(self) -> Option<serde_json::Value> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(handoff_choice_payload()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn parse_handoff_choice(self, output: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(parse_handoff_choice(output)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn default_handoff_choice(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(PROMETHEUS_DEFAULT_HANDOFF_CHOICE),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn momus_agent_name(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some("momus"),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn max_momus_rounds(self) -> Option<usize> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(PROMETHEUS_MAX_MOMUS_ROUNDS),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn momus_output_is_okay(self, output: &str) -> bool {
        match self.kind {
            SchedulerPresetKind::Prometheus => momus_output_is_okay(output),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => false,
        }
    }

    pub fn delegation_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Sisyphus => Some(format!(
                "You are Sisyphus's delegation executor. \
                 Prefer delegating non-trivial work through ROCode's task tools, \
                 but finish genuinely trivial work directly. \
                 Return only concrete execution results, not a fresh plan.{}",
                profile_suffix
            )),
            SchedulerPresetKind::Prometheus => Some(format!(
                "You are Prometheus's execution coordinator. \
                 Follow the approved plan precisely, execute with ROCode tools, \
                 and use task/task_flow when delegation materially improves the outcome. \
                 Return concrete execution results, not planning prose.{}",
                profile_suffix
            )),
            SchedulerPresetKind::Atlas | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn execution_fallback_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(format!(
                "You are Atlas's execution fallback. \
                 Work from a task-list mindset: decompose, track, verify, and only claim completion with evidence. \
                 Use task/task_flow when helpful and preserve per-task status in your output.{}",
                profile_suffix
            )),
            SchedulerPresetKind::Hephaestus => Some(format!(
                "You are Hephaestus's autonomous execution layer. \
                 Run the full explore -> plan -> decide -> execute -> verify loop yourself. \
                 Do not stop at partial progress when further verified action is possible.{}",
                profile_suffix
            )),
            SchedulerPresetKind::Sisyphus | SchedulerPresetKind::Prometheus => None,
        }
    }

    pub fn synthesis_stage_prompt(self, profile_suffix: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(atlas_synthesis_prompt(profile_suffix)),
            SchedulerPresetKind::Sisyphus | SchedulerPresetKind::Prometheus => Some(format!(
                "You are the final synthesis layer for ROCode's scheduler ({} mode, OMO-aligned).                  Merge prior stage outputs into a single final response for the user.                  Keep the answer faithful to actual stage results.                  Do not invent edits, tool calls, or conclusions.                  If there are remaining risks or follow-ups, state them clearly.{}",
                self.kind.as_str(),
                profile_suffix
            )),
            SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn coordination_verification_charter(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(atlas_verification_charter()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn coordination_gate_contract(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(atlas_gate_contract()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn coordination_gate_prompt(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Atlas => Some(atlas_gate_prompt()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn autonomous_verification_charter(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Hephaestus => Some(hephaestus_verification_charter()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas => None,
        }
    }

    pub fn autonomous_gate_contract(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Hephaestus => Some(hephaestus_gate_contract()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas => None,
        }
    }

    pub fn autonomous_gate_prompt(self) -> Option<&'static str> {
        match self.kind {
            SchedulerPresetKind::Hephaestus => Some(hephaestus_gate_prompt()),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Prometheus
            | SchedulerPresetKind::Atlas => None,
        }
    }

    pub fn workflow_todos_payload(self) -> serde_json::Value {
        match self.kind {
            SchedulerPresetKind::Prometheus => prometheus_workflow_todos_payload(),
            SchedulerPresetKind::Sisyphus => sisyphus_workflow_todos_payload(),
            SchedulerPresetKind::Atlas => atlas_workflow_todos_payload(),
            SchedulerPresetKind::Hephaestus => hephaestus_workflow_todos_payload(),
        }
    }

    pub fn planning_artifact_relative_path(self, session_id: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(build_prometheus_artifact_relative_path(
                PrometheusArtifactKind::Planning,
                session_id,
            )),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn draft_artifact_relative_path(self, session_id: &str) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(build_prometheus_artifact_relative_path(
                PrometheusArtifactKind::Draft,
                session_id,
            )),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_draft_artifact(self, input: SchedulerDraftArtifactInput<'_>) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_draft_artifact(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn compose_planning_artifact(
        self,
        input: SchedulerPlanningArtifactInput<'_>,
    ) -> Option<String> {
        match self.kind {
            SchedulerPresetKind::Prometheus => Some(compose_prometheus_planning_artifact(input)),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn decorate_handoff_output(
        self,
        content: String,
        decoration: SchedulerHandoffDecoration,
    ) -> String {
        match self.kind {
            SchedulerPresetKind::Prometheus => {
                decorate_prometheus_handoff_output(content, decoration)
            }
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => content,
        }
    }

    pub fn resolve_runtime_transition_target(
        self,
        transitions: &[&SchedulerTransitionSpec],
        handoff_choice: Option<&str>,
        high_accuracy_approved: Option<bool>,
    ) -> Option<SchedulerTransitionTarget> {
        match self.kind {
            SchedulerPresetKind::Prometheus => resolve_prometheus_transition_target(
                transitions,
                crate::scheduler::presets::prometheus::PrometheusTransitionContext {
                    handoff_choice,
                    high_accuracy_approved,
                },
            ),
            SchedulerPresetKind::Sisyphus
            | SchedulerPresetKind::Atlas
            | SchedulerPresetKind::Hephaestus => None,
        }
    }

    pub fn system_prompt_preview(self) -> &'static str {
        match self.kind {
            SchedulerPresetKind::Sisyphus => sisyphus_system_prompt_preview(),
            SchedulerPresetKind::Prometheus => prometheus_system_prompt_preview(),
            SchedulerPresetKind::Atlas => atlas_system_prompt_preview(),
            SchedulerPresetKind::Hephaestus => hephaestus_system_prompt_preview(),
        }
    }
}

fn shared_execution_workflow_effect_protocol(
    stages: &[SchedulerStageKind],
    workflow_stages: &[SchedulerStageKind],
) -> SchedulerEffectProtocol {
    let effects = workflow_stages
        .iter()
        .copied()
        .filter(|stage| stages.contains(stage))
        .map(|stage| SchedulerEffectSpec {
            stage,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::RegisterWorkflowTodos,
        })
        .collect();
    SchedulerEffectProtocol::new(effects)
}

fn default_effect_dispatch(
    effect: SchedulerEffectKind,
    context: SchedulerEffectContext,
) -> SchedulerEffectDispatch {
    match effect {
        SchedulerEffectKind::EnsurePlanningArtifactPath => {
            SchedulerEffectDispatch::EnsurePlanningArtifactPath
        }
        SchedulerEffectKind::PersistPlanningArtifact => {
            SchedulerEffectDispatch::PersistPlanningArtifact
        }
        SchedulerEffectKind::PersistDraftArtifact | SchedulerEffectKind::SyncDraftArtifact => {
            SchedulerEffectDispatch::SyncDraftArtifact
        }
        SchedulerEffectKind::RegisterWorkflowTodos => {
            SchedulerEffectDispatch::RegisterWorkflowTodos
        }
        SchedulerEffectKind::ConsultMetis => SchedulerEffectDispatch::ConsultMetis,
        SchedulerEffectKind::AskHandoffChoice => SchedulerEffectDispatch::AskHandoffChoice,
        SchedulerEffectKind::RunMomusReviewLoop => SchedulerEffectDispatch::RunMomusReviewLoop,
        SchedulerEffectKind::DeleteDraftArtifact => SchedulerEffectDispatch::DeleteDraftArtifact,
        SchedulerEffectKind::AppendStartWorkGuidance => {
            SchedulerEffectDispatch::AppendStartWorkGuidance(SchedulerHandoffDecoration {
                plan_path: context.planning_artifact_path,
                draft_path: context.draft_artifact_path,
                draft_deleted: !context.draft_exists,
                recommend_start_work: true,
                high_accuracy_approved: context.high_accuracy_approved,
            })
        }
    }
}

pub(super) fn plan_from_definition(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
    definition: SchedulerPresetDefinition,
) -> SchedulerProfilePlan {
    SchedulerProfilePlan::from_profile_config(
        profile_name,
        definition.default_stage_kinds(),
        profile,
    )
}

pub(super) fn orchestrator_from_definition(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
    tool_runner: ToolRunner,
    definition: SchedulerPresetDefinition,
) -> SchedulerProfileOrchestrator {
    SchedulerProfileOrchestrator::new(
        plan_from_definition(profile_name, profile, definition),
        tool_runner,
    )
}

pub fn scheduler_preset_definition(kind: SchedulerPresetKind) -> SchedulerPresetDefinition {
    match kind {
        SchedulerPresetKind::Sisyphus => SISYPHUS_PRESET,
        SchedulerPresetKind::Prometheus => PROMETHEUS_PRESET,
        SchedulerPresetKind::Atlas => ATLAS_PRESET,
        SchedulerPresetKind::Hephaestus => HEPHAESTUS_PRESET,
    }
}
