use crate::driver::StreamingEvent;
use crate::protocol_loader::{ProtocolManifest, StreamingConfig};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
struct OpenAiDeltaWire {
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning_text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiToolCallWire {
    #[serde(default)]
    index: Option<u64>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiFunctionWire>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiFunctionWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicEventWire {
    #[serde(rename = "type", default)]
    event_type: String,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    delta: Option<AnthropicDeltaWire>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlockWire>,
    #[serde(default)]
    message: Option<AnthropicMessageWire>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicDeltaWire {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicContentBlockWire {
    #[serde(rename = "type", default)]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicMessageWire {
    #[serde(default)]
    usage: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiLikeFrameWire {
    #[serde(default)]
    candidates: Vec<GeminiLikeCandidateWire>,
}

#[derive(Debug, Default, Deserialize)]
struct GeminiLikeCandidateWire {
    #[serde(default, alias = "finishReason")]
    finish_reason: Option<String>,
}

fn tool_arguments_delta(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(raw)
            }
        }
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(map).to_string())
            }
        }
        serde_json::Value::Array(values) => {
            if values.is_empty() {
                None
            } else {
                Some(serde_json::Value::Array(values).to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapperKind {
    OpenAi,
    Anthropic,
    Google,
    Vertex,
}

/// Path-driven event mapper for runtime pipeline output.
#[derive(Debug, Clone)]
pub struct PathEventMapper {
    kind: MapperKind,
    content_path: String,
    tool_call_path: String,
    usage_path: String,
}

impl PathEventMapper {
    pub fn openai_defaults() -> Self {
        Self {
            kind: MapperKind::OpenAi,
            content_path: "$.choices[0].delta.content".to_string(),
            tool_call_path: "$.choices[0].delta.tool_calls".to_string(),
            usage_path: "$.usage".to_string(),
        }
    }

    pub fn anthropic_defaults() -> Self {
        Self {
            kind: MapperKind::Anthropic,
            content_path: "$.delta.text".to_string(),
            tool_call_path: "$.delta.partial_json".to_string(),
            usage_path: "$.message.usage".to_string(),
        }
    }

    pub fn google_defaults() -> Self {
        Self {
            kind: MapperKind::Google,
            content_path: "$.candidates[0].content.parts[0].text".to_string(),
            tool_call_path: "$.candidates[0].content.parts[0].functionCall".to_string(),
            usage_path: "$.usageMetadata".to_string(),
        }
    }

    pub fn vertex_defaults() -> Self {
        Self {
            kind: MapperKind::Vertex,
            content_path: "$.candidates[0].content.parts[0].text".to_string(),
            tool_call_path: "$.candidates[0].content.parts[0].functionCall".to_string(),
            usage_path: "$.usageMetadata".to_string(),
        }
    }

    pub fn from_manifest(manifest: &ProtocolManifest) -> Self {
        let id = manifest.id.to_ascii_lowercase();
        let mut mapper = if id.contains("anthropic") {
            Self::anthropic_defaults()
        } else if id.contains("google-vertex") || id.contains("vertex") {
            Self::vertex_defaults()
        } else if id.contains("google") || id.contains("gemini") {
            Self::google_defaults()
        } else {
            Self::openai_defaults()
        };

        if let Some(streaming) = &manifest.streaming {
            mapper.apply_streaming_config(streaming);
        }

        mapper
    }

    pub fn from_streaming_config(kind: &str, streaming: &StreamingConfig) -> Self {
        let mut mapper = match kind {
            "anthropic" => Self::anthropic_defaults(),
            "google" => Self::google_defaults(),
            "vertex" => Self::vertex_defaults(),
            _ => Self::openai_defaults(),
        };
        mapper.apply_streaming_config(streaming);
        mapper
    }

    fn apply_streaming_config(&mut self, cfg: &StreamingConfig) {
        if let Some(path) = &cfg.content_path {
            self.content_path = path.clone();
        }
        if let Some(path) = &cfg.tool_call_path {
            self.tool_call_path = path.clone();
        }
        if let Some(path) = &cfg.usage_path {
            self.usage_path = path.clone();
        }
    }

    pub fn map_frame(&self, frame: &serde_json::Value) -> Vec<StreamingEvent> {
        match self.kind {
            MapperKind::OpenAi => self.map_openai(frame),
            MapperKind::Anthropic => self.map_anthropic(frame),
            MapperKind::Google | MapperKind::Vertex => self.map_gemini_like(frame),
        }
    }

    fn map_openai(&self, frame: &serde_json::Value) -> Vec<StreamingEvent> {
        let mut events = Vec::new();

        // OpenAI-compatible reasoning: reasoning_content or reasoning_text in delta
        if let Some(delta_value) = resolve_path(frame, "$.choices[0].delta") {
            let delta = serde_json::from_value::<OpenAiDeltaWire>(delta_value.clone())
                .ok()
                .unwrap_or_default();
            let reasoning = delta
                .reasoning_content
                .or(delta.reasoning_text)
                .unwrap_or_default();
            if !reasoning.is_empty() {
                events.push(StreamingEvent::ThinkingDelta {
                    thinking: reasoning,
                    tool_consideration: None,
                });
            }
        }

        if let Some(content) = resolve_path(frame, &self.content_path).and_then(|v| v.as_str()) {
            if !content.is_empty() {
                events.push(StreamingEvent::PartialContentDelta {
                    content: content.to_string(),
                    sequence_id: None,
                });
            }
        }

        if let Some(tool_calls) =
            resolve_path(frame, &self.tool_call_path).and_then(|v| v.as_array())
        {
            for tool_call in tool_calls {
                let tool_call = serde_json::from_value::<OpenAiToolCallWire>(tool_call.clone())
                    .ok()
                    .unwrap_or_default();

                let index = tool_call.index.map(|i| i as u32);
                let tool_call_id = tool_call
                    .id
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(|| format!("tool-call-{}", index.unwrap_or(0)));

                if let Some(function) = tool_call.function {
                    if let Some(name) = function.name.filter(|name| !name.trim().is_empty()) {
                        events.push(StreamingEvent::ToolCallStarted {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: name,
                            index,
                        });
                    }

                    if let Some(arguments) = function
                        .arguments
                        .and_then(tool_arguments_delta)
                        .filter(|args| {
                            // Avoid emitting a delta for the empty arguments sentinel.
                            args.trim() != "{}"
                        })
                    {
                        events.push(StreamingEvent::PartialToolCall {
                            tool_call_id: tool_call_id.clone(),
                            arguments,
                            index,
                            is_complete: None,
                        });
                    }
                }
            }
        }

        let usage = resolve_path(frame, &self.usage_path).cloned();
        let finish_reason = resolve_path(frame, "$.choices[0].finish_reason")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);

        if usage.is_some() || finish_reason.is_some() {
            events.push(StreamingEvent::Metadata {
                usage,
                finish_reason: finish_reason.clone(),
                stop_reason: None,
            });
        }

        if finish_reason.is_some() {
            events.push(StreamingEvent::StreamEnd { finish_reason });
        }

        events
    }

    fn map_anthropic(&self, frame: &serde_json::Value) -> Vec<StreamingEvent> {
        let mut events = Vec::new();
        let wire = serde_json::from_value::<AnthropicEventWire>(frame.clone())
            .ok()
            .unwrap_or_default();
        let event_type = wire.event_type.as_str();

        match event_type {
            "content_block_start" => {
                let Some(block) = wire.content_block else {
                    return events;
                };

                if block.block_type == "thinking" {
                    events.push(StreamingEvent::ThinkingDelta {
                        thinking: String::new(),
                        tool_consideration: None,
                    });
                }
                if block.block_type == "tool_use" {
                    let index = wire.index;
                    let tool_call_id = block
                        .id
                        .filter(|id| !id.trim().is_empty())
                        .unwrap_or_else(|| format!("tool-call-{}", index.unwrap_or(0)));
                    let tool_name = block.name.unwrap_or_default();
                    events.push(StreamingEvent::ToolCallStarted {
                        tool_call_id,
                        tool_name,
                        index,
                    });
                }
            }
            "content_block_delta" => {
                if let Some(delta) = wire.delta.as_ref() {
                    if let Some(thinking) = delta.thinking.as_deref().filter(|t| !t.is_empty()) {
                        events.push(StreamingEvent::ThinkingDelta {
                            thinking: thinking.to_string(),
                            tool_consideration: None,
                        });
                    }
                    if let Some(text) = delta.text.as_deref().filter(|t| !t.is_empty()) {
                        events.push(StreamingEvent::PartialContentDelta {
                            content: text.to_string(),
                            sequence_id: None,
                        });
                    }
                    if let Some(partial_json) =
                        delta.partial_json.as_deref().filter(|t| !t.is_empty())
                    {
                        let index = wire.index;
                        let tool_call_id = wire
                            .content_block
                            .as_ref()
                            .and_then(|block| block.id.as_deref())
                            .filter(|id| !id.trim().is_empty())
                            .map(ToString::to_string)
                            .unwrap_or_else(|| format!("tool-call-{}", index.unwrap_or(0)));
                        events.push(StreamingEvent::PartialToolCall {
                            tool_call_id,
                            arguments: partial_json.to_string(),
                            index,
                            is_complete: None,
                        });
                    }
                }
            }
            "message_start" => {
                let usage = resolve_path(frame, &self.usage_path)
                    .cloned()
                    .or_else(|| wire.message.as_ref().and_then(|m| m.usage.clone()));
                if usage.is_some() {
                    events.push(StreamingEvent::Metadata {
                        usage,
                        finish_reason: None,
                        stop_reason: None,
                    });
                }
            }
            "message_delta" => {
                let stop_reason = wire
                    .delta
                    .as_ref()
                    .and_then(|delta| delta.stop_reason.clone())
                    .filter(|reason| !reason.trim().is_empty());
                if stop_reason.is_some() {
                    events.push(StreamingEvent::Metadata {
                        usage: None,
                        finish_reason: None,
                        stop_reason: stop_reason.clone(),
                    });
                    events.push(StreamingEvent::StreamEnd {
                        finish_reason: stop_reason,
                    });
                }
            }
            "message_stop" => {
                events.push(StreamingEvent::StreamEnd {
                    finish_reason: None,
                });
            }
            "error" => {
                if let Some(error) = wire.error {
                    events.push(StreamingEvent::StreamError {
                        error,
                        event_id: None,
                    });
                }
            }
            _ => {}
        }

        events
    }

    fn map_gemini_like(&self, frame: &serde_json::Value) -> Vec<StreamingEvent> {
        let mut events = Vec::new();

        if let Some(text) = resolve_path(frame, &self.content_path).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                events.push(StreamingEvent::PartialContentDelta {
                    content: text.to_string(),
                    sequence_id: None,
                });
            }
        }

        let usage = resolve_path(frame, &self.usage_path)
            .cloned()
            .or_else(|| resolve_path(frame, "$.usage_metadata").cloned());
        let finish_reason = serde_json::from_value::<GeminiLikeFrameWire>(frame.clone())
            .ok()
            .and_then(|wire| {
                wire.candidates
                    .into_iter()
                    .next()
                    .and_then(|candidate| candidate.finish_reason)
            });

        if usage.is_some() || finish_reason.is_some() {
            events.push(StreamingEvent::Metadata {
                usage,
                finish_reason: finish_reason.clone(),
                stop_reason: None,
            });
        }

        if finish_reason.is_some() {
            events.push(StreamingEvent::StreamEnd { finish_reason });
        }

        events
    }
}

#[derive(Debug)]
enum PathPart {
    Key(String),
    Index(usize),
}

fn resolve_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts = parse_path(path)?;
    let mut current = value;
    for part in parts {
        match part {
            PathPart::Key(key) => {
                current = current.get(&key)?;
            }
            PathPart::Index(index) => {
                current = current.as_array()?.get(index)?;
            }
        }
    }
    Some(current)
}

fn parse_path(path: &str) -> Option<Vec<PathPart>> {
    let trimmed = path.trim();
    if trimmed == "$" {
        return Some(Vec::new());
    }
    let rest = trimmed.strip_prefix("$.")?;
    let mut parts = Vec::new();

    for segment in rest.split('.') {
        if segment.is_empty() {
            return None;
        }

        let mut cursor = segment;
        if let Some(start) = cursor.find('[') {
            let key = &cursor[..start];
            if !key.is_empty() {
                parts.push(PathPart::Key(key.to_string()));
            }
            cursor = &cursor[start..];
        } else {
            parts.push(PathPart::Key(cursor.to_string()));
            continue;
        }

        while let Some(stripped) = cursor.strip_prefix('[') {
            let end = stripped.find(']')?;
            let index = stripped[..end].parse::<usize>().ok()?;
            parts.push(PathPart::Index(index));
            cursor = &stripped[end + 1..];
        }

        if !cursor.is_empty() {
            return None;
        }
    }

    Some(parts)
}
