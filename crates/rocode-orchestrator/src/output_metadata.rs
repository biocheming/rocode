use crate::runtime::events::StepUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const CONTINUATION_TARGETS_METADATA_KEY: &str = "continuationTargets";
pub const OUTPUT_USAGE_METADATA_KEY: &str = "usage";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationTarget {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(
        rename = "agentTaskId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_task_id: Option<String>,
    #[serde(rename = "toolName", default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
}

impl OutputUsage {
    pub fn is_zero(&self) -> bool {
        self.prompt_tokens == 0
            && self.completion_tokens == 0
            && self.reasoning_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_write_tokens == 0
    }

    pub fn accumulate(&mut self, other: &OutputUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
    }
}

impl From<StepUsage> for OutputUsage {
    fn from(value: StepUsage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            reasoning_tokens: value.reasoning_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
        }
    }
}

impl From<&StepUsage> for OutputUsage {
    fn from(value: &StepUsage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            reasoning_tokens: value.reasoning_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
        }
    }
}

impl ContinuationTarget {
    fn key(&self) -> (&str, Option<&str>, Option<&str>) {
        (
            self.session_id.as_str(),
            self.agent_task_id.as_deref(),
            self.tool_name.as_deref(),
        )
    }
}

pub fn continuation_target_from_tool_metadata(
    tool_name: &str,
    metadata: Option<&Value>,
) -> Option<ContinuationTarget> {
    let metadata = metadata?;

    #[derive(Debug, Default, Deserialize)]
    struct ContinuationTargetWire {
        #[serde(
            default,
            alias = "sessionId",
            alias = "session_id",
            deserialize_with = "deserialize_opt_string_trimmed"
        )]
        session_id: Option<String>,
        #[serde(
            default,
            alias = "agentTaskId",
            alias = "agent_task_id",
            deserialize_with = "deserialize_opt_string_trimmed"
        )]
        agent_task_id: Option<String>,
    }

    fn deserialize_opt_string_trimmed<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<Value>::deserialize(deserializer)?;
        Ok(match value {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Some(Value::Number(value)) => Some(value.to_string()),
            Some(Value::Bool(value)) => Some(value.to_string()),
            _ => None,
        })
    }

    let wire =
        serde_json::from_value::<ContinuationTargetWire>(metadata.clone()).unwrap_or_default();
    let session_id = wire.session_id?;

    Some(ContinuationTarget {
        session_id,
        agent_task_id: wire.agent_task_id,
        tool_name: Some(tool_name.to_string()),
    })
}

pub fn continuation_targets(metadata: &HashMap<String, Value>) -> Vec<ContinuationTarget> {
    metadata
        .get(CONTINUATION_TARGETS_METADATA_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<ContinuationTarget>>(value).ok())
        .unwrap_or_default()
}

pub fn output_usage(metadata: &HashMap<String, Value>) -> Option<OutputUsage> {
    metadata
        .get(OUTPUT_USAGE_METADATA_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<OutputUsage>(value).ok())
}

pub fn append_output_usage(metadata: &mut HashMap<String, Value>, usage: &OutputUsage) {
    let mut merged = output_usage(metadata).unwrap_or_default();
    merged.accumulate(usage);
    if merged.is_zero() {
        metadata.remove(OUTPUT_USAGE_METADATA_KEY);
    } else if let Ok(value) = serde_json::to_value(merged) {
        metadata.insert(OUTPUT_USAGE_METADATA_KEY.to_string(), value);
    }
}

pub fn append_continuation_target(
    metadata: &mut HashMap<String, Value>,
    target: ContinuationTarget,
) {
    let mut merged = continuation_targets(metadata);
    if !merged.iter().any(|existing| existing.key() == target.key()) {
        merged.push(target);
    }
    if let Ok(value) = serde_json::to_value(merged) {
        metadata.insert(CONTINUATION_TARGETS_METADATA_KEY.to_string(), value);
    }
}

pub fn merge_output_metadata(target: &mut HashMap<String, Value>, source: &HashMap<String, Value>) {
    for continuation in continuation_targets(source) {
        append_continuation_target(target, continuation);
    }
    if let Some(usage) = output_usage(source) {
        append_output_usage(target, &usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_output_metadata_accumulates_usage() {
        let mut target = HashMap::new();
        append_output_usage(
            &mut target,
            &OutputUsage {
                prompt_tokens: 10,
                completion_tokens: 4,
                reasoning_tokens: 2,
                cache_read_tokens: 1,
                cache_write_tokens: 0,
            },
        );

        let mut source = HashMap::new();
        append_output_usage(
            &mut source,
            &OutputUsage {
                prompt_tokens: 7,
                completion_tokens: 3,
                reasoning_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 5,
            },
        );

        merge_output_metadata(&mut target, &source);
        let usage = output_usage(&target).expect("usage should exist");
        assert_eq!(
            usage,
            OutputUsage {
                prompt_tokens: 17,
                completion_tokens: 7,
                reasoning_tokens: 3,
                cache_read_tokens: 1,
                cache_write_tokens: 5,
            }
        );
    }
}
