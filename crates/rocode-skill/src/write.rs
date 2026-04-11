use crate::discovery::is_valid_relative_skill_path;
use crate::{SkillError, SkillMeta};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SKILL_MARKDOWN_BYTES: usize = 256 * 1024;
const MAX_SUPPORTING_FILE_BYTES: usize = 512 * 1024;
const SKILL_MARKDOWN_FILE_NAME: &str = "SKILL.md";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    pub body: String,
    pub category: Option<String>,
    pub directory_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchSkillRequest {
    pub name: String,
    pub new_name: Option<String>,
    pub description: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditSkillRequest {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteSkillFileRequest {
    pub name: String,
    pub file_path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveSkillFileRequest {
    pub name: String,
    pub file_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteSkillRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillWriteAction {
    Created,
    Patched,
    Edited,
    SupportingFileWritten,
    SupportingFileRemoved,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillWriteResult {
    pub action: SkillWriteAction,
    pub skill_name: String,
    pub location: PathBuf,
    pub skill: Option<SkillMeta>,
    pub supporting_file: Option<String>,
}

impl SkillWriteResult {
    pub(crate) fn with_skill(action: SkillWriteAction, skill: SkillMeta) -> Self {
        Self {
            action,
            skill_name: skill.name.clone(),
            location: skill.location.clone(),
            skill: Some(skill),
            supporting_file: None,
        }
    }

    pub(crate) fn deleted(skill_name: String, location: PathBuf) -> Self {
        Self {
            action: SkillWriteAction::Deleted,
            skill_name,
            location,
            skill: None,
            supporting_file: None,
        }
    }

    pub(crate) fn with_supporting_file(mut self, file_path: impl Into<String>) -> Self {
        self.supporting_file = Some(file_path.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedSkillDocument {
    pub frontmatter_lines: Vec<String>,
    pub body: String,
}

pub(crate) fn workspace_skill_root(base_dir: &Path) -> PathBuf {
    base_dir.join(".rocode").join("skills")
}

pub(crate) fn resolve_create_skill_markdown_path(
    base_dir: &Path,
    req: &CreateSkillRequest,
) -> Result<PathBuf, SkillError> {
    let root = workspace_skill_root(base_dir);
    let mut path = root;
    if let Some(category) = normalize_category_path(req.category.as_deref())? {
        path.push(category);
    }
    path.push(normalize_directory_name(
        req.directory_name.as_deref(),
        &req.name,
    )?);
    path.push(SKILL_MARKDOWN_FILE_NAME);
    Ok(path)
}

pub(crate) fn ensure_workspace_skill_markdown(
    base_dir: &Path,
    skill_name: &str,
    skill_markdown: &Path,
) -> Result<(), SkillError> {
    let root = workspace_skill_root(base_dir);
    if !skill_markdown.starts_with(&root) {
        return Err(SkillError::SkillNotWritable {
            name: skill_name.to_string(),
            path: skill_markdown.to_path_buf(),
        });
    }

    if skill_markdown.file_name().and_then(|value| value.to_str()) != Some(SKILL_MARKDOWN_FILE_NAME)
    {
        return Err(SkillError::InvalidWriteTarget {
            path: skill_markdown.to_path_buf(),
        });
    }

    Ok(())
}

pub(crate) fn supporting_file_path(
    skill_markdown: &Path,
    file_path: &str,
) -> Result<PathBuf, SkillError> {
    if !is_valid_relative_skill_path(file_path) {
        return Err(SkillError::InvalidSkillFilePath {
            skill: skill_markdown.to_string_lossy().to_string(),
            file_path: file_path.to_string(),
        });
    }
    if file_path.eq_ignore_ascii_case(SKILL_MARKDOWN_FILE_NAME) {
        return Err(SkillError::InvalidSkillFilePath {
            skill: skill_markdown.to_string_lossy().to_string(),
            file_path: file_path.to_string(),
        });
    }

    let skill_dir = skill_markdown
        .parent()
        .ok_or_else(|| SkillError::InvalidWriteTarget {
            path: skill_markdown.to_path_buf(),
        })?;
    Ok(skill_dir.join(file_path))
}

pub(crate) fn validate_skill_name(name: &str) -> Result<String, SkillError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(|ch| ch.is_control())
        || trimmed.contains('\n')
        || trimmed.contains('\r')
    {
        return Err(SkillError::InvalidSkillName {
            name: name.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

pub(crate) fn validate_skill_description(
    skill_name: &str,
    description: &str,
) -> Result<String, SkillError> {
    let trimmed = description.trim();
    if trimmed.is_empty() || trimmed.chars().any(|ch| ch.is_control()) {
        return Err(SkillError::InvalidSkillDescription {
            name: skill_name.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

pub(crate) fn validate_skill_body(body: &str) -> Result<String, SkillError> {
    if body.trim().is_empty() {
        return Err(SkillError::InvalidSkillContent {
            message: "skill body must not be empty".to_string(),
        });
    }
    Ok(body.replace("\r\n", "\n").trim().to_string())
}

pub(crate) fn validate_skill_markdown_size(content: &str, path: &str) -> Result<(), SkillError> {
    ensure_size_limit(path, content.len(), MAX_SKILL_MARKDOWN_BYTES)
}

pub(crate) fn validate_supporting_file_size(path: &str, content: &str) -> Result<(), SkillError> {
    ensure_size_limit(path, content.len(), MAX_SUPPORTING_FILE_BYTES)
}

pub(crate) fn build_skill_document(name: &str, description: &str, body: &str) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}\n",
        quote_yaml_string(name),
        quote_yaml_string(description),
        body.trim()
    )
}

pub(crate) fn load_skill_document(path: &Path) -> Result<ParsedSkillDocument, SkillError> {
    let content = fs::read_to_string(path).map_err(|error| SkillError::ReadFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    parse_skill_document(&content)
}

pub(crate) fn parse_skill_document(content: &str) -> Result<ParsedSkillDocument, SkillError> {
    let normalized = content.replace("\r\n", "\n");
    let mut lines = normalized.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(SkillError::InvalidSkillFrontmatter {
            message: "missing opening `---`".to_string(),
        });
    }

    let mut frontmatter_lines = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        frontmatter_lines.push(line.to_string());
    }

    if !closed {
        return Err(SkillError::InvalidSkillFrontmatter {
            message: "missing closing `---`".to_string(),
        });
    }

    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Ok(ParsedSkillDocument {
        frontmatter_lines,
        body,
    })
}

pub(crate) fn render_skill_document(document: &ParsedSkillDocument) -> String {
    let mut out = String::from("---\n");
    for line in &document.frontmatter_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("---\n");
    if !document.body.is_empty() {
        out.push('\n');
        out.push_str(document.body.trim());
        out.push('\n');
    }
    out
}

pub(crate) fn upsert_frontmatter_value(lines: &mut Vec<String>, key: &str, value: &str) {
    let rendered = format!("{key}: {}", quote_yaml_string(value));
    if let Some(line) = lines.iter_mut().find(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with(&format!("{key}:"))
    }) {
        *line = rendered;
    } else {
        lines.push(rendered);
    }
}

pub(crate) fn read_frontmatter_value(lines: &[String], key: &str) -> Option<String> {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix(&format!("{key}:")) {
            let value = value.trim();
            if value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')))
            {
                return Some(value[1..value.len() - 1].to_string());
            }
            return Some(value.to_string());
        }
    }
    None
}

pub(crate) fn atomic_write_string(path: &Path, content: &str) -> Result<(), SkillError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| SkillError::WriteFailed {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }

    let temp_path = temp_path_for(path);
    fs::write(&temp_path, content).map_err(|error| SkillError::WriteFailed {
        path: temp_path.clone(),
        message: error.to_string(),
    })?;
    fs::rename(&temp_path, path).map_err(|error| SkillError::WriteFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(())
}

pub(crate) fn delete_file(
    path: &Path,
    skill_name: &str,
    file_path: &str,
) -> Result<(), SkillError> {
    if !path.exists() {
        return Err(SkillError::SkillFileNotFound {
            skill: skill_name.to_string(),
            file_path: file_path.to_string(),
        });
    }
    fs::remove_file(path).map_err(|error| SkillError::WriteFailed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(())
}

pub(crate) fn delete_skill_directory(path: &Path) -> Result<(), SkillError> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|error| SkillError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    } else {
        fs::remove_file(path).map_err(|error| SkillError::WriteFailed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    Ok(())
}

pub(crate) fn prune_empty_skill_parent_dirs(path: &Path, stop_at: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }
        match fs::read_dir(dir) {
            Ok(entries) => {
                if entries.count() == 0 {
                    let _ = fs::remove_dir(dir);
                    current = dir.parent();
                } else {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn ensure_size_limit(path: &str, size: usize, limit: usize) -> Result<(), SkillError> {
    if size > limit {
        return Err(SkillError::SkillWriteSizeExceeded {
            path: path.to_string(),
            size,
            limit,
        });
    }
    Ok(())
}

fn normalize_category_path(category: Option<&str>) -> Result<Option<PathBuf>, SkillError> {
    let Some(category) = category.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if !is_valid_relative_skill_path(category) {
        return Err(SkillError::InvalidSkillCategory {
            category: category.to_string(),
        });
    }

    Ok(Some(PathBuf::from(category)))
}

fn normalize_directory_name(
    directory_name: Option<&str>,
    skill_name: &str,
) -> Result<String, SkillError> {
    let candidate = directory_name.unwrap_or(skill_name).trim();
    if candidate.is_empty() {
        return Err(SkillError::InvalidWriteTarget {
            path: PathBuf::from(candidate),
        });
    }

    if let Some(dir_name) = directory_name {
        if !is_valid_relative_skill_path(dir_name) {
            return Err(SkillError::InvalidWriteTarget {
                path: PathBuf::from(dir_name),
            });
        }
        let value = dir_name.trim_matches('/');
        if value.is_empty() || value.contains('/') {
            return Err(SkillError::InvalidWriteTarget {
                path: PathBuf::from(dir_name),
            });
        }
        return Ok(value.to_string());
    }

    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in candidate.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
            continue;
        }
        if matches!(ch, '-' | '_' | ' ' | '.') && !last_was_sep && !slug.is_empty() {
            slug.push('-');
            last_was_sep = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err(SkillError::InvalidSkillName {
            name: skill_name.to_string(),
        });
    }
    Ok(slug)
}

fn quote_yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{}\"", value))
}

fn temp_path_for(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skill.tmp");
    path.with_file_name(format!(".{file_name}.{nanos}.tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SkillAuthority;
    use rocode_config::{Config, ConfigStore};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn create_skill_writes_into_workspace_root_and_is_immediately_visible() {
        let dir = tempdir().unwrap();
        let authority = SkillAuthority::new(dir.path(), None);

        let result = authority
            .create_skill(CreateSkillRequest {
                name: "workspace-reviewer".to_string(),
                description: "review things".to_string(),
                body: "Check correctness first.".to_string(),
                category: Some("analysis".to_string()),
                directory_name: None,
            })
            .unwrap();

        assert_eq!(result.action, SkillWriteAction::Created);
        let skill = result.skill.unwrap();
        assert_eq!(skill.name, "workspace-reviewer");
        assert_eq!(skill.category.as_deref(), Some("analysis"));
        assert!(skill.location.exists());

        let names = authority
            .list_skill_meta(None)
            .unwrap()
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"workspace-reviewer".to_string()));
    }

    #[test]
    fn patch_skill_rejects_non_workspace_roots() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let external_skill = root.join("external-skills/reviewer/SKILL.md");
        fs::create_dir_all(external_skill.parent().unwrap()).unwrap();
        fs::write(
            &external_skill,
            "---\nname: external-reviewer\ndescription: outside\n---\n\nExternal.\n",
        )
        .unwrap();

        let mut config = Config::default();
        config
            .skill_paths
            .insert("external".to_string(), "external-skills".to_string());
        let authority = SkillAuthority::new(root, Some(Arc::new(ConfigStore::new(config))));

        let err = authority
            .patch_skill(PatchSkillRequest {
                name: "external-reviewer".to_string(),
                new_name: None,
                description: Some("new".to_string()),
                body: None,
            })
            .unwrap_err();

        assert!(matches!(err, SkillError::SkillNotWritable { .. }));
    }

    #[test]
    fn patch_skill_updates_frontmatter_and_body() {
        let dir = tempdir().unwrap();
        let authority = SkillAuthority::new(dir.path(), None);
        authority
            .create_skill(CreateSkillRequest {
                name: "review-skill".to_string(),
                description: "review".to_string(),
                body: "Old body.".to_string(),
                category: None,
                directory_name: None,
            })
            .unwrap();

        let result = authority
            .patch_skill(PatchSkillRequest {
                name: "review-skill".to_string(),
                new_name: Some("review-skill-v2".to_string()),
                description: Some("better review".to_string()),
                body: Some("New body.".to_string()),
            })
            .unwrap();

        let skill = result.skill.unwrap();
        assert_eq!(skill.name, "review-skill-v2");
        assert_eq!(skill.description, "better review");
        let loaded = authority.load_skill("review-skill-v2", None).unwrap();
        assert!(loaded.content.contains("New body."));
    }

    #[test]
    fn supporting_file_write_and_remove_rejects_path_escape() {
        let dir = tempdir().unwrap();
        let authority = SkillAuthority::new(dir.path(), None);
        authority
            .create_skill(CreateSkillRequest {
                name: "writer".to_string(),
                description: "writer".to_string(),
                body: "Base body.".to_string(),
                category: None,
                directory_name: None,
            })
            .unwrap();

        let err = authority
            .write_supporting_file(WriteSkillFileRequest {
                name: "writer".to_string(),
                file_path: "../escape.md".to_string(),
                content: "oops".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, SkillError::InvalidSkillFilePath { .. }));

        let err = authority
            .remove_supporting_file(RemoveSkillFileRequest {
                name: "writer".to_string(),
                file_path: "../escape.md".to_string(),
            })
            .unwrap_err();
        assert!(matches!(err, SkillError::InvalidSkillFilePath { .. }));
    }

    #[test]
    fn delete_skill_removes_directory_and_refreshes_catalog() {
        let dir = tempdir().unwrap();
        let authority = SkillAuthority::new(dir.path(), None);
        let created = authority
            .create_skill(CreateSkillRequest {
                name: "delete-me".to_string(),
                description: "delete".to_string(),
                body: "Soon gone.".to_string(),
                category: None,
                directory_name: None,
            })
            .unwrap();

        let location = created.location.clone();
        let skill_dir = location.parent().unwrap().to_path_buf();
        let deleted = authority
            .delete_skill(DeleteSkillRequest {
                name: "delete-me".to_string(),
            })
            .unwrap();

        assert_eq!(deleted.action, SkillWriteAction::Deleted);
        assert_eq!(deleted.skill, None);
        assert!(!skill_dir.exists());
        assert!(authority.resolve_skill("delete-me", None).is_err());
    }
}
