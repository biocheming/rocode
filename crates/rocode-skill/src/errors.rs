use std::path::PathBuf;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SkillError {
    #[error("Unknown skill: {requested}. Available skills: {available}")]
    UnknownSkill {
        requested: String,
        available: String,
    },

    #[error("invalid skill file path for `{skill}`: {file_path}")]
    InvalidSkillFilePath { skill: String, file_path: String },

    #[error("skill file not found for `{skill}`: {file_path}")]
    SkillFileNotFound { skill: String, file_path: String },

    #[error("workspace skill writes are limited to `.rocode/skills`: {path}")]
    InvalidWriteTarget { path: PathBuf },

    #[error("skill `{name}` is not writable because it is outside the workspace sandbox: {path}")]
    SkillNotWritable { name: String, path: PathBuf },

    #[error("invalid skill name: {name}")]
    InvalidSkillName { name: String },

    #[error("invalid skill description for `{name}`")]
    InvalidSkillDescription { name: String },

    #[error("invalid skill content: {message}")]
    InvalidSkillContent { message: String },

    #[error("invalid skill category path: {category}")]
    InvalidSkillCategory { category: String },

    #[error("invalid skill frontmatter: {message}")]
    InvalidSkillFrontmatter { message: String },

    #[error("skill already exists: {name}")]
    SkillAlreadyExists { name: String },

    #[error("skill write size limit exceeded for `{path}`: {size} bytes > {limit} bytes")]
    SkillWriteSizeExceeded {
        path: String,
        size: usize,
        limit: usize,
    },

    #[error("failed to read skill path `{path}`: {message}")]
    ReadFailed { path: PathBuf, message: String },

    #[error("failed to write skill path `{path}`: {message}")]
    WriteFailed { path: PathBuf, message: String },
}
