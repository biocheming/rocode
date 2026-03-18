use strum_macros::EnumString;

/// Shared auth payload keys used by server/provider/CLI auth flows.
pub mod auth_keys {
    pub const TYPE: &str = "type";
    pub const PROVIDER: &str = "provider";
    pub const SUCCESS: &str = "success";

    pub const API_KEY_SNAKE: &str = "api_key";
    pub const API_KEY_CAMEL: &str = "apiKey";
    pub const TOKEN: &str = "token";
    pub const KEY: &str = "key";
    pub const ACCESS: &str = "access";
    pub const REFRESH: &str = "refresh";
    pub const EXPIRES: &str = "expires";
    pub const ACCOUNT_ID: &str = "accountId";
    pub const ENTERPRISE_URL: &str = "enterpriseUrl";
}

/// Provider option-map keys (config/bootstrap/runtime state).
pub mod option_keys {
    pub const API_KEY: &str = "apiKey";
    pub const API_KEY_SNAKE: &str = "api_key";
    pub const API_KEY_LOWER: &str = "apikey";
    pub const BASE_URL: &str = "baseURL";
    pub const BASE_URL_CAMEL: &str = "baseUrl";
    pub const URL: &str = "url";
    pub const API: &str = "api";
    pub const ACCOUNT_ID: &str = "accountId";
}

/// Reusable tolerant-reader key sets for provider options.
pub mod option_keysets {
    use super::option_keys;

    pub const API_KEY_ANY: &[&str] = &[
        option_keys::API_KEY,
        option_keys::API_KEY_SNAKE,
        option_keys::API_KEY_LOWER,
    ];

    pub const BASE_URL_ANY: &[&str] = &[
        option_keys::BASE_URL,
        option_keys::BASE_URL_CAMEL,
        option_keys::URL,
        option_keys::API,
    ];
}

/// Canonical provider finish reason strings (wire format).
///
/// These are **normalized** values surfaced across session/runtime layers, and
/// are intentionally stable because they are stored in message metadata and
/// used by multiple frontends.
///
/// Canonical values:
/// - `"stop"`
/// - `"tool-calls"`
/// - `"length"`
/// - `"content_filter"`
/// - `"error"`
/// - `"unknown"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum ProviderFinishReasonWire {
    #[strum(serialize = "stop", serialize = "end_turn")]
    Stop,
    #[strum(
        serialize = "tool-calls",
        serialize = "tool_calls",
        serialize = "tool_use"
    )]
    ToolCalls,
    #[strum(serialize = "length")]
    Length,
    #[strum(serialize = "content_filter", serialize = "content-filter")]
    ContentFilter,
    #[strum(serialize = "error")]
    Error,
    #[strum(serialize = "unknown")]
    Unknown,
}

impl std::fmt::Display for ProviderFinishReasonWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ProviderFinishReasonWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::ToolCalls => "tool-calls",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

/// Canonical "tool_name" strings used for **provider-managed** tool calls.
///
/// These are used for providers that surface internal tool calls as streaming
/// events (e.g. OpenAI Responses API output items like web search / code interpreter).
///
/// Keep these stable — they are part of the runtime wire contract between
/// `rocode-provider`, `rocode-orchestrator`, and UI layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum ProviderToolCallNameWire {
    #[strum(serialize = "web_search_call", serialize = "web-search-call")]
    WebSearchCall,
    #[strum(
        serialize = "code_interpreter_call",
        serialize = "code-interpreter-call"
    )]
    CodeInterpreterCall,
    #[strum(serialize = "file_search_call", serialize = "file-search-call")]
    FileSearchCall,
    #[strum(
        serialize = "image_generation_call",
        serialize = "image-generation-call"
    )]
    ImageGenerationCall,
    #[strum(serialize = "computer_call", serialize = "computer-call")]
    ComputerCall,
    #[strum(
        serialize = "local_shell",
        serialize = "local-shell",
        serialize = "local_shell_call",
        serialize = "local-shell-call"
    )]
    LocalShell,
}

impl std::fmt::Display for ProviderToolCallNameWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ProviderToolCallNameWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WebSearchCall => "web_search_call",
            Self::CodeInterpreterCall => "code_interpreter_call",
            Self::FileSearchCall => "file_search_call",
            Self::ImageGenerationCall => "image_generation_call",
            Self::ComputerCall => "computer_call",
            Self::LocalShell => "local_shell",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_finish_reason_is_canonical() {
        assert_eq!(ProviderFinishReasonWire::Stop.as_str(), "stop");
        assert_eq!(ProviderFinishReasonWire::ToolCalls.as_str(), "tool-calls");
        assert_eq!(ProviderFinishReasonWire::Unknown.as_str(), "unknown");
    }

    #[test]
    fn provider_finish_reason_parses_aliases() {
        assert_eq!(
            ProviderFinishReasonWire::parse("end_turn"),
            Some(ProviderFinishReasonWire::Stop)
        );
        assert_eq!(
            ProviderFinishReasonWire::parse("tool_calls"),
            Some(ProviderFinishReasonWire::ToolCalls)
        );
        assert_eq!(
            ProviderFinishReasonWire::parse("tool_use"),
            Some(ProviderFinishReasonWire::ToolCalls)
        );
    }

    #[test]
    fn provider_tool_call_names_round_trip() {
        let cases: &[ProviderToolCallNameWire] = &[
            ProviderToolCallNameWire::WebSearchCall,
            ProviderToolCallNameWire::CodeInterpreterCall,
            ProviderToolCallNameWire::FileSearchCall,
            ProviderToolCallNameWire::ImageGenerationCall,
            ProviderToolCallNameWire::ComputerCall,
            ProviderToolCallNameWire::LocalShell,
        ];
        for value in cases {
            assert_eq!(
                ProviderToolCallNameWire::parse(value.as_str()),
                Some(*value)
            );
            assert_eq!(value.to_string(), value.as_str());
        }
    }
}
