use serde_json::json;

pub fn hephaestus_workflow_todos_payload() -> serde_json::Value {
    json!({
        "todos": [
            { "id": "hephaestus-1", "content": "Run the autonomous execution loop to completion", "status": "pending", "priority": "high" },
            { "id": "hephaestus-2", "content": "Verify the deep worker result before finish gate", "status": "pending", "priority": "high" },
            { "id": "hephaestus-3", "content": "Return the finalized executor result", "status": "pending", "priority": "medium" }
        ]
    })
}

mod input;
mod output;
mod prompt;

pub use input::*;
pub use output::*;
pub use prompt::*;

use super::super::{
    SchedulerPresetKind, SchedulerPresetMetadata, SchedulerProfileConfig,
    SchedulerProfileOrchestrator, SchedulerProfilePlan, SchedulerStageKind,
};
use super::{orchestrator_from_definition, plan_from_definition, SchedulerPresetDefinition};
use crate::tool_runner::ToolRunner;

const HEPHAESTUS_DEFAULT_STAGES: &[SchedulerStageKind] = &[
    SchedulerStageKind::RequestAnalysis,
    SchedulerStageKind::ExecutionOrchestration,
];

pub const HEPHAESTUS_PRESET: SchedulerPresetDefinition = SchedulerPresetDefinition {
    kind: SchedulerPresetKind::Hephaestus,
    metadata: SchedulerPresetMetadata {
        public: true,
        router_recommended: true,
        deprecated: false,
    },
    default_stages: HEPHAESTUS_DEFAULT_STAGES,
};

/// OMO Hephaestus-aligned orchestration: autonomous deep worker.
///
/// Hephaestus keeps the shared autonomous-workflow low-overhead topology, but the scheduler
/// now treats autonomous fallback execution as a first-class path and still
/// requires verification before the finish gate can settle the result.
pub fn hephaestus_default_stages() -> Vec<SchedulerStageKind> {
    HEPHAESTUS_PRESET.default_stage_kinds()
}

pub type HephaestusPlan = SchedulerProfilePlan;
pub type HephaestusOrchestrator = SchedulerProfileOrchestrator;

pub fn hephaestus_plan() -> HephaestusPlan {
    SchedulerProfilePlan::new(hephaestus_default_stages()).with_orchestrator("hephaestus")
}

pub fn hephaestus_plan_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
) -> HephaestusPlan {
    plan_from_definition(profile_name, profile, HEPHAESTUS_PRESET)
}

pub fn hephaestus_orchestrator_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
    tool_runner: ToolRunner,
) -> HephaestusOrchestrator {
    orchestrator_from_definition(profile_name, profile, tool_runner, HEPHAESTUS_PRESET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SchedulerEffectContext, SchedulerEffectDispatch, SchedulerEffectKind};

    #[test]
    fn hephaestus_uses_low_overhead_stages() {
        assert_eq!(
            hephaestus_default_stages(),
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::ExecutionOrchestration,
            ]
        );
    }

    #[test]
    fn hephaestus_plan_sets_orchestrator() {
        let plan = hephaestus_plan();
        assert_eq!(plan.orchestrator.as_deref(), Some("hephaestus"));
    }

    #[test]
    fn hephaestus_effect_protocol_registers_workflow_todos() {
        let effects = hephaestus_plan().effect_protocol();
        assert!(effects.effects.iter().any(|effect| {
            effect.stage == SchedulerStageKind::ExecutionOrchestration
                && effect.moment == crate::SchedulerEffectMoment::OnEnter
                && effect.effect == SchedulerEffectKind::RegisterWorkflowTodos
        }));
    }

    #[test]
    fn hephaestus_uses_shared_effect_dispatch_framework() {
        let dispatch = hephaestus_plan().effect_dispatch(
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
}
