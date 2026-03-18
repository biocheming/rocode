use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

/// Common metadata keys used by permission requests across tools and UI layers.
pub mod keys {
    /// Human-readable permission prompt description.
    pub const DESCRIPTION: &str = "description";
    /// Alternate prompt key used by some tools.
    pub const QUESTION: &str = "question";
    /// Command string that triggered the permission request.
    pub const COMMAND: &str = "command";

    /// Permission request input JSON field: permission name.
    pub const REQUEST_PERMISSION: &str = "permission";
    /// Permission request input JSON field: patterns array.
    pub const REQUEST_PATTERNS: &str = "patterns";
    /// Permission request input JSON field: metadata object.
    pub const REQUEST_METADATA: &str = "metadata";
    /// Permission request input JSON field: always allow flag.
    pub const REQUEST_ALWAYS: &str = "always";
}

/// Permission decision statuses used by the `permission.ask` hook.
///
/// Wire format: lowercase strings (`"ask"`, `"deny"`, `"allow"`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum PermissionHookStatus {
    Ask,
    Deny,
    Allow,
}

impl std::fmt::Display for PermissionHookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PermissionHookStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Deny => "deny",
            Self::Allow => "allow",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical permission type strings used in permission requests and rulesets.
///
/// These values are used as:
/// - `PermissionRequest.permission` in tool execution
/// - keys in `permission` config/rulesets
/// - UI routing (CLI/TUI) for labeling permission prompts
///
/// Keep them stable — they are part of the cross-crate wire contract.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive)]
pub enum PermissionTypeWire {
    #[strum(
        serialize = "external_directory",
        serialize = "externalDirectory",
        serialize = "external-directory"
    )]
    ExternalDirectory,

    #[strum(serialize = "list")]
    List,

    #[strum(serialize = "doom_loop", serialize = "doomLoop", serialize = "doom-loop")]
    DoomLoop,
}

impl std::fmt::Display for PermissionTypeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PermissionTypeWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExternalDirectory => "external_directory",
            Self::List => "list",
            Self::DoomLoop => "doom_loop",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical permission reply strings used by `/permission/{id}/reply`.
///
/// Wire format: lowercase strings (`"once"`, `"always"`, `"reject"`).
///
/// Keep them stable — they are part of the cross-crate wire contract between
/// frontends and the server.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum PermissionReplyWire {
    Once,
    Always,
    Reject,
}

impl std::fmt::Display for PermissionReplyWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PermissionReplyWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}
