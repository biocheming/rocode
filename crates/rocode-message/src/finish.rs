use serde::{Deserialize, Serialize};

/// Normalized finish reason for assistant messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Error,
    Unknown,
    /// Non-standard provider reason.
    Custom(String),
}

impl FinishReason {
    fn normalize_style(value: &str) -> String {
        let trimmed = value.trim();
        let mut out = String::with_capacity(trimmed.len() + 2);
        for (idx, ch) in trimmed.chars().enumerate() {
            if ch.is_ascii_uppercase() {
                if idx > 0 {
                    out.push('_');
                }
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push(ch.to_ascii_lowercase());
            }
        }
        out
    }

    /// Parse and normalize known aliases.
    pub fn parse(value: impl AsRef<str>) -> Self {
        let raw = value.as_ref().trim();
        if raw.is_empty() {
            return Self::Unknown;
        }

        if raw.eq_ignore_ascii_case("tool-calls") {
            return Self::ToolCalls;
        }

        let normalized = Self::normalize_style(raw);
        match normalized.as_str() {
            "stop" => Self::Stop,
            "tool_calls" => Self::ToolCalls,
            "length" => Self::Length,
            "content_filter" => Self::ContentFilter,
            "error" => Self::Error,
            "unknown" => Self::Unknown,
            _ => Self::Custom(raw.to_string()),
        }
    }

    /// Canonical string representation.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stop => "stop",
            Self::ToolCalls => "tool-calls",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::Error => "error",
            Self::Unknown => "unknown",
            Self::Custom(value) => value.as_str(),
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Custom(_))
    }
}

impl std::fmt::Display for FinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for FinishReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FinishReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::parse(raw))
    }
}

/// Normalize a free-form finish reason into canonical wire text.
pub fn normalize_finish_reason(value: impl AsRef<str>) -> String {
    FinishReason::parse(value).as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_style_variants() {
        assert_eq!(FinishReason::parse("tool-calls"), FinishReason::ToolCalls);
        assert_eq!(FinishReason::parse("toolCalls"), FinishReason::ToolCalls);
        assert_eq!(
            FinishReason::parse("contentFilter"),
            FinishReason::ContentFilter
        );
        assert_eq!(normalize_finish_reason("toolCalls"), "tool-calls");
    }

    #[test]
    fn keeps_custom_reason() {
        assert_eq!(
            FinishReason::parse("provider_specific").as_str(),
            "provider_specific"
        );
        assert_eq!(FinishReason::parse("end_turn").as_str(), "end_turn");
    }
}
