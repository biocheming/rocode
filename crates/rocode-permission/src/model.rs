use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;

use rocode_core::contracts::tools::BuiltinToolName;

use crate::matching::wildcard_match;

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    })
}

// ============================================================================
// Canonical permission primitives
// ============================================================================

/// Result of a permission prompt reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionReply {
    #[serde(alias = "approve", alias = "allow")]
    Once,
    Always,
    Reject,
}

impl std::fmt::Display for PermissionReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        })
    }
}

/// Canonical permission kind.
///
/// This wraps both stable built-in tool permissions and a few synthetic
/// permission channels (e.g. `external_directory`, `doom_loop`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    ExternalDirectory,
    List,
    DoomLoop,
    Tool(BuiltinToolName),
    Custom(String),
}

impl PermissionKind {
    pub fn from_name(value: impl AsRef<str>) -> Self {
        let raw = value.as_ref().trim();
        if raw.is_empty() {
            return Self::Custom(String::new());
        }

        match raw.to_ascii_lowercase().as_str() {
            "external_directory" | "externaldirectory" | "external-directory" => {
                return Self::ExternalDirectory;
            }
            "list" => return Self::List,
            "doom_loop" | "doomloop" | "doom-loop" => return Self::DoomLoop,
            _ => {}
        }

        if let Some(tool) = BuiltinToolName::parse(raw) {
            return Self::Tool(tool);
        }

        Self::Custom(raw.to_string())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::ExternalDirectory => "external_directory",
            Self::List => "list",
            Self::DoomLoop => "doom_loop",
            Self::Tool(tool) => tool.as_str(),
            Self::Custom(raw) => raw.as_str(),
        }
    }

    /// Canonical permission kind for a tool invocation name.
    ///
    /// - edit-family tools (`write`, `edit`, `multiedit`, `apply_patch`) map to `edit`
    /// - list aliases (`ls`, `list`, ...) map to `list`
    /// - all other built-ins map to their canonical built-in id
    /// - unknown names fall back to `from_name`
    pub fn from_tool_name(value: impl AsRef<str>) -> Self {
        match BuiltinToolName::parse(value.as_ref()) {
            Some(
                BuiltinToolName::Write
                | BuiltinToolName::Edit
                | BuiltinToolName::MultiEdit
                | BuiltinToolName::ApplyPatch,
            ) => Self::Tool(BuiltinToolName::Edit),
            Some(BuiltinToolName::Ls) => Self::List,
            Some(tool) => Self::Tool(tool),
            None => Self::from_name(value),
        }
    }

    pub fn from_tool(tool: BuiltinToolName) -> Self {
        Self::from_tool_name(tool.as_str())
    }

    pub fn label(&self) -> Cow<'static, str> {
        match self {
            Self::ExternalDirectory => Cow::Borrowed("External directory access"),
            Self::List => Cow::Borrowed("List directory"),
            Self::DoomLoop => Cow::Borrowed("Doom-loop safeguard"),
            Self::Tool(tool) => match tool {
                BuiltinToolName::Read => Cow::Borrowed("Read file"),
                BuiltinToolName::Write => Cow::Borrowed("Write file"),
                BuiltinToolName::Edit
                | BuiltinToolName::MultiEdit
                | BuiltinToolName::ApplyPatch => Cow::Borrowed("Edit file"),
                BuiltinToolName::Bash | BuiltinToolName::ShellSession => {
                    Cow::Borrowed("Run shell command")
                }
                BuiltinToolName::Glob => Cow::Borrowed("Glob search"),
                BuiltinToolName::Grep => Cow::Borrowed("Grep search"),
                BuiltinToolName::Task | BuiltinToolName::TaskFlow => {
                    Cow::Borrowed("Task operation")
                }
                BuiltinToolName::WebFetch => Cow::Borrowed("Fetch web content"),
                BuiltinToolName::WebSearch => Cow::Borrowed("Web search"),
                BuiltinToolName::CodeSearch | BuiltinToolName::AstGrepSearch => {
                    Cow::Borrowed("Code search")
                }
                BuiltinToolName::TodoRead => Cow::Borrowed("Read todos"),
                BuiltinToolName::TodoWrite => Cow::Borrowed("Write todos"),
                BuiltinToolName::MediaInspect => Cow::Borrowed("Inspect media"),
                BuiltinToolName::BrowserSession => Cow::Borrowed("Browser session"),
                BuiltinToolName::ContextDocs => Cow::Borrowed("Context docs"),
                BuiltinToolName::GitHubResearch => Cow::Borrowed("GitHub research"),
                BuiltinToolName::RepoHistory => Cow::Borrowed("Repository history"),
                BuiltinToolName::Lsp => Cow::Borrowed("Language server operation"),
                BuiltinToolName::Question => Cow::Borrowed("Ask user question"),
                BuiltinToolName::PlanEnter | BuiltinToolName::PlanExit => {
                    Cow::Borrowed("Plan workflow control")
                }
                BuiltinToolName::Skill => Cow::Borrowed("Load skill"),
                BuiltinToolName::Batch => Cow::Borrowed("Batch execution"),
                BuiltinToolName::AstGrepReplace => Cow::Borrowed("AST replace"),
                BuiltinToolName::NotebookEdit => Cow::Borrowed("Notebook edit"),
                BuiltinToolName::Invalid => Cow::Borrowed("Invalid tool"),
                BuiltinToolName::Ls => Cow::Borrowed("List directory"),
            },
            Self::Custom(raw) => Cow::Owned(raw.clone()),
        }
    }

    pub const fn icon(&self) -> &'static str {
        match self {
            Self::ExternalDirectory => "[D]",
            Self::List => "[L]",
            Self::DoomLoop => "[!]",
            Self::Tool(tool) => match tool {
                BuiltinToolName::Read => "[R]",
                BuiltinToolName::Write => "[W]",
                BuiltinToolName::Edit
                | BuiltinToolName::MultiEdit
                | BuiltinToolName::ApplyPatch
                | BuiltinToolName::AstGrepReplace => "[E]",
                BuiltinToolName::Bash | BuiltinToolName::ShellSession => "[!]",
                BuiltinToolName::Glob => "[G]",
                BuiltinToolName::Grep => "[S]",
                BuiltinToolName::Task | BuiltinToolName::TaskFlow | BuiltinToolName::Batch => "[T]",
                BuiltinToolName::WebFetch
                | BuiltinToolName::WebSearch
                | BuiltinToolName::GitHubResearch
                | BuiltinToolName::BrowserSession => "[N]",
                BuiltinToolName::CodeSearch | BuiltinToolName::AstGrepSearch => "[C]",
                BuiltinToolName::TodoRead | BuiltinToolName::TodoWrite => "[✓]",
                BuiltinToolName::Question => "[?]",
                _ => "[*]",
            },
            Self::Custom(_) => "[*]",
        }
    }
}

impl std::fmt::Display for PermissionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for PermissionKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PermissionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_name(raw))
    }
}

impl From<&str> for PermissionKind {
    fn from(value: &str) -> Self {
        Self::from_name(value)
    }
}

impl From<String> for PermissionKind {
    fn from(value: String) -> Self {
        Self::from_name(value)
    }
}

impl From<BuiltinToolName> for PermissionKind {
    fn from(value: BuiltinToolName) -> Self {
        Self::from_tool(value)
    }
}

impl PartialEq<&str> for PermissionKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<PermissionKind> for &str {
    fn eq(&self, other: &PermissionKind) -> bool {
        *self == other.as_str()
    }
}

/// Pattern-matcher for permission names in rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PermissionMatcher(String);

impl PermissionMatcher {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn any() -> Self {
        Self("*".to_string())
    }

    pub fn from_kind(kind: impl Into<PermissionKind>) -> Self {
        Self(kind.into().to_string())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn matches_name(&self, permission_name: &str) -> bool {
        wildcard_match(permission_name, self.as_str())
    }
}

/// Canonicalize a tool invocation name to a stable identifier for allowlist checks.
pub fn canonicalize_tool_name(value: impl AsRef<str>) -> String {
    let raw = value.as_ref().trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Some(tool) = BuiltinToolName::parse(raw) {
        return tool.as_str().to_string();
    }
    raw.to_ascii_lowercase().replace('-', "_")
}

/// Returns true when `tool_name` is allowed by `allowlist`.
///
/// Empty allowlist means no filtering.
pub fn allowlist_allows_tool(tool_name: &str, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    let requested = canonicalize_tool_name(tool_name);
    allowlist
        .iter()
        .map(canonicalize_tool_name)
        .any(|allowed| !allowed.is_empty() && allowed == requested)
}

impl std::fmt::Display for PermissionMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PermissionMatcher {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PermissionMatcher {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<PermissionKind> for PermissionMatcher {
    fn from(value: PermissionKind) -> Self {
        Self::from_kind(value)
    }
}

impl From<BuiltinToolName> for PermissionMatcher {
    fn from(value: BuiltinToolName) -> Self {
        Self::from_kind(value)
    }
}

// ============================================================================
// Session-level permission memory (allow/deny/mode)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SessionPermissionMode {
    Ask,
    Allow,
    Deny,
    Custom(String),
}

impl SessionPermissionMode {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "ask" => Self::Ask,
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Custom(raw) => raw.as_str(),
        }
    }
}

impl std::fmt::Display for SessionPermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SessionPermissionMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SessionPermissionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(&raw))
    }
}

/// Session-scoped permission memory model.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionPermissionRuleset {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SessionPermissionMode>,
}

impl SessionPermissionRuleset {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PermissionMemoryEntry {
    pub permission: PermissionMatcher,
    pub pattern: String,
}

impl PermissionMemoryEntry {
    pub fn new(permission: impl Into<PermissionMatcher>, pattern: impl Into<String>) -> Self {
        Self {
            permission: permission.into(),
            pattern: pattern.into(),
        }
    }

    fn matches(&self, permission: &PermissionKind, pattern: &str) -> bool {
        self.permission.matches_name(permission.as_str()) && wildcard_match(pattern, &self.pattern)
    }
}

/// Session-scoped remembered approvals from "allow always" style decisions.
///
/// This replaces ad-hoc `"{permission}:{pattern}"` string concatenation.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PermissionMemory {
    #[serde(default)]
    grants: Vec<PermissionMemoryEntry>,
}

impl PermissionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grants(&self) -> &[PermissionMemoryEntry] {
        &self.grants
    }

    pub fn grant_always(&mut self, permission: impl Into<PermissionKind>, patterns: &[String]) {
        let permission = PermissionMatcher::from_kind(permission.into());
        if patterns.is_empty() {
            self.grants
                .push(PermissionMemoryEntry::new(permission, "*"));
            return;
        }
        for pattern in patterns {
            self.grants.push(PermissionMemoryEntry::new(
                permission.clone(),
                pattern.clone(),
            ));
        }
    }

    pub fn grant_request(&mut self, request: &PermissionRequest) {
        self.grant_always(request.permission.clone(), &request.patterns);
    }

    pub fn is_granted(&self, permission: impl Into<PermissionKind>, patterns: &[String]) -> bool {
        let permission = permission.into();
        if self
            .grants
            .iter()
            .any(|entry| entry.matches(&permission, "*"))
        {
            return true;
        }
        if patterns.is_empty() {
            return false;
        }
        patterns.iter().all(|pattern| {
            self.grants
                .iter()
                .any(|entry| entry.matches(&permission, pattern))
        })
    }

    pub fn is_request_granted(&self, request: &PermissionRequest) -> bool {
        self.is_granted(request.permission.clone(), &request.patterns)
    }
}

// ============================================================================
// Wire models shared by server/cli/tui/tool
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionRequestMetadata {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    pub question: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    pub command: Option<String>,
    #[serde(
        default,
        alias = "filePath",
        alias = "file_path",
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    pub filepath: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    pub path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    pub query: Option<String>,
}

impl PermissionRequestMetadata {
    fn from_map(metadata: &HashMap<String, serde_json::Value>) -> Self {
        serde_json::to_value(metadata)
            .ok()
            .and_then(|value| serde_json::from_value::<Self>(value).ok())
            .unwrap_or_default()
    }

    fn primary_hint(&self) -> Option<String> {
        self.description
            .clone()
            .or(self.question.clone())
            .or(self.command.clone())
            .or(self.filepath.clone())
            .or(self.path.clone())
            .or(self.query.clone())
    }
}

/// Canonical request payload for permission checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionRequest {
    pub permission: PermissionKind,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub always: Vec<String>,
}

impl PermissionRequest {
    pub fn new(permission: impl Into<PermissionKind>) -> Self {
        Self {
            permission: permission.into(),
            patterns: Vec::new(),
            metadata: HashMap::new(),
            always: Vec::new(),
        }
    }

    pub fn for_kind(kind: PermissionKind) -> Self {
        Self::new(kind)
    }

    pub fn for_tool(tool: BuiltinToolName) -> Self {
        Self::new(PermissionKind::from_tool(tool))
    }

    pub fn for_tool_name(tool_name: impl AsRef<str>) -> Self {
        Self::new(PermissionKind::from_tool_name(tool_name))
    }

    pub fn external_directory() -> Self {
        Self::new(PermissionKind::ExternalDirectory)
    }

    pub fn metadata_view(&self) -> PermissionRequestMetadata {
        PermissionRequestMetadata::from_map(&self.metadata)
    }

    fn display_message(&self) -> String {
        if let Some(hint) = self.metadata_view().primary_hint() {
            return hint;
        }

        if !self.patterns.is_empty() {
            return format!("{}: {}", self.permission, self.patterns.join(", "));
        }

        format!("Permission required: {}", self.permission)
    }

    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    pub fn with_patterns(mut self, patterns: Vec<String>) -> Self {
        self.patterns = patterns;
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn with_always(mut self, always: impl Into<String>) -> Self {
        self.always.push(always.into());
        self
    }

    pub fn always_allow(mut self) -> Self {
        self.always.push("*".to_string());
        self
    }
}

/// Public permission request entry used by `/permission` APIs and SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestInfo {
    pub id: String,
    #[serde(alias = "sessionID", alias = "sessionId")]
    pub session_id: String,
    pub tool: PermissionKind,
    pub input: PermissionRequest,
    pub message: String,
}

impl PermissionRequestInfo {
    pub fn from_request(
        id: impl Into<String>,
        session_id: impl Into<String>,
        request: &PermissionRequest,
    ) -> Self {
        let tool = request.permission.clone();
        let message = request.display_message();
        Self {
            id: id.into(),
            session_id: session_id.into(),
            tool,
            input: request.clone(),
            message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionReplyRequest {
    pub reply: PermissionReply,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermissionSummary {
    pub permission_id: String,
    pub info: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_kind_from_tool_name_normalizes_aliases() {
        assert_eq!(
            PermissionKind::from_tool_name("patch"),
            PermissionKind::Tool(BuiltinToolName::Edit)
        );
        assert_eq!(
            PermissionKind::from_tool_name("LIST_DIRECTORY"),
            PermissionKind::List
        );
        assert_eq!(
            PermissionKind::from_tool_name("shell"),
            PermissionKind::Tool(BuiltinToolName::Bash)
        );
    }

    #[test]
    fn allowlist_allows_tool_handles_alias_and_case() {
        assert!(allowlist_allows_tool("RiPgReP", &["grep".to_string()]));
        assert!(allowlist_allows_tool(
            "taskFlow",
            &["task_flow".to_string()]
        ));
        assert!(!allowlist_allows_tool("write", &["read".to_string()]));
    }

    #[test]
    fn permission_memory_grants_and_matches_requests() {
        let mut memory = PermissionMemory::new();
        memory.grant_always(
            PermissionKind::from_tool_name("bash"),
            &["cargo *".to_string()],
        );

        let granted = PermissionRequest::for_tool(BuiltinToolName::Bash)
            .with_pattern("cargo test -p rocode-permission");
        assert!(memory.is_request_granted(&granted));

        let denied = PermissionRequest::for_tool(BuiltinToolName::Bash).with_pattern("rm -rf /");
        assert!(!memory.is_request_granted(&denied));
    }

    #[test]
    fn permission_memory_blanket_grant_works_for_patternless_checks() {
        let mut memory = PermissionMemory::new();
        memory.grant_always(PermissionKind::from_tool_name("edit"), &[]);

        assert!(memory.is_granted(PermissionKind::from_tool_name("write"), &["a.rs".into()]));
        assert!(memory.is_granted(PermissionKind::from_tool_name("edit"), &[]));
    }
}
