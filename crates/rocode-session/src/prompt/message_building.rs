// Message building/conversion/compaction methods for SessionPrompt

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rocode_provider::{
    get_model_context_limit, ChatResponse, Content, ContentPart, Message, Provider,
};
use serde::Deserialize;

use crate::compaction::{
    CompactionConfig, CompactionEngine, MessageForPrune, ModelLimits, PruneToolPart, TokenUsage,
    ToolPartStatus,
};
use crate::message_v2::{
    AssistantTime, AssistantTokens, CacheTokens, CompactionPart as V2CompactionPart, MessageInfo,
    MessagePath, MessageWithParts, ModelRef as V2ModelRef, Part as V2Part, StepFinishPart,
    StepStartPart, StepTokens, UserTime,
};
use crate::summary::{summarize_into_session, SummarizeInput};
use crate::{PartType, Role, Session, SessionMessage};
use rocode_message::ToolState as MessageToolState;

use super::tools_and_output::{compose_session_title_source, generate_session_title_for_session};
use super::SessionPrompt;

type LegacyToolResult = (
    String,
    bool,
    Option<String>,
    Option<HashMap<String, serde_json::Value>>,
    Option<Vec<serde_json::Value>>,
);

type LegacyToolResultMap = HashMap<String, LegacyToolResult>;

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        _ => None,
    })
}

fn deserialize_opt_u64_lossy<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(value)) => value.as_u64(),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn deserialize_opt_f64_lossy<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Number(value)) => value.as_f64(),
        Some(serde_json::Value::String(raw)) => raw.trim().parse::<f64>().ok(),
        _ => None,
    })
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LegacyUsageWire {
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    prompt_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    completion_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    cache_read_tokens: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    cache_write_tokens: Option<u64>,
}

fn deserialize_opt_legacy_usage_lossy<'de, D>(
    deserializer: D,
) -> Result<Option<LegacyUsageWire>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    Ok(serde_json::from_value::<LegacyUsageWire>(value).ok())
}

#[derive(Debug, Deserialize, Default)]
struct MessageMetadataWire {
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    step_start_snapshot: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    snapshot: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    step_finish_snapshot: Option<String>,

    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    tokens_input: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    tokens_output: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    tokens_cache_read: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
    tokens_cache_write: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_opt_legacy_usage_lossy")]
    usage: Option<LegacyUsageWire>,

    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    finish_reason: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_lossy")]
    cost: Option<f64>,

    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    agent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_provider: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    model_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    variant: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
    mode: Option<String>,
}

fn parse_message_metadata(metadata: &HashMap<String, serde_json::Value>) -> MessageMetadataWire {
    let map: serde_json::Map<String, serde_json::Value> = metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    serde_json::from_value::<MessageMetadataWire>(serde_json::Value::Object(map))
        .unwrap_or_default()
}

fn clamp_u64_to_i32(value: Option<u64>) -> i32 {
    value
        .unwrap_or(0)
        .min(i32::MAX as u64)
        .clamp(0, i32::MAX as u64) as i32
}

struct LegacyToolStateInput<'a> {
    tool_call_id: &'a str,
    tool_name: &'a str,
    input: &'a serde_json::Value,
    status: &'a crate::ToolCallStatus,
    raw: &'a str,
    tool_result: Option<&'a LegacyToolResult>,
    session_id: &'a str,
    message_id: &'a str,
}

impl SessionPrompt {
    pub(super) fn build_chat_messages(
        session_messages: &[SessionMessage],
        system_prompt: Option<&str>,
    ) -> anyhow::Result<Vec<Message>> {
        let mut messages = Vec::new();

        if let Some(system) = system_prompt {
            messages.push(Message::system(system));
        }

        for msg in session_messages {
            // Skip messages with no parts — empty Tool/Assistant messages
            // confuse providers (especially Anthropic which rejects empty content).
            if msg.parts.is_empty() {
                continue;
            }

            if matches!(msg.role, Role::Assistant)
                && msg
                    .parts
                    .iter()
                    .any(|p| matches!(p.part_type, PartType::ToolResult { .. }))
            {
                // Backward-compat: old sessions may carry tool_result parts on
                // assistant messages. Split those into a synthetic tool-role
                // message to preserve provider role expectations.
                let mut assistant_parts = Vec::new();
                let mut tool_parts = Vec::new();
                for part in &msg.parts {
                    if matches!(part.part_type, PartType::ToolResult { .. }) {
                        tool_parts.push(part.clone());
                    } else {
                        assistant_parts.push(part.clone());
                    }
                }

                if !assistant_parts.is_empty() {
                    messages.push(Message {
                        role: Role::Assistant,
                        content: Self::parts_to_content(&assistant_parts),
                        cache_control: None,
                        provider_options: None,
                    });
                }
                if !tool_parts.is_empty() {
                    messages.push(Message {
                        role: Role::Tool,
                        content: Self::parts_to_content(&tool_parts),
                        cache_control: None,
                        provider_options: None,
                    });
                }
                continue;
            }

            let content = Self::parts_to_content(&msg.parts);
            let role = msg.role;

            messages.push(Message {
                role,
                content,
                cache_control: None,
                provider_options: None,
            });
        }

        Ok(messages)
    }

    pub(super) fn parts_to_content(parts: &[crate::MessagePart]) -> Content {
        let has_parts = parts
            .iter()
            .any(|p| !matches!(p.part_type, PartType::Text { .. }));

        if !has_parts {
            let text = parts
                .iter()
                .filter_map(|p| match &p.part_type {
                    PartType::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Content::Text(text);
        }

        let content_parts: Vec<ContentPart> = parts
            .iter()
            .filter_map(|p| match &p.part_type {
                PartType::Text { text, .. } => Some(ContentPart {
                    content_type: "text".to_string(),
                    text: Some(text.clone()),
                    image_url: None,
                    tool_use: None,
                    tool_result: None,
                    cache_control: None,
                    filename: None,
                    media_type: None,
                    provider_options: None,
                }),
                PartType::ToolCall {
                    id, name, input, ..
                } => Some(ContentPart {
                    content_type: "tool_use".to_string(),
                    text: None,
                    image_url: None,
                    tool_use: Some(rocode_provider::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }),
                    tool_result: None,
                    cache_control: None,
                    filename: None,
                    media_type: None,
                    provider_options: None,
                }),
                PartType::ToolResult {
                    tool_call_id,
                    content,
                    is_error,
                    ..
                } => Some(ContentPart {
                    content_type: "tool_result".to_string(),
                    text: None,
                    image_url: None,
                    tool_use: None,
                    tool_result: Some(rocode_provider::ToolResult {
                        tool_use_id: tool_call_id.clone(),
                        content: content.clone(),
                        is_error: Some(*is_error),
                    }),
                    cache_control: None,
                    filename: None,
                    media_type: None,
                    provider_options: None,
                }),
                PartType::File {
                    url,
                    filename,
                    mime,
                } => {
                    if mime.starts_with("image/") {
                        Some(ContentPart {
                            content_type: "image_url".to_string(),
                            text: None,
                            image_url: Some(rocode_provider::ImageUrl { url: url.clone() }),
                            tool_use: None,
                            tool_result: None,
                            cache_control: None,
                            filename: Some(filename.clone()),
                            media_type: Some(mime.clone()),
                            provider_options: None,
                        })
                    } else {
                        Some(ContentPart {
                            content_type: "text".to_string(),
                            text: Some(format!("[File: {} ({})]", filename, mime)),
                            image_url: None,
                            tool_use: None,
                            tool_result: None,
                            cache_control: None,
                            filename: Some(filename.clone()),
                            media_type: Some(mime.clone()),
                            provider_options: None,
                        })
                    }
                }
                _ => None,
            })
            .collect();

        Content::Parts(content_parts)
    }

    #[allow(dead_code)]
    pub(super) fn process_response(response: &ChatResponse) -> SessionMessage {
        let now = chrono::Utc::now();

        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or(Content::Text(String::new()));

        let finish_reason = response
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone());

        let parts = match content {
            Content::Text(text) => vec![crate::MessagePart {
                id: format!("prt_{}", uuid::Uuid::new_v4()),
                part_type: PartType::Text {
                    text,
                    synthetic: None,
                    ignored: None,
                },
                created_at: now,
                message_id: None,
            }],
            Content::Parts(content_parts) => content_parts
                .into_iter()
                .filter_map(|p| match p.content_type.as_str() {
                    "text" => p.text.map(|text| crate::MessagePart {
                        id: format!("prt_{}", uuid::Uuid::new_v4()),
                        part_type: PartType::Text {
                            text,
                            synthetic: None,
                            ignored: None,
                        },
                        created_at: now,
                        message_id: None,
                    }),
                    "tool_use" => p.tool_use.map(|tu| crate::MessagePart {
                        id: format!("prt_{}", uuid::Uuid::new_v4()),
                        part_type: PartType::ToolCall {
                            id: tu.id,
                            name: tu.name,
                            input: tu.input,
                            status: crate::ToolCallStatus::Running,
                            raw: None,
                            state: None,
                        },
                        created_at: now,
                        message_id: None,
                    }),
                    _ => None,
                })
                .collect(),
        };

        SessionMessage {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            session_id: String::new(),
            role: Role::Assistant,
            parts,
            created_at: now,
            metadata: {
                let mut m = HashMap::new();
                if let Some(usage) = &response.usage {
                    m.insert(
                        "tokens_input".to_string(),
                        serde_json::json!(usage.prompt_tokens),
                    );
                    m.insert(
                        "tokens_output".to_string(),
                        serde_json::json!(usage.completion_tokens),
                    );
                }
                if let Some(ref reason) = finish_reason {
                    m.insert("finish_reason".to_string(), serde_json::json!(reason));
                }
                m
            },
            usage: None,
            finish: finish_reason,
        }
    }

    pub(super) fn filter_compacted_messages(messages: &[SessionMessage]) -> Vec<SessionMessage> {
        let start = messages
            .iter()
            .rposition(|m| {
                m.parts
                    .iter()
                    .any(|p| matches!(p.part_type, PartType::Compaction { .. }))
            })
            .unwrap_or(0);
        let tail = messages[start..].to_vec();
        if tail.iter().any(|m| matches!(m.role, Role::User)) {
            return tail;
        }

        // Keep the latest user anchor before the compaction boundary so prompt
        // loop invariants hold (`last_user_idx` must exist).
        if let Some(last_user_idx) = messages.iter().rposition(|m| matches!(m.role, Role::User)) {
            if last_user_idx < start {
                let mut anchored = Vec::with_capacity(messages.len() - last_user_idx);
                anchored.push(messages[last_user_idx].clone());
                anchored.extend_from_slice(&messages[start..]);
                return anchored;
            }
        }

        tail
    }

    pub(super) fn token_usage_from_messages(messages: &[SessionMessage]) -> TokenUsage {
        let mut usage = TokenUsage {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        };

        for msg in messages {
            // Prefer the strongly-typed usage field populated by provider stream/final responses.
            if let Some(msg_usage) = msg.usage.as_ref() {
                usage.input += msg_usage.input_tokens;
                usage.output += msg_usage.output_tokens;
                usage.cache_read += msg_usage.cache_read_tokens;
                usage.cache_write += msg_usage.cache_write_tokens;
                continue;
            }

            // Fallback to metadata for backward compatibility with legacy snapshots.
            let meta = parse_message_metadata(&msg.metadata);
            let legacy_usage = meta.usage.unwrap_or_default();

            usage.input += meta
                .tokens_input
                .or(legacy_usage.prompt_tokens)
                .unwrap_or(0);
            usage.output += meta
                .tokens_output
                .or(legacy_usage.completion_tokens)
                .unwrap_or(0);
            usage.cache_read += meta
                .tokens_cache_read
                .or(legacy_usage.cache_read_tokens)
                .unwrap_or(0);
            usage.cache_write += meta
                .tokens_cache_write
                .or(legacy_usage.cache_write_tokens)
                .unwrap_or(0);
        }
        usage.total = usage.input + usage.output + usage.cache_read + usage.cache_write;
        usage
    }

    pub(super) fn should_compact(
        messages: &[SessionMessage],
        provider: &dyn Provider,
        model_id: &str,
        max_output_tokens: Option<u64>,
    ) -> bool {
        let usage = Self::token_usage_from_messages(messages);
        let model = provider.get_model(model_id);
        let limits = ModelLimits {
            context: model
                .map(|info| info.context_window)
                .unwrap_or_else(|| get_model_context_limit(model_id)),
            max_input: model.and_then(|info| info.max_input_tokens),
            max_output: max_output_tokens
                .or_else(|| model.map(|info| info.max_output_tokens))
                .unwrap_or(8192),
        };
        let engine = CompactionEngine::new(CompactionConfig::default());
        if engine.is_overflow(&usage, &limits) {
            return true;
        }

        // Estimate total content size across ALL part types (not just text).
        // This catches large tool results and tool call inputs that the
        // token-based check misses (it relies on cached API response counts).
        let total_chars: usize = messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .map(|p| match &p.part_type {
                PartType::Text { text, .. } => text.len(),
                PartType::ToolResult { content, title, .. } => {
                    content.len() + title.as_ref().map_or(0, |t| t.len())
                }
                PartType::ToolCall { input, raw, .. } => {
                    let input_len = serde_json::to_string(input).map_or(0, |s| s.len());
                    input_len + raw.as_ref().map_or(0, |r| r.len())
                }
                PartType::Reasoning { text } => text.len(),
                _ => 0,
            })
            .sum();

        // Hard cap: 5MB of content to stay under typical 6MB API body limits
        // (leaves ~1MB for JSON overhead, tool definitions, system prompt).
        const MAX_BODY_CHARS: usize = 5_000_000;
        if total_chars > MAX_BODY_CHARS {
            return true;
        }

        // Softer cap based on estimated token count.
        const MAX_CONTEXT_CHARS: usize = 200_000;
        total_chars > MAX_CONTEXT_CHARS
    }

    pub(super) async fn ensure_title(
        session: &mut Session,
        provider: Arc<dyn Provider>,
        model_id: &str,
    ) {
        let Some((_, fallback)) = compose_session_title_source(session) else {
            return;
        };

        if !session.allows_auto_title_regeneration() && session.title.trim() != fallback.trim() {
            return;
        }

        let title = generate_session_title_for_session(session, provider, model_id).await;
        if !title.trim().is_empty() {
            session.set_title(title);
        }
    }

    pub(super) fn to_message_with_parts(
        messages: &[SessionMessage],
        provider_id: &str,
        model_id: &str,
        session_directory: &str,
    ) -> Vec<MessageWithParts> {
        let legacy_tool_results: LegacyToolResultMap = messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter_map(|part| match &part.part_type {
                PartType::ToolResult {
                    tool_call_id,
                    content,
                    is_error,
                    title,
                    metadata,
                    attachments,
                } => Some((
                    tool_call_id.clone(),
                    (
                        content.clone(),
                        *is_error,
                        title.clone(),
                        metadata.clone(),
                        attachments.clone(),
                    ),
                )),
                _ => None,
            })
            .collect();

        let mut out = Vec::with_capacity(messages.len());
        let mut last_user_id = String::new();

        for msg in messages {
            let created = msg.created_at.timestamp_millis();
            let meta = parse_message_metadata(&msg.metadata);
            let input = clamp_u64_to_i32(meta.tokens_input);
            let output = clamp_u64_to_i32(meta.tokens_output);
            let mut parts: Vec<V2Part> = msg
                .parts
                .iter()
                .filter_map(|part| match &part.part_type {
                    PartType::Text { text, .. } => Some(V2Part::Text {
                        id: part.id.clone(),
                        session_id: msg.session_id.clone(),
                        message_id: msg.id.clone(),
                        text: text.clone(),
                        synthetic: None,
                        ignored: None,
                        time: None,
                        metadata: None,
                    }),
                    PartType::File {
                        url,
                        filename,
                        mime,
                    } => Some(V2Part::File(crate::message_v2::FilePart {
                        id: part.id.clone(),
                        session_id: msg.session_id.clone(),
                        message_id: msg.id.clone(),
                        mime: mime.clone(),
                        url: url.clone(),
                        filename: Some(filename.clone()),
                        source: None,
                    })),
                    PartType::Compaction { .. } => Some(V2Part::Compaction(V2CompactionPart {
                        id: part.id.clone(),
                        session_id: msg.session_id.clone(),
                        message_id: msg.id.clone(),
                        auto: true,
                    })),
                    PartType::ToolCall {
                        id,
                        name,
                        input,
                        status,
                        raw,
                        state,
                    } => {
                        let state = state
                            .as_ref()
                            .map(Self::message_tool_state_to_v2)
                            .unwrap_or_else(|| {
                                Self::legacy_tool_state_to_v2(LegacyToolStateInput {
                                    tool_call_id: id,
                                    tool_name: name,
                                    input,
                                    status,
                                    raw: raw.as_deref().unwrap_or_default(),
                                    tool_result: legacy_tool_results.get(id),
                                    session_id: &msg.session_id,
                                    message_id: &msg.id,
                                })
                            });
                        Some(V2Part::Tool(crate::message_v2::ToolPart {
                            id: part.id.clone(),
                            session_id: msg.session_id.clone(),
                            message_id: msg.id.clone(),
                            call_id: id.clone(),
                            tool: name.clone(),
                            state,
                            metadata: None,
                        }))
                    }
                    _ => None,
                })
                .collect();

            if let Some(snapshot) = meta
                .step_start_snapshot
                .as_deref()
                .or(meta.snapshot.as_deref())
            {
                parts.push(V2Part::StepStart(StepStartPart {
                    id: format!("prt_{}", uuid::Uuid::new_v4()),
                    session_id: msg.session_id.clone(),
                    message_id: msg.id.clone(),
                    snapshot: Some(snapshot.to_string()),
                }));
            }
            if let Some(snapshot) = meta.step_finish_snapshot.as_deref() {
                parts.push(V2Part::StepFinish(StepFinishPart {
                    id: format!("prt_{}", uuid::Uuid::new_v4()),
                    session_id: msg.session_id.clone(),
                    message_id: msg.id.clone(),
                    reason: msg
                        .finish
                        .as_deref()
                        .or(meta.finish_reason.as_deref())
                        .unwrap_or("stop")
                        .to_string(),
                    snapshot: Some(snapshot.to_string()),
                    cost: meta.cost.unwrap_or(0.0),
                    tokens: StepTokens {
                        total: Some(input.saturating_add(output)),
                        input,
                        output,
                        reasoning: 0,
                        cache: CacheTokens { read: 0, write: 0 },
                    },
                }));
            }

            let info = match msg.role {
                Role::User => {
                    last_user_id = msg.id.clone();
                    MessageInfo::User {
                        id: msg.id.clone(),
                        session_id: msg.session_id.clone(),
                        time: UserTime { created },
                        agent: meta.agent.as_deref().unwrap_or("general").to_string(),
                        model: V2ModelRef {
                            provider_id: meta
                                .model_provider
                                .as_deref()
                                .unwrap_or(provider_id)
                                .to_string(),
                            model_id: meta.model_id.as_deref().unwrap_or(model_id).to_string(),
                        },
                        format: None,
                        summary: None,
                        system: None,
                        tools: None,
                        variant: meta.variant.clone(),
                    }
                }
                _ => MessageInfo::Assistant {
                    id: msg.id.clone(),
                    session_id: msg.session_id.clone(),
                    time: AssistantTime {
                        created,
                        completed: Some(created),
                    },
                    parent_id: if last_user_id.is_empty() {
                        msg.id.clone()
                    } else {
                        last_user_id.clone()
                    },
                    model_id: meta.model_id.as_deref().unwrap_or(model_id).to_string(),
                    provider_id: meta
                        .model_provider
                        .as_deref()
                        .unwrap_or(provider_id)
                        .to_string(),
                    mode: meta.mode.as_deref().unwrap_or("default").to_string(),
                    agent: meta.agent.as_deref().unwrap_or("general").to_string(),
                    path: MessagePath {
                        cwd: session_directory.to_string(),
                        root: session_directory.to_string(),
                    },
                    summary: None,
                    cost: meta.cost.unwrap_or(0.0),
                    tokens: AssistantTokens {
                        total: Some(input.saturating_add(output)),
                        input,
                        output,
                        reasoning: 0,
                        cache: CacheTokens { read: 0, write: 0 },
                    },
                    error: None,
                    structured: None,
                    variant: meta.variant.clone(),
                    finish: msg.finish.clone().or_else(|| meta.finish_reason.clone()),
                },
            };

            out.push(MessageWithParts { info, parts });
        }

        out
    }

    fn message_tool_state_to_v2(state: &MessageToolState) -> crate::message_v2::ToolState {
        match state {
            MessageToolState::Pending { input, raw } => crate::message_v2::ToolState::Pending {
                input: input.clone(),
                raw: raw.clone(),
            },
            MessageToolState::Running {
                input,
                title,
                metadata,
                time,
            } => crate::message_v2::ToolState::Running {
                input: input.clone(),
                title: title.clone(),
                metadata: metadata.clone(),
                time: crate::RunningTime { start: time.start },
            },
            MessageToolState::Completed {
                input,
                output,
                title,
                metadata,
                time,
                attachments,
            } => {
                let files = attachments.as_ref().map(|values| {
                    values
                        .iter()
                        .filter_map(|value| {
                            serde_json::from_value::<crate::message_v2::FilePart>(value.clone())
                                .ok()
                        })
                        .collect::<Vec<_>>()
                });
                crate::message_v2::ToolState::Completed {
                    input: input.clone(),
                    output: output.clone(),
                    title: title.clone(),
                    metadata: metadata.clone(),
                    time: crate::CompletedTime {
                        start: time.start,
                        end: time.end,
                        compacted: time.compacted,
                    },
                    attachments: files,
                }
            }
            MessageToolState::Error {
                input,
                error,
                metadata,
                time,
            } => crate::message_v2::ToolState::Error {
                input: input.clone(),
                error: error.clone(),
                metadata: metadata.clone(),
                time: crate::ErrorTime {
                    start: time.start,
                    end: time.end,
                },
            },
        }
    }

    fn legacy_tool_state_to_v2(input_data: LegacyToolStateInput<'_>) -> crate::ToolState {
        let now = chrono::Utc::now().timestamp_millis();
        match input_data.status {
            crate::ToolCallStatus::Pending => crate::ToolState::Pending {
                input: input_data.input.clone(),
                raw: input_data.raw.to_string(),
            },
            crate::ToolCallStatus::Running => crate::ToolState::Running {
                input: input_data.input.clone(),
                title: None,
                metadata: None,
                time: crate::RunningTime { start: now },
            },
            crate::ToolCallStatus::Completed => {
                let (output, title, mut metadata, part_attachments) = input_data
                    .tool_result
                    .map(|(content, _, title, metadata, attachments)| {
                        (
                            content.clone(),
                            title
                                .clone()
                                .unwrap_or_else(|| input_data.tool_name.to_string()),
                            metadata.clone().unwrap_or_default(),
                            attachments.clone(),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            String::new(),
                            input_data.tool_name.to_string(),
                            HashMap::new(),
                            None,
                        )
                    });

                let mut merged_attachments = Vec::new();
                if let Some(values) = part_attachments {
                    merged_attachments.extend(values);
                }
                if let Some(values) = Self::take_attachment_values(&mut metadata) {
                    merged_attachments.extend(values);
                }
                let (_, normalized_attachments) = Self::normalize_tool_attachments(
                    (!merged_attachments.is_empty()).then_some(merged_attachments),
                    input_data.session_id,
                    input_data.message_id,
                );

                crate::ToolState::Completed {
                    input: input_data.input.clone(),
                    output,
                    title,
                    metadata,
                    time: crate::CompletedTime {
                        start: now,
                        end: now,
                        compacted: None,
                    },
                    attachments: normalized_attachments,
                }
            }
            crate::ToolCallStatus::Error => {
                let error = input_data
                    .tool_result
                    .map(|(content, _, _, _, _)| content.clone())
                    .unwrap_or_else(|| {
                        format!("Tool execution failed: {}", input_data.tool_call_id)
                    });
                crate::ToolState::Error {
                    input: input_data.input.clone(),
                    error,
                    metadata: None,
                    time: crate::ErrorTime {
                        start: now,
                        end: now,
                    },
                }
            }
        }
    }

    pub(super) async fn summarize_session(
        session: &mut Session,
        session_id: &str,
        provider_id: &str,
        model_id: &str,
        provider: &dyn Provider,
    ) -> anyhow::Result<()> {
        let directory = session.directory.clone();
        let worktree = std::path::Path::new(&directory);
        let last_user = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::User))
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let messages =
            Self::to_message_with_parts(&session.messages, provider_id, model_id, &directory);
        summarize_into_session(
            &SummarizeInput {
                session_id: session_id.to_string(),
                message_id: last_user,
            },
            session,
            &messages,
            worktree,
            Some(provider),
            Some(model_id),
            None,
        )
        .await?;

        Ok(())
    }

    pub(super) fn prune_after_loop(session: &mut Session) {
        let mut tool_name_by_call: HashMap<String, String> = HashMap::new();
        for msg in &session.messages {
            for part in &msg.parts {
                if let PartType::ToolCall { id, name, .. } = &part.part_type {
                    tool_name_by_call.insert(id.clone(), name.clone());
                }
            }
        }

        let mut prune_messages: Vec<MessageForPrune> = session
            .messages
            .iter()
            .map(|m| {
                let parts: Vec<PruneToolPart> = m
                    .parts
                    .iter()
                    .filter_map(|p| match &p.part_type {
                        PartType::ToolResult {
                            tool_call_id,
                            content,
                            is_error,
                            ..
                        } => Some(PruneToolPart {
                            id: p.id.clone(),
                            tool: tool_name_by_call
                                .get(tool_call_id)
                                .cloned()
                                .unwrap_or_default(),
                            output: content.clone(),
                            status: if *is_error {
                                ToolPartStatus::Error
                            } else {
                                ToolPartStatus::Completed
                            },
                            compacted: None,
                        }),
                        _ => None,
                    })
                    .collect();
                MessageForPrune {
                    role: match m.role {
                        Role::User => rocode_provider::Role::User,
                        _ => rocode_provider::Role::Assistant,
                    },
                    parts,
                    summary: false,
                }
            })
            .collect();

        let engine = CompactionEngine::new(CompactionConfig::default());
        let pruned_ids = engine.prune(&mut prune_messages);
        if pruned_ids.is_empty() {
            return;
        }
        let pruned: HashSet<String> = pruned_ids.into_iter().collect();
        for msg in &mut session.messages {
            for part in &mut msg.parts {
                if !pruned.contains(&part.id) {
                    continue;
                }
                if let PartType::ToolResult { content, .. } = &mut part.part_type {
                    let compacted = content.chars().take(200).collect::<String>();
                    *content = format!("[tool result compacted]\n{}", compacted);
                }
            }
        }

        // Mark session as updated so pruning effects are persisted.
        session.touch();
    }

    pub(super) fn trigger_compaction(
        session: &mut Session,
        messages: &[SessionMessage],
    ) -> Option<String> {
        let total_messages = messages.len();
        if total_messages < 10 {
            return None;
        }

        let keep_count = total_messages / 2;
        let summary_parts: Vec<String> = messages
            .iter()
            .take(keep_count)
            .flat_map(|m| m.parts.iter())
            .filter_map(|p| match &p.part_type {
                PartType::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        let summary = format!(
            "Compacted {} messages. Summary: {}...",
            total_messages - keep_count,
            summary_parts
                .join(" ")
                .chars()
                .take(500)
                .collect::<String>()
        );

        // Persist the compaction summary as a Compaction part on a new assistant message.
        // This mirrors the TS behavior where compaction creates an assistant message with
        // summary=true and a compaction part, so that filter_compacted_messages can find it.
        let mut compaction_msg = SessionMessage::assistant(session.id.clone());
        compaction_msg.parts.push(crate::MessagePart {
            id: format!("prt_{}", uuid::Uuid::new_v4()),
            part_type: PartType::Compaction {
                summary: summary.clone(),
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        session.messages.push(compaction_msg);

        // Mark session as updated so compaction summary is persisted.
        session.touch();

        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use rocode_provider::{ChatRequest, ChatResponse, ModelInfo, ProviderError, StreamResult};

    struct StaticModelProvider {
        model: Option<ModelInfo>,
    }

    impl StaticModelProvider {
        fn with_model(model_id: &str, context_window: u64, max_output_tokens: u64) -> Self {
            Self {
                model: Some(ModelInfo {
                    id: model_id.to_string(),
                    name: "Static Model".to_string(),
                    provider: "mock".to_string(),
                    context_window,
                    max_input_tokens: None,
                    max_output_tokens,
                    supports_vision: false,
                    supports_tools: false,
                    cost_per_million_input: 0.0,
                    cost_per_million_output: 0.0,
                }),
            }
        }
    }

    #[async_trait]
    impl Provider for StaticModelProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            self.model.clone().into_iter().collect()
        }

        fn get_model(&self, id: &str) -> Option<&ModelInfo> {
            self.model.as_ref().filter(|model| model.id == id)
        }
        // PLACEHOLDER_TESTS_CONTINUE_1

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "chat() not used in this test".to_string(),
            ))
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[test]
    fn filter_compacted_messages_keeps_tail_after_last_compaction() {
        let session_id = "ses_test".to_string();
        let before = SessionMessage::user(session_id.clone(), "before");
        let mut compact = SessionMessage::assistant(session_id.clone());
        compact.parts.push(crate::MessagePart {
            id: "prt_compact".to_string(),
            part_type: PartType::Compaction {
                summary: "summary".to_string(),
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        let after = SessionMessage::user(session_id, "after");

        let filtered = SessionPrompt::filter_compacted_messages(&[before, compact, after]);
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0]
            .parts
            .iter()
            .any(|p| matches!(p.part_type, PartType::Compaction { .. })));
    }

    #[test]
    fn filter_compacted_messages_preserves_latest_user_anchor_when_tail_has_no_user() {
        let session_id = "ses_test_anchor".to_string();
        let user = SessionMessage::user(session_id.clone(), "user anchor");

        let mut compact = SessionMessage::assistant(session_id.clone());
        compact.parts.push(crate::MessagePart {
            id: "prt_compact_anchor".to_string(),
            part_type: PartType::Compaction {
                summary: "summary".to_string(),
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });

        let assistant_after = SessionMessage::assistant(session_id);
        let filtered =
            SessionPrompt::filter_compacted_messages(&[user.clone(), compact, assistant_after]);

        assert_eq!(filtered.len(), 3);
        assert!(matches!(filtered[0].role, Role::User));
        assert_eq!(filtered[0].id, user.id);
    }

    #[test]
    fn prune_after_loop_compacts_large_old_tool_results() {
        let mut session = Session::new(".");
        let session_id = session.id.clone();

        session
            .messages
            .push(SessionMessage::user(session_id.clone(), "old user message"));

        let mut old_assistant = SessionMessage::assistant(session_id.clone());
        old_assistant.add_tool_call("call_a", "bash", serde_json::json!({"command": "echo a"}));
        old_assistant.add_tool_result("call_a", "A".repeat(140_000), false);
        old_assistant.add_tool_call("call_b", "bash", serde_json::json!({"command": "echo b"}));
        old_assistant.add_tool_result("call_b", "B".repeat(140_000), false);
        session.messages.push(old_assistant);
        // PLACEHOLDER_TESTS_CONTINUE_2

        session
            .messages
            .push(SessionMessage::user(session_id.clone(), "new user one"));
        session
            .messages
            .push(SessionMessage::assistant(session_id.clone()));
        session
            .messages
            .push(SessionMessage::user(session_id.clone(), "new user two"));
        session.messages.push(SessionMessage::assistant(session_id));

        SessionPrompt::prune_after_loop(&mut session);

        let compacted_count = session
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter_map(|p| match &p.part_type {
                PartType::ToolResult { content, .. } => Some(content),
                _ => None,
            })
            .filter(|c| c.starts_with("[tool result compacted]"))
            .count();

        assert!(
            compacted_count >= 1,
            "expected at least one tool result to be compacted"
        );
    }

    #[test]
    fn should_compact_prefers_provider_model_limits() {
        let provider = StaticModelProvider::with_model("tiny-model", 1000, 100);
        let mut msg = SessionMessage::user("ses_test", "hello");
        msg.metadata
            .insert("tokens_input".to_string(), serde_json::json!(950_u64));

        let compact = SessionPrompt::should_compact(&[msg], &provider, "tiny-model", None);
        assert!(compact);
    }

    #[test]
    fn should_compact_counts_tool_results() {
        let provider = StaticModelProvider::with_model("big-model", 1_000_000, 65536);
        let mut msg = SessionMessage::assistant("ses_test");
        let large_content = "x".repeat(5_100_000);
        msg.parts.push(crate::MessagePart {
            id: "part_1".to_string(),
            part_type: PartType::ToolResult {
                tool_call_id: "tc_1".to_string(),
                content: large_content,
                is_error: false,
                title: None,
                metadata: None,
                attachments: None,
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        // PLACEHOLDER_TESTS_CONTINUE_3

        let compact = SessionPrompt::should_compact(&[msg], &provider, "big-model", None);
        assert!(
            compact,
            "should trigger compaction for >5MB tool result content"
        );
    }

    #[test]
    fn should_compact_uses_max_input_tokens() {
        let provider = StaticModelProvider {
            model: Some(ModelInfo {
                id: "limited-model".to_string(),
                name: "Limited Model".to_string(),
                provider: "mock".to_string(),
                context_window: 1_000_000,
                max_input_tokens: Some(50_000),
                max_output_tokens: 8192,
                supports_vision: false,
                supports_tools: false,
                cost_per_million_input: 0.0,
                cost_per_million_output: 0.0,
            }),
        };
        let mut msg = SessionMessage::user("ses_test", "hello");
        msg.metadata
            .insert("tokens_input".to_string(), serde_json::json!(48_000_u64));

        let compact = SessionPrompt::should_compact(&[msg], &provider, "limited-model", None);
        assert!(
            compact,
            "should trigger compaction when input tokens approach max_input_tokens"
        );
    }

    #[test]
    fn token_usage_from_messages_prefers_usage_field_over_metadata() {
        let mut msg = SessionMessage::assistant("ses_test");
        msg.metadata
            .insert("tokens_input".to_string(), serde_json::json!(1_u64));
        msg.metadata
            .insert("tokens_output".to_string(), serde_json::json!(2_u64));
        msg.metadata
            .insert("tokens_cache_read".to_string(), serde_json::json!(3_u64));
        msg.metadata
            .insert("tokens_cache_write".to_string(), serde_json::json!(4_u64));
        msg.usage = Some(crate::message::MessageUsage {
            input_tokens: 100,
            output_tokens: 200,
            reasoning_tokens: 50,
            cache_read_tokens: 30,
            cache_write_tokens: 20,
            total_cost: 0.0,
        });

        let usage = SessionPrompt::token_usage_from_messages(&[msg]);
        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 200);
        assert_eq!(usage.cache_read, 30);
        assert_eq!(usage.cache_write, 20);
        assert_eq!(usage.total, 350);
    }
    // PLACEHOLDER_TESTS_CONTINUE_4

    #[test]
    fn token_usage_from_messages_falls_back_to_usage_metadata_object() {
        let mut msg = SessionMessage::assistant("ses_test");
        msg.metadata.insert(
            "usage".to_string(),
            serde_json::json!({
                "prompt_tokens": 77_u64,
                "completion_tokens": 33_u64,
                "reasoning_tokens": 11_u64,
                "cache_read_tokens": 5_u64,
                "cache_write_tokens": 2_u64
            }),
        );

        let usage = SessionPrompt::token_usage_from_messages(&[msg]);
        assert_eq!(usage.input, 77);
        assert_eq!(usage.output, 33);
        assert_eq!(usage.cache_read, 5);
        assert_eq!(usage.cache_write, 2);
        assert_eq!(usage.total, 117);
    }

    #[test]
    fn build_chat_messages_splits_legacy_assistant_tool_results() {
        let sid = "sid".to_string();
        let mut assistant = SessionMessage::assistant(sid);
        assistant.add_text("working");
        assistant.add_tool_result("call_1", "ok", false);

        let messages = SessionPrompt::build_chat_messages(&[assistant], None).unwrap();
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, Role::Assistant));
        assert!(matches!(messages[1].role, Role::Tool));
    }

    #[test]
    fn legacy_tool_state_to_v2_recovers_attachments_from_tool_result_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "attachment".to_string(),
            serde_json::json!({ "mime": "application/pdf", "url": "data:application/pdf;base64,AA==" }),
        );
        metadata.insert(
            "preview".to_string(),
            serde_json::json!("PDF read successfully"),
        );

        let tool_result = (
            "PDF read successfully".to_string(),
            false,
            Some("Read".to_string()),
            Some(metadata),
            None,
        );

        let input = serde_json::json!({ "file_path": "report.pdf" });
        let state = SessionPrompt::legacy_tool_state_to_v2(LegacyToolStateInput {
            tool_call_id: "tool-call-1",
            tool_name: "read",
            input: &input,
            status: &crate::ToolCallStatus::Completed,
            raw: "",
            tool_result: Some(&tool_result),
            session_id: "ses_1",
            message_id: "msg_1",
        });

        match state {
            crate::ToolState::Completed {
                metadata,
                attachments,
                ..
            } => {
                assert!(!metadata.contains_key("attachment"));
                assert_eq!(attachments.as_ref().map(|v| v.len()), Some(1));
                assert_eq!(
                    attachments
                        .as_ref()
                        .and_then(|v| v.first())
                        .map(|f| f.mime.as_str()),
                    Some("application/pdf")
                );
            }
            _ => panic!("expected completed state"),
        }
    }
}
