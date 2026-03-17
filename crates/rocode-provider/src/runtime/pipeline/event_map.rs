use crate::driver::StreamingEvent;
use crate::protocol_loader::{ProtocolManifest, StreamingConfig};
use serde::Deserialize;

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

        #[derive(Debug, Deserialize, Default)]
        struct OpenAiFrame {
            #[serde(default)]
            choices: Vec<OpenAiChoice>,
        }

        #[derive(Debug, Deserialize, Default)]
        struct OpenAiChoice {
            #[serde(default)]
            delta: Option<OpenAiDelta>,
            #[serde(default)]
            finish_reason: Option<String>,
        }

        #[derive(Debug, Deserialize, Default)]
        struct OpenAiDelta {
            #[serde(default)]
            reasoning_content: Option<String>,
            #[serde(default, rename = "reasoning_text")]
            reasoning_text: Option<String>,
        }

        let parsed: OpenAiFrame = serde_json::from_value(frame.clone()).unwrap_or_default();

        // OpenAI-compatible reasoning: reasoning_content or reasoning_text in delta
        let reasoning = parsed
            .choices
            .get(0)
            .and_then(|choice| choice.delta.as_ref())
            .and_then(|delta| {
                delta
                    .reasoning_content
                    .as_deref()
                    .or(delta.reasoning_text.as_deref())
            })
            .unwrap_or("");
        if !reasoning.is_empty() {
            events.push(StreamingEvent::ThinkingDelta {
                thinking: reasoning.to_string(),
                tool_consideration: None,
            });
        }

        if let Some(content) = resolve_path(frame, &self.content_path).and_then(|v| v.as_str()) {
            if !content.is_empty() {
                events.push(StreamingEvent::PartialContentDelta {
                    content: content.to_string(),
                    sequence_id: None,
                });
            }
        }

        if let Some(tool_calls_value) = resolve_path(frame, &self.tool_call_path).cloned() {
            #[derive(Debug, Deserialize, Default)]
            struct OpenAiToolCall {
                #[serde(default)]
                index: Option<u32>,
                #[serde(default)]
                id: Option<String>,
                #[serde(default)]
                function: Option<OpenAiToolCallFunction>,
            }

            #[derive(Debug, Deserialize, Default)]
            struct OpenAiToolCallFunction {
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                arguments: Option<String>,
            }

            let tool_calls: Vec<OpenAiToolCall> =
                serde_json::from_value(tool_calls_value).unwrap_or_default();
            for tool_call in tool_calls {
                let index = tool_call.index;
                let tool_call_id = tool_call
                    .id
                    .unwrap_or_else(|| format!("tool-call-{}", index.unwrap_or(0)));

                if let Some(name) = tool_call
                    .function
                    .as_ref()
                    .and_then(|function| function.name.as_deref())
                {
                    if !name.is_empty() {
                        events.push(StreamingEvent::ToolCallStarted {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: name.to_string(),
                            index,
                        });
                    }
                }

                if let Some(arguments) = tool_call
                    .function
                    .as_ref()
                    .and_then(|function| function.arguments.as_deref())
                {
                    if !arguments.is_empty() {
                        events.push(StreamingEvent::PartialToolCall {
                            tool_call_id: tool_call_id.clone(),
                            arguments: arguments.to_string(),
                            index,
                            is_complete: None,
                        });
                    }
                }
            }
        }

        let usage = resolve_path(frame, &self.usage_path).cloned();
        let finish_reason = parsed
            .choices
            .get(0)
            .and_then(|choice| choice.finish_reason.clone());

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
        #[derive(Debug, Deserialize, Default)]
        struct AnthropicContentBlock {
            #[serde(default, rename = "type")]
            block_type: Option<String>,
            #[serde(default)]
            id: Option<String>,
            #[serde(default)]
            name: Option<String>,
        }

        #[derive(Debug, Deserialize, Default)]
        struct AnthropicDelta {
            #[serde(default)]
            thinking: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            partial_json: Option<String>,
        }

        #[derive(Debug, Deserialize, Default)]
        struct AnthropicMessageDelta {
            #[serde(default)]
            stop_reason: Option<String>,
        }

        #[derive(Debug, Deserialize)]
        #[serde(tag = "type")]
        enum AnthropicFrame {
            #[serde(rename = "content_block_start")]
            ContentBlockStart {
                #[serde(default)]
                index: Option<u32>,
                #[serde(default)]
                content_block: Option<AnthropicContentBlock>,
            },
            #[serde(rename = "content_block_delta")]
            ContentBlockDelta {
                #[serde(default)]
                index: Option<u32>,
                #[serde(default)]
                content_block: Option<AnthropicContentBlock>,
                #[serde(default)]
                delta: Option<AnthropicDelta>,
            },
            #[serde(rename = "message_start")]
            MessageStart,
            #[serde(rename = "message_delta")]
            MessageDelta {
                #[serde(default)]
                delta: Option<AnthropicMessageDelta>,
            },
            #[serde(rename = "message_stop")]
            MessageStop,
            #[serde(rename = "error")]
            Error {
                #[serde(default)]
                error: Option<serde_json::Value>,
            },
            #[serde(other)]
            Other,
        }

        let parsed: AnthropicFrame =
            serde_json::from_value(frame.clone()).unwrap_or(AnthropicFrame::Other);

        match parsed {
            AnthropicFrame::ContentBlockStart {
                index,
                content_block,
            } => {
                let block_type = content_block
                    .as_ref()
                    .and_then(|block| block.block_type.as_deref())
                    .unwrap_or_default();
                if block_type == "thinking" {
                    events.push(StreamingEvent::ThinkingDelta {
                        thinking: String::new(),
                        tool_consideration: None,
                    });
                }
                if block_type == "tool_use" {
                    let tool_call_id = content_block
                        .as_ref()
                        .and_then(|block| block.id.clone())
                        .unwrap_or_else(|| format!("tool-call-{}", index.unwrap_or(0)));
                    let tool_name = content_block
                        .as_ref()
                        .and_then(|block| block.name.as_deref())
                        .unwrap_or_default()
                        .to_string();
                    events.push(StreamingEvent::ToolCallStarted {
                        tool_call_id,
                        tool_name,
                        index,
                    });
                }
            }
            AnthropicFrame::ContentBlockDelta {
                index,
                content_block,
                delta,
            } => {
                if let Some(thinking) = delta.as_ref().and_then(|delta| delta.thinking.as_deref()) {
                    if !thinking.is_empty() {
                        events.push(StreamingEvent::ThinkingDelta {
                            thinking: thinking.to_string(),
                            tool_consideration: None,
                        });
                    }
                }

                if let Some(text) = delta.as_ref().and_then(|delta| delta.text.as_deref()) {
                    if !text.is_empty() {
                        events.push(StreamingEvent::PartialContentDelta {
                            content: text.to_string(),
                            sequence_id: None,
                        });
                    }
                }

                if let Some(partial_json) = delta
                    .as_ref()
                    .and_then(|delta| delta.partial_json.as_deref())
                {
                    if !partial_json.is_empty() {
                        let tool_call_id = content_block
                            .as_ref()
                            .and_then(|block| block.id.clone())
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
            AnthropicFrame::MessageStart => {
                let usage = resolve_path(frame, &self.usage_path).cloned();
                if usage.is_some() {
                    events.push(StreamingEvent::Metadata {
                        usage,
                        finish_reason: None,
                        stop_reason: None,
                    });
                }
            }
            AnthropicFrame::MessageDelta { delta } => {
                let stop_reason = delta.and_then(|delta| delta.stop_reason);
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
            AnthropicFrame::MessageStop => {
                events.push(StreamingEvent::StreamEnd {
                    finish_reason: None,
                });
            }
            AnthropicFrame::Error { error } => {
                if let Some(error) = error {
                    events.push(StreamingEvent::StreamError {
                        error,
                        event_id: None,
                    });
                }
            }
            AnthropicFrame::Other => {}
        };

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

        #[derive(Debug, Deserialize, Default)]
        struct GeminiFrame {
            #[serde(default)]
            usage_metadata: Option<serde_json::Value>,
            #[serde(default)]
            candidates: Vec<GeminiCandidate>,
        }

        #[derive(Debug, Deserialize, Default)]
        struct GeminiCandidate {
            #[serde(default, alias = "finishReason")]
            finish_reason: Option<String>,
        }

        let parsed: GeminiFrame = serde_json::from_value(frame.clone()).unwrap_or_default();
        let usage = resolve_path(frame, &self.usage_path)
            .cloned()
            .or(parsed.usage_metadata);
        let finish_reason = parsed
            .candidates
            .get(0)
            .and_then(|candidate| candidate.finish_reason.clone());

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
