use super::SchedulerStageKind;
use crate::agent_tree::AgentTreeNode;
use crate::scheduler::{AvailableAgentMeta, AvailableCategoryMeta};
use crate::skill_graph::SkillGraphDefinition;
use crate::skill_tree::SkillTreeRequestPlan;
use crate::ModelRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum SchedulerConfigError {
    #[error("failed to read scheduler config: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse scheduler config as jsonc: {0}")]
    Parse(String),

    #[error("failed to deserialize scheduler config: {0}")]
    Deserialize(#[from] serde_json::Error),

    #[error("scheduler profile not found: {0}")]
    ProfileNotFound(String),

    #[error("unsupported scheduler orchestrator: {0}")]
    UnknownOrchestrator(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerConfig {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<SchedulerDefaults>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub profiles: HashMap<String, SchedulerProfileConfig>,
}

impl SchedulerConfig {
    pub fn load_from_str(content: &str) -> Result<Self, SchedulerConfigError> {
        let parse_options = jsonc_parser::ParseOptions {
            allow_trailing_commas: true,
            ..Default::default()
        };
        let value = jsonc_parser::parse_to_serde_value(content, &parse_options)
            .map_err(|err| SchedulerConfigError::Parse(err.to_string()))?
            .ok_or_else(|| SchedulerConfigError::Parse("empty scheduler config".to_string()))?;

        Ok(serde_json::from_value(value)?)
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, SchedulerConfigError> {
        let content = fs::read_to_string(path)?;
        Self::load_from_str(&content)
    }

    pub fn profile(&self, key: &str) -> Result<&SchedulerProfileConfig, SchedulerConfigError> {
        self.profiles
            .get(key)
            .ok_or_else(|| SchedulerConfigError::ProfileNotFound(key.to_string()))
    }

    pub fn default_profile_key(&self) -> Option<&str> {
        self.defaults.as_ref()?.profile.as_deref()
    }

    pub fn default_profile(&self) -> Result<&SchedulerProfileConfig, SchedulerConfigError> {
        let key = self
            .default_profile_key()
            .ok_or_else(|| SchedulerConfigError::ProfileNotFound("<default>".to_string()))?;
        self.profile(key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchedulerProfileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,

    #[serde(default, alias = "skillList", skip_serializing_if = "Vec::is_empty")]
    pub skill_list: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<SchedulerStageKind>,

    #[serde(default, alias = "agentTree", skip_serializing_if = "Option::is_none")]
    pub agent_tree: Option<AgentTreeNode>,

    #[serde(default, alias = "skillGraph", skip_serializing_if = "Option::is_none")]
    pub skill_graph: Option<SkillGraphDefinition>,

    #[serde(default, alias = "skillTree", skip_serializing_if = "Option::is_none")]
    pub skill_tree: Option<SkillTreeRequestPlan>,

    #[serde(
        default,
        alias = "availableAgents",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub available_agents: Vec<AvailableAgentMeta>,

    #[serde(
        default,
        alias = "availableCategories",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub available_categories: Vec<AvailableCategoryMeta>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_config_parses_jsonc_profile() {
        let content = r#"
        {
          // comment
          "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
          "defaults": { "profile": "prometheus-default" },
          "profiles": {
            "prometheus-default": {
              "orchestrator": "prometheus",
              "model": {
                "providerId": "anthropic",
                "modelId": "claude-opus-4-6"
              },
              "skillList": ["request-analysis", "plan", "delegation"],
              "stages": ["request-analysis", "plan", "delegation", "review", "synthesis"],
              "skillTree": {
                "contextMarkdown": "Article 1: one execution kernel"
              },
              "agentTree": {
                "agent": { "name": "deep-worker" }
              },
              "skillGraph": {
                "entryNodeId": "review",
                "nodes": [
                  {
                    "id": "review",
                    "agent": { "name": "architecture-advisor" }
                  }
                ]
              }
            }
          }
        }
        "#;

        let config = SchedulerConfig::load_from_str(content).unwrap();
        let profile = config.default_profile().unwrap();
        assert_eq!(profile.orchestrator.as_deref(), Some("prometheus"));
        assert_eq!(
            profile
                .model
                .as_ref()
                .map(|model| model.provider_id.as_str()),
            Some("anthropic")
        );
        assert_eq!(
            profile.model.as_ref().map(|model| model.model_id.as_str()),
            Some("claude-opus-4-6")
        );
        assert_eq!(profile.skill_list.len(), 3);
        assert_eq!(profile.stages.len(), 5);
        assert!(profile.agent_tree.is_some());
        assert!(profile.skill_graph.is_some());
        assert_eq!(
            profile
                .skill_tree
                .as_ref()
                .map(|tree| tree.context_markdown.as_str()),
            Some("Article 1: one execution kernel")
        );
    }

    #[test]
    fn scheduler_config_handles_empty_profiles() {
        let config = SchedulerConfig::load_from_str("{}").unwrap();
        assert!(config.defaults.is_none());
        assert!(config.profiles.is_empty());
    }
}
