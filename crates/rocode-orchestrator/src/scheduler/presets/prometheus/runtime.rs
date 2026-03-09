use crate::scheduler::SchedulerMetisConsultInput;
use serde_json::{json, Value};

use super::super::super::{
    SchedulerEffectContext, SchedulerEffectDispatch, SchedulerEffectKind, SchedulerEffectMoment,
    SchedulerEffectProtocol, SchedulerEffectSpec, SchedulerHandoffDecoration,
    SchedulerPresetRuntimeFields, SchedulerStageKind, SchedulerTransitionGraph,
    SchedulerTransitionSpec, SchedulerTransitionTarget, SchedulerTransitionTrigger,
};
use super::{append_handoff_guidance, normalize_prometheus_review_output, PrometheusReviewContext};

pub const PROMETHEUS_MAX_MOMUS_ROUNDS: usize = usize::MAX;
pub const PROMETHEUS_DEFAULT_HANDOFF_CHOICE: &str = "Start Work";
pub const PROMETHEUS_HIGH_ACCURACY_CHOICE: &str = "High Accuracy Review";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrometheusReviewStateSnapshot<'a> {
    pub route_rationale_summary: Option<&'a str>,
    pub planning_artifact_path: Option<&'a str>,
    pub interviewed: Option<&'a str>,
    pub planned: Option<&'a str>,
    pub draft_snapshot: Option<&'a str>,
    pub metis_review: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrometheusTransitionContext<'a> {
    pub handoff_choice: Option<&'a str>,
    pub high_accuracy_approved: Option<bool>,
}

pub fn prometheus_review_state_snapshot(
    runtime: SchedulerPresetRuntimeFields<'_>,
) -> PrometheusReviewStateSnapshot<'_> {
    PrometheusReviewStateSnapshot {
        route_rationale_summary: runtime.route_rationale_summary,
        planning_artifact_path: runtime.planning_artifact_path,
        interviewed: runtime.interviewed,
        planned: runtime.planned,
        draft_snapshot: runtime.draft_snapshot,
        metis_review: runtime.metis_review,
    }
}

pub fn compose_prometheus_metis_input(input: SchedulerMetisConsultInput<'_>) -> String {
    let mut sections = Vec::new();
    sections.push("Review this planning session before I generate the work plan:".to_string());
    sections.push(format!("**User's Goal**: {}", input.goal));
    sections.push(format!(
        "**Original Request**:
{}",
        input.original_request
    ));

    let discussed = input
        .discussed
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("No interview summary captured yet.");
    sections.push(format!(
        "**What We Discussed**:
{discussed}"
    ));

    let understanding = input
        .draft_context
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Use the current Prometheus draft as the working understanding.");
    sections.push(format!(
        "**My Understanding**:
{understanding}"
    ));

    let research = input
        .research
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("No extra repo research findings were captured yet.");
    sections.push(format!(
        "**Research Findings**:
{research}"
    ));

    sections.push(
        "Please identify:
1. Questions I should have asked but didn't
2. Guardrails that need to be explicitly set
3. Potential scope creep areas to lock down
4. Assumptions I'm making that need validation
5. Missing acceptance criteria
6. Edge cases not addressed

Return actionable guidance for Prometheus only. Focus on planning quality, not implementation."
            .to_string(),
    );
    sections.join(
        "

",
    )
}

pub fn handoff_choice_payload() -> Value {
    json!({
        "questions": [{
            "header": "Next Step",
            "question": "Plan is ready. How would you like to proceed?",
            "options": [
                { "label": "Start Work", "description": "Use the reviewed plan and begin execution with /start-work." },
                { "label": "High Accuracy Review", "description": "Run Momus review before execution." }
            ]
        }]
    })
}

pub fn parse_handoff_choice(output: &str) -> String {
    serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|value| value.get("answers").and_then(|v| v.as_array()).cloned())
        .and_then(|answers| answers.first().cloned())
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| PROMETHEUS_DEFAULT_HANDOFF_CHOICE.to_string())
}

pub fn should_run_high_accuracy_review(choice: &str) -> bool {
    choice.eq_ignore_ascii_case(PROMETHEUS_HIGH_ACCURACY_CHOICE)
}

pub fn recommend_start_work(high_accuracy_approved: Option<bool>) -> bool {
    !matches!(high_accuracy_approved, Some(false))
}

pub fn prometheus_transition_graph(
    mut transitions: Vec<SchedulerTransitionSpec>,
) -> SchedulerTransitionGraph {
    transitions.retain(|transition| transition.from != SchedulerStageKind::Handoff);
    transitions.push(SchedulerTransitionSpec {
        from: SchedulerStageKind::Handoff,
        trigger: SchedulerTransitionTrigger::OnUserChoice(PROMETHEUS_DEFAULT_HANDOFF_CHOICE),
        to: SchedulerTransitionTarget::Finish,
    });
    transitions.push(SchedulerTransitionSpec {
        from: SchedulerStageKind::Handoff,
        trigger: SchedulerTransitionTrigger::OnUserChoice(PROMETHEUS_HIGH_ACCURACY_CHOICE),
        to: SchedulerTransitionTarget::Stage(SchedulerStageKind::Plan),
    });
    transitions.push(SchedulerTransitionSpec {
        from: SchedulerStageKind::Handoff,
        trigger: SchedulerTransitionTrigger::OnHighAccuracyApproved,
        to: SchedulerTransitionTarget::Finish,
    });
    transitions.push(SchedulerTransitionSpec {
        from: SchedulerStageKind::Handoff,
        trigger: SchedulerTransitionTrigger::OnHighAccuracyBlocked,
        to: SchedulerTransitionTarget::Stage(SchedulerStageKind::Plan),
    });
    SchedulerTransitionGraph::new(transitions)
}

pub fn prometheus_effect_protocol(stages: &[SchedulerStageKind]) -> SchedulerEffectProtocol {
    let mut effects = Vec::new();

    if stages.contains(&SchedulerStageKind::Interview) {
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Interview,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::SyncDraftArtifact,
        });
    }

    if stages.contains(&SchedulerStageKind::Plan) {
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::EnsurePlanningArtifactPath,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::RegisterWorkflowTodos,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::ConsultMetis,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::SyncDraftArtifact,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::PersistPlanningArtifact,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Plan,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::SyncDraftArtifact,
        });
    }

    if stages.contains(&SchedulerStageKind::Handoff) {
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::OnEnter,
            effect: SchedulerEffectKind::AskHandoffChoice,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::BeforeTransition,
            effect: SchedulerEffectKind::RunMomusReviewLoop,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::DeleteDraftArtifact,
        });
        effects.push(SchedulerEffectSpec {
            stage: SchedulerStageKind::Handoff,
            moment: SchedulerEffectMoment::OnSuccess,
            effect: SchedulerEffectKind::AppendStartWorkGuidance,
        });
    }

    SchedulerEffectProtocol::new(effects)
}

pub fn resolve_prometheus_transition_target(
    transitions: &[&SchedulerTransitionSpec],
    context: PrometheusTransitionContext<'_>,
) -> Option<SchedulerTransitionTarget> {
    if context.high_accuracy_approved == Some(false) {
        if let Some(target) = find_prometheus_transition_target(
            transitions,
            SchedulerTransitionTrigger::OnHighAccuracyBlocked,
        ) {
            return Some(target);
        }
    }

    if context.high_accuracy_approved == Some(true) {
        if let Some(target) = find_prometheus_transition_target(
            transitions,
            SchedulerTransitionTrigger::OnHighAccuracyApproved,
        ) {
            return Some(target);
        }
    }

    if let Some(choice) = context.handoff_choice {
        if let Some(transition) = transitions
            .iter()
            .find(|transition| match transition.trigger {
                SchedulerTransitionTrigger::OnUserChoice(expected) => {
                    expected.eq_ignore_ascii_case(choice)
                }
                _ => false,
            })
        {
            return Some(transition.to);
        }
    }

    find_prometheus_transition_target(transitions, SchedulerTransitionTrigger::OnSuccess)
}

fn find_prometheus_transition_target(
    transitions: &[&SchedulerTransitionSpec],
    trigger: SchedulerTransitionTrigger,
) -> Option<SchedulerTransitionTarget> {
    transitions
        .iter()
        .find(|transition| transition.trigger == trigger)
        .map(|transition| transition.to)
}

pub fn resolve_prometheus_effect_dispatch(
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
        SchedulerEffectKind::RunMomusReviewLoop => {
            if context
                .user_choice
                .as_deref()
                .map(should_run_high_accuracy_review)
                .unwrap_or(false)
            {
                SchedulerEffectDispatch::RunMomusReviewLoop
            } else {
                SchedulerEffectDispatch::Skip
            }
        }
        SchedulerEffectKind::DeleteDraftArtifact => {
            if resolve_prometheus_handoff_decoration(context).recommend_start_work {
                SchedulerEffectDispatch::DeleteDraftArtifact
            } else {
                SchedulerEffectDispatch::Skip
            }
        }
        SchedulerEffectKind::AppendStartWorkGuidance => {
            SchedulerEffectDispatch::AppendStartWorkGuidance(resolve_prometheus_handoff_decoration(
                context,
            ))
        }
    }
}

pub fn resolve_prometheus_handoff_decoration(
    context: SchedulerEffectContext,
) -> SchedulerHandoffDecoration {
    let recommend_start_work = recommend_start_work(context.high_accuracy_approved);
    SchedulerHandoffDecoration {
        plan_path: context.planning_artifact_path,
        draft_path: context.draft_artifact_path,
        draft_deleted: recommend_start_work && !context.draft_exists,
        recommend_start_work,
        high_accuracy_approved: context.high_accuracy_approved,
    }
}

pub fn decorate_prometheus_handoff_output(
    content: String,
    decoration: SchedulerHandoffDecoration,
) -> String {
    append_handoff_guidance(
        content,
        decoration.plan_path.as_deref(),
        decoration.draft_path.as_deref(),
        decoration.draft_deleted,
        decoration.recommend_start_work,
        decoration.high_accuracy_approved,
    )
}

pub fn normalize_prometheus_review_stage_output(
    snapshot: PrometheusReviewStateSnapshot<'_>,
    review_output: &str,
) -> String {
    normalize_prometheus_review_output(
        PrometheusReviewContext {
            route_rationale_summary: snapshot.route_rationale_summary,
            planning_artifact_path: snapshot.planning_artifact_path,
            interviewed: snapshot.interviewed,
            planned: snapshot.planned,
            draft_snapshot: snapshot.draft_snapshot,
            metis_review: snapshot.metis_review,
        },
        review_output,
    )
}
