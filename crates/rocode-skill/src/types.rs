use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillConditions {
    #[serde(default)]
    pub requires_tools: Vec<String>,
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
    #[serde(default)]
    pub requires_toolsets: Vec<String>,
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
    #[serde(default)]
    pub stage_filter: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFileRef {
    pub relative_path: String,
    pub location: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub location: PathBuf,
    #[serde(default)]
    pub supporting_files: Vec<SkillFileRef>,
    #[serde(default)]
    pub conditions: SkillConditions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetaView {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
}

impl From<&SkillMeta> for SkillMetaView {
    fn from(value: &SkillMeta) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
            category: value.category.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub location: PathBuf,
}

impl From<&SkillMeta> for SkillSummary {
    fn from(value: &SkillMeta) -> Self {
        Self {
            name: value.name.clone(),
            description: value.description.clone(),
            location: value.location.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedSkill {
    pub meta: SkillMeta,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedSkillFile {
    pub skill_name: String,
    pub file_path: String,
    pub location: PathBuf,
    pub content: String,
}
