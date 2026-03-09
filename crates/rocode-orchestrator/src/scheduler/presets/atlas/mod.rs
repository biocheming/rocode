use serde_json::json;

pub fn atlas_workflow_todos_payload() -> serde_json::Value {
    json!({
        "todos": [
            { "id": "atlas-1", "content": "Coordinate parallel execution across the selected workers", "status": "pending", "priority": "high" },
            { "id": "atlas-2", "content": "Run verification and settle the coordination gate", "status": "pending", "priority": "high" },
            { "id": "atlas-3", "content": "Synthesize the verified coordinator result", "status": "pending", "priority": "medium" }
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

const ATLAS_DEFAULT_STAGES: &[SchedulerStageKind] = &[
    SchedulerStageKind::RequestAnalysis,
    SchedulerStageKind::ExecutionOrchestration,
    SchedulerStageKind::Synthesis,
];

pub const ATLAS_PRESET: SchedulerPresetDefinition = SchedulerPresetDefinition {
    kind: SchedulerPresetKind::Atlas,
    metadata: SchedulerPresetMetadata {
        public: true,
        router_recommended: true,
        deprecated: false,
    },
    default_stages: ATLAS_DEFAULT_STAGES,
};

/// OMO Atlas-aligned orchestration: todo-list-driven parallel coordination.
///
/// Atlas keeps the shared coordination-workflow stage topology, but its runtime semantics are
/// stricter: coordination results must be verified before the gate can declare
/// completion, and verification can fall back to the scheduler review layer
/// when no verification graph is configured.
pub fn atlas_default_stages() -> Vec<SchedulerStageKind> {
    ATLAS_PRESET.default_stage_kinds()
}

pub type AtlasPlan = SchedulerProfilePlan;
pub type AtlasOrchestrator = SchedulerProfileOrchestrator;

pub fn atlas_plan() -> AtlasPlan {
    SchedulerProfilePlan::new(atlas_default_stages()).with_orchestrator("atlas")
}

pub fn atlas_plan_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
) -> AtlasPlan {
    plan_from_definition(profile_name, profile, ATLAS_PRESET)
}

pub fn atlas_orchestrator_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
    tool_runner: ToolRunner,
) -> AtlasOrchestrator {
    orchestrator_from_definition(profile_name, profile, tool_runner, ATLAS_PRESET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SchedulerEffectContext, SchedulerEffectDispatch, SchedulerEffectKind};

    #[test]
    fn atlas_uses_coordination_stages() {
        assert_eq!(
            atlas_default_stages(),
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::ExecutionOrchestration,
                SchedulerStageKind::Synthesis,
            ]
        );
    }

    #[test]
    fn atlas_plan_sets_orchestrator() {
        let plan = atlas_plan();
        assert_eq!(plan.orchestrator.as_deref(), Some("atlas"));
    }

    #[test]
    fn atlas_effect_protocol_registers_workflow_todos_for_execution_and_synthesis() {
        let effects = atlas_plan().effect_protocol();
        assert!(effects.effects.iter().any(|effect| {
            effect.stage == SchedulerStageKind::ExecutionOrchestration
                && effect.moment == crate::SchedulerEffectMoment::OnEnter
                && effect.effect == SchedulerEffectKind::RegisterWorkflowTodos
        }));
        assert!(effects.effects.iter().any(|effect| {
            effect.stage == SchedulerStageKind::Synthesis
                && effect.moment == crate::SchedulerEffectMoment::OnEnter
                && effect.effect == SchedulerEffectKind::RegisterWorkflowTodos
        }));
    }

    #[test]
    fn atlas_uses_shared_effect_dispatch_framework() {
        let dispatch = atlas_plan().effect_dispatch(
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
}
