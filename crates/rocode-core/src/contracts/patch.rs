use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

/// Shared metadata keys produced by patch/edit/write-style tools.
pub mod keys {
    /// Unified diff text (string).
    pub const DIFF: &str = "diff";
    /// Per-file change metadata (array).
    pub const FILES: &str = "files";
    /// LSP-style diagnostics payload (array).
    pub const DIAGNOSTICS: &str = "diagnostics";

    /// File entry absolute path (string).
    pub const FILE_PATH: &str = "filePath";
    /// Common snake_case file path key used by many tools (`read`, `write`, `edit`, ...).
    pub const FILE_PATH_SNAKE: &str = "file_path";
    /// File entry relative path (string).
    pub const RELATIVE_PATH: &str = "relativePath";
    /// File entry change type (string; see [`FileChangeType`]).
    pub const CHANGE_TYPE: &str = "type";
    /// File entry diff text (string).
    pub const FILE_DIFF: &str = "diff";
    /// File entry pre-change content snapshot (string).
    pub const BEFORE: &str = "before";
    /// File entry post-change content snapshot (string).
    pub const AFTER: &str = "after";
    /// File entry move/rename destination (string).
    pub const MOVE_PATH: &str = "movePath";

    /// Legacy key some clients may still emit for paths.
    pub const LEGACY_PATH: &str = "path";

    /// Common lowercased file path key used in tool metadata and permission prompts.
    pub const FILEPATH: &str = "filepath";
    /// File content body for write/edit style tool inputs.
    pub const CONTENT: &str = "content";
    /// Edit-tool old string argument key.
    pub const OLD_STRING: &str = "old_string";
    /// Edit-tool new string argument key.
    pub const NEW_STRING: &str = "new_string";

    /// File write/edit summary: number of bytes written.
    pub const BYTES: &str = "bytes";
    /// File write/edit summary: number of lines written.
    pub const LINES: &str = "lines";
    /// File write summary: whether the target existed before the operation.
    pub const EXISTS: &str = "exists";
    /// File edit summary: number of replacements performed.
    pub const REPLACEMENTS: &str = "replacements";
}

/// File change type strings used in tool metadata payloads.
///
/// Wire format: lowercase strings (`"add"`, `"update"`, `"delete"`, `"move"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum FileChangeType {
    Add,
    Update,
    Delete,
    Move,
}

impl std::fmt::Display for FileChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FileChangeType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Move => "move",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}
