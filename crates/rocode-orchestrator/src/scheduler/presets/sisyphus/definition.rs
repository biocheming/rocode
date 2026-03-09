use super::super::super::{SchedulerPresetKind, SchedulerPresetMetadata, SchedulerStageKind};
use super::super::SchedulerPresetDefinition;

const SISYPHUS_DEFAULT_STAGES: &[SchedulerStageKind] = &[
    SchedulerStageKind::RequestAnalysis,
    SchedulerStageKind::Route,
    SchedulerStageKind::ExecutionOrchestration,
];

pub const SISYPHUS_PRESET: SchedulerPresetDefinition = SchedulerPresetDefinition {
    kind: SchedulerPresetKind::Sisyphus,
    metadata: SchedulerPresetMetadata {
        public: true,
        router_recommended: true,
        deprecated: false,
    },
    default_stages: SISYPHUS_DEFAULT_STAGES,
};

/// OMO Sisyphus-aligned orchestration: single-loop execution with prompt-driven phase control.
///
/// Stages: RequestAnalysis → Route → ExecutionOrchestration
/// - Route: intent classification + preset switching (ROCode observability gate)
/// - ExecutionOrchestration: single long loop with dynamic prompt (OMO Phase 0-3 self-directed)
///
/// This matches OMO's single-agent model while preserving ROCode's routing architecture.
pub fn sisyphus_default_stages() -> Vec<SchedulerStageKind> {
    SISYPHUS_PRESET.default_stage_kinds()
}
