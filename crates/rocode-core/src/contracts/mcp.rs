use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

/// Canonical MCP server connection status strings (wire format).
///
/// These values are produced by the server and consumed by CLI/TUI/Web.
/// Keep them stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum McpConnectionStatusWire {
    Connected,
    #[strum(serialize = "failed", serialize = "error")]
    Failed,
    NeedsAuth,
    NeedsClientRegistration,
    Disabled,
    Disconnected,
}

impl std::fmt::Display for McpConnectionStatusWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl McpConnectionStatusWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Failed => "failed",
            Self::NeedsAuth => "needs_auth",
            Self::NeedsClientRegistration => "needs_client_registration",
            Self::Disabled => "disabled",
            Self::Disconnected => "disconnected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Allow richer statuses like: "needs_client_registration: <error>"
        // and tolerate spaces/hyphens.
        let normalized = trimmed
            .to_ascii_lowercase()
            .replace('-', "_")
            .replace(' ', "_");

        if let Some((prefix, _)) = normalized.split_once(':') {
            return prefix.trim().parse().ok();
        }

        normalized.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_connection_status_round_trips() {
        let values: &[McpConnectionStatusWire] = &[
            McpConnectionStatusWire::Connected,
            McpConnectionStatusWire::Failed,
            McpConnectionStatusWire::NeedsAuth,
            McpConnectionStatusWire::NeedsClientRegistration,
            McpConnectionStatusWire::Disabled,
            McpConnectionStatusWire::Disconnected,
        ];
        for value in values {
            assert_eq!(McpConnectionStatusWire::parse(value.as_str()), Some(*value));
            assert_eq!(value.to_string(), value.as_str());
        }
    }

    #[test]
    fn mcp_connection_status_parses_prefix_error() {
        assert_eq!(
            McpConnectionStatusWire::parse("needs_client_registration: boom"),
            Some(McpConnectionStatusWire::NeedsClientRegistration)
        );
    }
}
