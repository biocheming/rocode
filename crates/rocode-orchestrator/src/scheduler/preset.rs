use std::path::Path;

use super::{
    scheduler_preset_definition, SchedulerConfig, SchedulerConfigError, SchedulerPresetDefinition,
    SchedulerProfileConfig, SchedulerProfileOrchestrator, SchedulerProfilePlan, SchedulerStageKind,
    SchedulerStageObservability,
};
use crate::skill_tree::SkillTreeRequestPlan;
use crate::tool_runner::ToolRunner;

#[derive(Debug, Clone, Default)]
pub struct SchedulerRequestDefaults {
    pub profile_name: Option<String>,
    pub root_agent_name: Option<String>,
    pub skill_tree_plan: Option<SkillTreeRequestPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerPresetMetadata {
    pub public: bool,
    pub router_recommended: bool,
    pub deprecated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerPresetKind {
    Sisyphus,
    Prometheus,
    Atlas,
    Hephaestus,
}

const ALL_SCHEDULER_PRESETS: [SchedulerPresetKind; 4] = [
    SchedulerPresetKind::Sisyphus,
    SchedulerPresetKind::Prometheus,
    SchedulerPresetKind::Atlas,
    SchedulerPresetKind::Hephaestus,
];

const PUBLIC_SCHEDULER_PRESETS: [SchedulerPresetKind; 4] = [
    SchedulerPresetKind::Sisyphus,
    SchedulerPresetKind::Prometheus,
    SchedulerPresetKind::Atlas,
    SchedulerPresetKind::Hephaestus,
];

const ROUTER_RECOMMENDED_SCHEDULER_PRESETS: [SchedulerPresetKind; 4] = [
    SchedulerPresetKind::Sisyphus,
    SchedulerPresetKind::Prometheus,
    SchedulerPresetKind::Atlas,
    SchedulerPresetKind::Hephaestus,
];

impl SchedulerPresetKind {
    pub fn all() -> &'static [Self] {
        &ALL_SCHEDULER_PRESETS
    }

    pub fn public_presets() -> &'static [Self] {
        &PUBLIC_SCHEDULER_PRESETS
    }

    pub fn router_recommended_presets() -> &'static [Self] {
        &ROUTER_RECOMMENDED_SCHEDULER_PRESETS
    }

    pub fn definition(self) -> SchedulerPresetDefinition {
        scheduler_preset_definition(self)
    }

    pub fn stage_observability(self, stage: SchedulerStageKind) -> SchedulerStageObservability {
        self.definition().stage_observability(stage)
    }

    pub fn metadata(self) -> SchedulerPresetMetadata {
        self.definition().metadata
    }

    pub fn is_public(self) -> bool {
        self.metadata().public
    }

    pub fn is_router_recommended(self) -> bool {
        self.metadata().router_recommended
    }

    pub fn is_deprecated(self) -> bool {
        self.metadata().deprecated
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sisyphus => "sisyphus",
            Self::Prometheus => "prometheus",
            Self::Atlas => "atlas",
            Self::Hephaestus => "hephaestus",
        }
    }

    pub fn from_profile_config(
        profile: &SchedulerProfileConfig,
    ) -> Result<Self, SchedulerConfigError> {
        Self::from_str(profile.orchestrator.as_deref().unwrap_or("sisyphus"))
    }

    pub fn from_str(value: &str) -> Result<Self, SchedulerConfigError> {
        match value {
            "sisyphus" => Ok(Self::Sisyphus),
            "prometheus" => Ok(Self::Prometheus),
            "atlas" => Ok(Self::Atlas),
            "hephaestus" => Ok(Self::Hephaestus),
            other => Err(SchedulerConfigError::UnknownOrchestrator(other.to_string())),
        }
    }

    pub fn plan_from_profile(
        self,
        profile_name: Option<String>,
        profile: &SchedulerProfileConfig,
    ) -> SchedulerProfilePlan {
        let definition = self.definition();
        let mut plan = SchedulerProfilePlan::from_profile_config(
            profile_name,
            definition.default_stage_kinds(),
            profile,
        );
        plan.stages = definition.resolved_stage_kinds(profile);
        plan
    }

    pub fn orchestrator_from_profile(
        self,
        profile_name: Option<String>,
        profile: &SchedulerProfileConfig,
        tool_runner: ToolRunner,
    ) -> SchedulerProfileOrchestrator {
        SchedulerProfileOrchestrator::new(
            self.plan_from_profile(profile_name, profile),
            tool_runner,
        )
    }
}

pub fn scheduler_stage_observability(
    scheduler_profile: &str,
    stage_name: &str,
) -> Option<SchedulerStageObservability> {
    let preset = SchedulerPresetKind::from_str(scheduler_profile).ok()?;
    let stage = SchedulerStageKind::from_event_name(stage_name)?;
    Some(preset.stage_observability(stage))
}

pub fn scheduler_plan_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
) -> Result<SchedulerProfilePlan, SchedulerConfigError> {
    Ok(SchedulerPresetKind::from_profile_config(profile)?.plan_from_profile(profile_name, profile))
}

pub fn scheduler_plan_from_config(
    config: &SchedulerConfig,
) -> Result<SchedulerProfilePlan, SchedulerConfigError> {
    let profile_name = config
        .default_profile_key()
        .ok_or_else(|| SchedulerConfigError::ProfileNotFound("<default>".to_string()))?;
    let profile = config.profile(profile_name)?;
    scheduler_plan_from_profile(Some(profile_name.to_string()), profile)
}

pub fn scheduler_plan_from_file(
    path: impl AsRef<Path>,
) -> Result<SchedulerProfilePlan, SchedulerConfigError> {
    let config = SchedulerConfig::load_from_file(path)?;
    scheduler_plan_from_config(&config)
}

pub fn scheduler_request_defaults_from_plan(
    plan: &SchedulerProfilePlan,
) -> SchedulerRequestDefaults {
    SchedulerRequestDefaults {
        profile_name: plan.profile_name.clone(),
        root_agent_name: plan
            .agent_tree
            .as_ref()
            .map(|node| node.agent.name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        skill_tree_plan: plan.skill_tree.clone(),
    }
}

pub fn scheduler_request_defaults_from_config(
    config: &SchedulerConfig,
) -> Result<SchedulerRequestDefaults, SchedulerConfigError> {
    let plan = scheduler_plan_from_config(config)?;
    Ok(scheduler_request_defaults_from_plan(&plan))
}

pub fn scheduler_request_defaults_from_file(
    path: impl AsRef<Path>,
) -> Result<SchedulerRequestDefaults, SchedulerConfigError> {
    let config = SchedulerConfig::load_from_file(path)?;
    scheduler_request_defaults_from_config(&config)
}

pub fn scheduler_orchestrator_from_profile(
    profile_name: Option<String>,
    profile: &SchedulerProfileConfig,
    tool_runner: ToolRunner,
) -> Result<SchedulerProfileOrchestrator, SchedulerConfigError> {
    Ok(
        SchedulerPresetKind::from_profile_config(profile)?.orchestrator_from_profile(
            profile_name,
            profile,
            tool_runner,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::SchedulerStageKind;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn checked_in_scheduler_example_path(file_name: &str) -> PathBuf {
        repo_root().join("docs/examples/scheduler").join(file_name)
    }

    fn assert_checked_in_public_scheduler_example(
        file_name: &str,
        profile_name: &str,
        orchestrator: &str,
        expected_stages: Vec<SchedulerStageKind>,
    ) {
        let path = checked_in_scheduler_example_path(file_name);
        let config = SchedulerConfig::load_from_file(&path)
            .unwrap_or_else(|err| panic!("example {} should parse: {}", path.display(), err));
        let profile = config.default_profile().unwrap_or_else(|err| {
            panic!(
                "example {} should resolve default profile: {}",
                path.display(),
                err
            )
        });
        let plan = scheduler_plan_from_config(&config).unwrap_or_else(|err| {
            panic!(
                "example {} should resolve scheduler plan: {}",
                path.display(),
                err
            )
        });

        assert_eq!(config.default_profile_key(), Some(profile_name));
        assert_eq!(profile.orchestrator.as_deref(), Some(orchestrator));
        assert_eq!(profile.stages, expected_stages);
        assert_eq!(plan.profile_name.as_deref(), Some(profile_name));
        assert_eq!(plan.orchestrator.as_deref(), Some(orchestrator));
        assert_eq!(plan.stages, expected_stages);
    }

    fn write_temp_scheduler(content: &str) -> std::path::PathBuf {
        let unique = format!(
            "rocode_scheduler_preset_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock error")
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).expect("temp dir should exist");
        let path = dir.join("scheduler.jsonc");
        fs::write(&path, content).expect("scheduler file should write");
        path
    }

    #[test]
    fn scheduler_preset_defaults_to_sisyphus() {
        let profile = SchedulerProfileConfig::default();
        let plan = scheduler_plan_from_profile(Some("default".to_string()), &profile).unwrap();
        assert_eq!(plan.profile_name.as_deref(), Some("default"));
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::Route,
                SchedulerStageKind::ExecutionOrchestration,
            ]
        );
    }

    #[test]
    fn scheduler_preset_can_resolve_prometheus() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("prometheus".to_string()),
            ..Default::default()
        };
        let plan = scheduler_plan_from_profile(Some("planner".to_string()), &profile).unwrap();
        assert_eq!(plan.profile_name.as_deref(), Some("planner"));
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::Route,
                SchedulerStageKind::Interview,
                SchedulerStageKind::Plan,
                SchedulerStageKind::Review,
                SchedulerStageKind::Handoff,
            ]
        );
    }

    #[test]
    fn scheduler_prometheus_ignores_custom_execution_stages() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("prometheus".to_string()),
            stages: vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::ExecutionOrchestration,
                SchedulerStageKind::Synthesis,
            ],
            ..Default::default()
        };
        let plan = scheduler_plan_from_profile(Some("planner".to_string()), &profile).unwrap();
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::Route,
                SchedulerStageKind::Interview,
                SchedulerStageKind::Plan,
                SchedulerStageKind::Review,
                SchedulerStageKind::Handoff,
            ]
        );
    }

    #[test]
    fn scheduler_preset_can_resolve_atlas() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("atlas".to_string()),
            ..Default::default()
        };
        let plan = scheduler_plan_from_profile(Some("atlas".to_string()), &profile).unwrap();
        assert_eq!(plan.profile_name.as_deref(), Some("atlas"));
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::ExecutionOrchestration,
                SchedulerStageKind::Synthesis,
            ]
        );
    }

    #[test]
    fn scheduler_preset_can_resolve_hephaestus() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("hephaestus".to_string()),
            ..Default::default()
        };
        let plan = scheduler_plan_from_profile(Some("hephaestus".to_string()), &profile).unwrap();
        assert_eq!(plan.profile_name.as_deref(), Some("hephaestus"));
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::ExecutionOrchestration,
            ]
        );
    }

    #[test]
    fn scheduler_public_presets_only_include_omo_presets() {
        assert_eq!(
            SchedulerPresetKind::public_presets()
                .iter()
                .map(|preset| preset.as_str())
                .collect::<Vec<_>>(),
            vec!["sisyphus", "prometheus", "atlas", "hephaestus"]
        );
        assert_eq!(
            SchedulerPresetKind::router_recommended_presets()
                .iter()
                .map(|preset| preset.as_str())
                .collect::<Vec<_>>(),
            vec!["sisyphus", "prometheus", "atlas", "hephaestus"]
        );
    }

    #[test]
    fn scheduler_omo_presets_are_public_and_recommended() {
        for preset in SchedulerPresetKind::public_presets() {
            assert!(preset.is_public(), "{} should stay public", preset.as_str());
            assert!(
                preset.is_router_recommended(),
                "{} should stay router recommended",
                preset.as_str()
            );
            assert!(
                !preset.is_deprecated(),
                "{} should not be deprecated",
                preset.as_str()
            );
        }
    }

    #[test]
    fn scheduler_plan_from_config_uses_default_profile_key() {
        let config = SchedulerConfig::load_from_str(
            r#"{
                "defaults": { "profile": "planner" },
                "profiles": {
                    "planner": { "orchestrator": "prometheus" }
                }
            }"#,
        )
        .unwrap();

        let plan = scheduler_plan_from_config(&config).unwrap();
        assert_eq!(plan.profile_name.as_deref(), Some("planner"));
        assert_eq!(
            plan.stages,
            vec![
                SchedulerStageKind::RequestAnalysis,
                SchedulerStageKind::Route,
                SchedulerStageKind::Interview,
                SchedulerStageKind::Plan,
                SchedulerStageKind::Review,
                SchedulerStageKind::Handoff,
            ]
        );
    }

    #[test]
    fn scheduler_request_defaults_extract_root_agent_and_skill_tree() {
        let path = write_temp_scheduler(
            r#"{
                "defaults": { "profile": "delivery" },
                "profiles": {
                    "delivery": {
                        "orchestrator": "sisyphus",
                        "skillTree": { "contextMarkdown": "External scheduler context" },
                        "agentTree": {
                            "agent": { "name": "deep-worker" }
                        }
                    }
                }
            }"#,
        );

        let defaults = scheduler_request_defaults_from_file(&path).unwrap();
        assert_eq!(defaults.profile_name.as_deref(), Some("delivery"));
        assert_eq!(defaults.root_agent_name.as_deref(), Some("deep-worker"));
        assert_eq!(
            defaults
                .skill_tree_plan
                .as_ref()
                .map(|tree| tree.context_markdown.as_str()),
            Some("External scheduler context")
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn checked_in_public_scheduler_examples_align_with_runtime_defaults() {
        assert_checked_in_public_scheduler_example(
            "sisyphus.example.jsonc",
            "sisyphus-default",
            "sisyphus",
            crate::scheduler::sisyphus_default_stages(),
        );
        assert_checked_in_public_scheduler_example(
            "prometheus.example.jsonc",
            "prometheus-default",
            "prometheus",
            crate::scheduler::prometheus_default_stages(),
        );
        assert_checked_in_public_scheduler_example(
            "atlas.example.jsonc",
            "atlas-default",
            "atlas",
            crate::scheduler::atlas_default_stages(),
        );
        assert_checked_in_public_scheduler_example(
            "hephaestus.example.jsonc",
            "hephaestus-default",
            "hephaestus",
            crate::scheduler::hephaestus_default_stages(),
        );
    }

    #[test]
    fn scheduler_preset_rejects_unknown_orchestrator() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("unknown".to_string()),
            ..Default::default()
        };
        let err = scheduler_plan_from_profile(None, &profile).unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported scheduler orchestrator"));
    }
}
