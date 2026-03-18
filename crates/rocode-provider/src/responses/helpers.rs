use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::message::{ContentPart, ToolResult, ToolUse};
use crate::stream::{StreamEvent, StreamUsage, ToolResultOutput};

use super::types::{
    map_openai_response_finish_reason, ActiveReasoning, CodeInterpreterState, FinishReason,
    LogprobEntry, OngoingToolCall, OutputItemAddedItem, OutputItemDoneItem, ResponsesIncludeValue,
    ResponsesStreamChunk, ResponsesUsage,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn process_stream_chunk(
    chunk: ResponsesStreamChunk,
    finish_reason: &mut FinishReason,
    usage: &mut ResponsesUsage,
    logprobs: &mut Vec<Vec<LogprobEntry>>,
    response_id: &mut Option<String>,
    ongoing_tool_calls: &mut HashMap<usize, OngoingToolCall>,
    has_function_call: &mut bool,
    active_reasoning: &mut HashMap<usize, ActiveReasoning>,
    current_reasoning_output_index: &mut Option<usize>,
    reasoning_item_to_output_index: &mut HashMap<String, usize>,
    current_text_id: &mut Option<String>,
    text_open: &mut bool,
    service_tier: &mut Option<String>,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    match chunk {
        ResponsesStreamChunk::OutputItemAdded { output_index, item } => match item {
            OutputItemAddedItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_name: name.clone(),
                        tool_call_id: call_id.clone(),
                        code_interpreter: None,
                    },
                );
                events.push(StreamEvent::ToolInputStart {
                    id: call_id.clone(),
                    tool_name: name,
                });
                if !arguments.is_empty() {
                    events.push(StreamEvent::ToolInputDelta {
                        id: call_id,
                        delta: arguments,
                    });
                }
            }
            OutputItemAddedItem::WebSearchCall { id, .. } => {
                ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_name: "web_search_call".to_string(),
                        tool_call_id: id.clone(),
                        code_interpreter: None,
                    },
                );
                events.push(StreamEvent::ToolInputStart {
                    id,
                    tool_name: "web_search_call".to_string(),
                });
            }
            OutputItemAddedItem::CodeInterpreterCall {
                id,
                container_id,
                code,
                ..
            } => {
                ongoing_tool_calls.insert(
                    output_index,
                    OngoingToolCall {
                        tool_name: "code_interpreter_call".to_string(),
                        tool_call_id: id.clone(),
                        code_interpreter: Some(CodeInterpreterState { container_id }),
                    },
                );
                events.push(StreamEvent::ToolInputStart {
                    id: id.clone(),
                    tool_name: "code_interpreter_call".to_string(),
                });
                if let Some(code) = code {
                    if !code.is_empty() {
                        events.push(StreamEvent::ToolInputDelta { id, delta: code });
                    }
                }
            }
            OutputItemAddedItem::FileSearchCall { id } => {
                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name: "file_search_call".to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: "file_search_call".to_string(),
                    input: json!({}),
                });
            }
            OutputItemAddedItem::ImageGenerationCall { id } => {
                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name: "image_generation_call".to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: "image_generation_call".to_string(),
                    input: json!({}),
                });
            }
            OutputItemAddedItem::Message { id } => {
                *current_text_id = Some(id);
                if !*text_open {
                    *text_open = true;
                    events.push(StreamEvent::TextStart);
                }
            }
            OutputItemAddedItem::Reasoning {
                id,
                encrypted_content,
            } => {
                active_reasoning.insert(
                    output_index,
                    ActiveReasoning {
                        canonical_id: id.clone(),
                        encrypted_content,
                        summary_parts: vec![0],
                    },
                );
                reasoning_item_to_output_index.insert(id.clone(), output_index);
                *current_reasoning_output_index = Some(output_index);
                events.push(StreamEvent::ReasoningStart { id });
            }
            OutputItemAddedItem::ComputerCall { id, .. } => {
                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name: "computer_call".to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: "computer_call".to_string(),
                    input: json!({}),
                });
            }
        },
        ResponsesStreamChunk::OutputItemDone { output_index, item } => match item {
            OutputItemDoneItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => {
                ongoing_tool_calls.remove(&output_index);
                events.push(StreamEvent::ToolInputEnd {
                    id: call_id.clone(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id: call_id.clone(),
                    name: name.clone(),
                    input: parse_json_or_string(arguments),
                });
                *has_function_call = true;
            }
            OutputItemDoneItem::WebSearchCall { id, action, .. } => {
                ongoing_tool_calls.remove(&output_index);
                let input = action.unwrap_or_else(|| json!({}));
                events.push(StreamEvent::ToolInputEnd { id: id.clone() });
                events.push(StreamEvent::ToolCallEnd {
                    id: id.clone(),
                    name: "web_search_call".to_string(),
                    input: input.clone(),
                });
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: "web_search_call".to_string(),
                    input: Some(input.clone()),
                    output: ToolResultOutput {
                        output: serde_json::to_string(&input).unwrap_or_default(),
                        title: "Web Search".to_string(),
                        metadata: HashMap::from([(
                            "providerExecuted".to_string(),
                            Value::Bool(true),
                        )]),
                        attachments: None,
                    },
                });
                *has_function_call = true;
            }
            OutputItemDoneItem::CodeInterpreterCall {
                id,
                code,
                container_id,
                outputs,
            } => {
                ongoing_tool_calls.remove(&output_index);
                if let Some(code) = code {
                    events.push(StreamEvent::ToolCallEnd {
                        id: id.clone(),
                        name: "code_interpreter_call".to_string(),
                        input: json!({
                            "code": code,
                            "container_id": container_id,
                        }),
                    });
                    *has_function_call = true;
                }
                let output_json = json!({ "outputs": outputs });
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: "code_interpreter_call".to_string(),
                    input: None,
                    output: ToolResultOutput {
                        output: serde_json::to_string(&output_json).unwrap_or_default(),
                        title: "Code Interpreter".to_string(),
                        metadata: HashMap::from([(
                            "providerExecuted".to_string(),
                            Value::Bool(true),
                        )]),
                        attachments: None,
                    },
                });
            }
            OutputItemDoneItem::FileSearchCall {
                id,
                queries,
                results,
            } => {
                let output_json = json!({
                    "queries": queries.unwrap_or_default(),
                    "results": results,
                });
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: "file_search_call".to_string(),
                    input: None,
                    output: ToolResultOutput {
                        output: serde_json::to_string(&output_json).unwrap_or_default(),
                        title: "File Search".to_string(),
                        metadata: HashMap::from([(
                            "providerExecuted".to_string(),
                            Value::Bool(true),
                        )]),
                        attachments: None,
                    },
                });
            }
            OutputItemDoneItem::ImageGenerationCall { id, result } => {
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: "image_generation_call".to_string(),
                    input: None,
                    output: ToolResultOutput {
                        output: result,
                        title: "Image Generation".to_string(),
                        metadata: HashMap::from([(
                            "providerExecuted".to_string(),
                            Value::Bool(true),
                        )]),
                        attachments: None,
                    },
                });
            }
            OutputItemDoneItem::LocalShellCall {
                call_id, action, ..
            } => {
                ongoing_tool_calls.remove(&output_index);
                events.push(StreamEvent::ToolInputEnd {
                    id: call_id.clone(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id: call_id.clone(),
                    name: "local_shell".to_string(),
                    input: json!({ "action": action }),
                });
                *has_function_call = true;
            }
            OutputItemDoneItem::Message { id } => {
                if current_text_id.as_deref() == Some(id.as_str()) && *text_open {
                    *text_open = false;
                    *current_text_id = None;
                    events.push(StreamEvent::TextEnd);
                }
            }
            OutputItemDoneItem::Reasoning { id, .. } => {
                if let Some(index) = reasoning_item_to_output_index.remove(&id) {
                    if let Some(reasoning) = active_reasoning.remove(&index) {
                        events.push(StreamEvent::ReasoningEnd {
                            id: reasoning.canonical_id,
                        });
                    }
                }
            }
            OutputItemDoneItem::ComputerCall { id, status } => {
                let status = status.unwrap_or_else(|| "unknown".to_string());
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: "computer_call".to_string(),
                    input: None,
                    output: ToolResultOutput {
                        output: status,
                        title: "Computer Call".to_string(),
                        metadata: HashMap::from([(
                            "providerExecuted".to_string(),
                            Value::Bool(true),
                        )]),
                        attachments: None,
                    },
                });
            }
        },
        ResponsesStreamChunk::FunctionCallArgumentsDelta {
            output_index,
            delta,
            ..
        } => {
            if let Some(call) = ongoing_tool_calls.get(&output_index) {
                events.push(StreamEvent::ToolInputDelta {
                    id: call.tool_call_id.clone(),
                    delta,
                });
            }
        }
        ResponsesStreamChunk::CodeInterpreterCodeDelta {
            output_index,
            delta,
            ..
        } => {
            if let Some(call) = ongoing_tool_calls.get(&output_index) {
                events.push(StreamEvent::ToolInputDelta {
                    id: call.tool_call_id.clone(),
                    delta,
                });
            }
        }
        ResponsesStreamChunk::CodeInterpreterCodeDone {
            output_index, code, ..
        } => {
            if let Some(call) = ongoing_tool_calls.get(&output_index) {
                events.push(StreamEvent::ToolInputDelta {
                    id: call.tool_call_id.clone(),
                    delta: code.clone(),
                });
                events.push(StreamEvent::ToolInputEnd {
                    id: call.tool_call_id.clone(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id: call.tool_call_id.clone(),
                    name: call.tool_name.clone(),
                    input: json!({
                        "code": code,
                        "container_id": call.code_interpreter.as_ref().map(|c| c.container_id.clone()),
                    }),
                });
            }
            *has_function_call = true;
        }
        ResponsesStreamChunk::ImageGenerationPartialImage {
            item_id,
            partial_image_b64,
            ..
        } => {
            events.push(StreamEvent::ToolResult {
                tool_call_id: item_id,
                tool_name: "image_generation_call".to_string(),
                input: None,
                output: ToolResultOutput {
                    output: partial_image_b64,
                    title: "Image Generation (partial)".to_string(),
                    metadata: HashMap::from([("partial".to_string(), Value::Bool(true))]),
                    attachments: None,
                },
            });
        }
        ResponsesStreamChunk::ResponseCreated { response } => {
            *response_id = Some(response.id);
            *service_tier = response.service_tier;
        }
        ResponsesStreamChunk::TextDelta {
            item_id,
            delta,
            logprobs: lp,
        } => {
            if !*text_open {
                *text_open = true;
                *current_text_id = Some(item_id);
                events.push(StreamEvent::TextStart);
            }
            if !delta.is_empty() {
                events.push(StreamEvent::TextDelta(delta));
            }
            if let Some(entries) = lp {
                logprobs.push(entries);
            }
        }
        ResponsesStreamChunk::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            let maybe_index = reasoning_item_to_output_index
                .get(&item_id)
                .copied()
                .or(*current_reasoning_output_index);
            if let Some(index) = maybe_index {
                if let Some(reasoning) = active_reasoning.get_mut(&index) {
                    if !reasoning.summary_parts.contains(&summary_index) {
                        reasoning.summary_parts.push(summary_index);
                        if summary_index > 0 {
                            events.push(StreamEvent::ReasoningStart {
                                id: reasoning.canonical_id.clone(),
                            });
                        }
                    }
                }
            }
        }
        ResponsesStreamChunk::ReasoningSummaryTextDelta { item_id, delta, .. } => {
            if let Some(index) = reasoning_item_to_output_index.get(&item_id).copied() {
                if let Some(reasoning) = active_reasoning.get(&index) {
                    events.push(StreamEvent::ReasoningDelta {
                        id: reasoning.canonical_id.clone(),
                        text: delta,
                    });
                }
            }
        }
        ResponsesStreamChunk::ResponseCompleted { response } => {
            *usage = response.usage.clone();
            *service_tier = response.service_tier;
            *finish_reason = map_openai_response_finish_reason(
                response
                    .incomplete_details
                    .as_ref()
                    .map(|d| d.reason.as_str()),
                *has_function_call,
            );
        }
        ResponsesStreamChunk::ResponseIncomplete { response } => {
            *usage = response.usage.clone();
            *service_tier = response.service_tier;
            *finish_reason = map_openai_response_finish_reason(
                response
                    .incomplete_details
                    .as_ref()
                    .map(|d| d.reason.as_str()),
                *has_function_call,
            );
        }
        ResponsesStreamChunk::AnnotationAdded { .. } => {}
        ResponsesStreamChunk::Error { message, .. } => {
            events.push(StreamEvent::Error(message));
            *finish_reason = FinishReason::Error;
        }
        ResponsesStreamChunk::Unknown => {}
    }

    events
}

pub(super) fn parse_output_items(
    output: &[Value],
) -> (Vec<ContentPart>, bool, Vec<Vec<LogprobEntry>>) {
    fn deserialize_vec_value_lossy<'de, D>(deserializer: D) -> Result<Vec<Value>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<Value>::deserialize(deserializer)?;
        Ok(match value {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values,
            Some(other) => vec![other],
        })
    }

    #[derive(Debug, Default, Deserialize)]
    struct OutputItemWire {
        #[serde(rename = "type", default)]
        item_type: String,
        #[serde(default)]
        id: String,
        #[serde(default)]
        call_id: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        arguments: Option<Value>,
        #[serde(default)]
        encrypted_content: Option<String>,
        #[serde(default, deserialize_with = "deserialize_vec_value_lossy")]
        summary: Vec<Value>,
        #[serde(default, deserialize_with = "deserialize_vec_value_lossy")]
        content: Vec<Value>,
        #[serde(default)]
        action: Option<Value>,
        #[serde(default)]
        queries: Option<Value>,
        #[serde(default)]
        results: Option<Value>,
        #[serde(default)]
        code: Option<Value>,
        #[serde(default)]
        container_id: Option<Value>,
        #[serde(default)]
        outputs: Option<Value>,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        status: Option<Value>,
    }

    #[derive(Debug, Default, Deserialize)]
    struct SummaryTextWire {
        #[serde(default)]
        text: String,
    }

    #[derive(Debug, Default, Deserialize)]
    struct MessageContentWire {
        #[serde(rename = "type", default)]
        content_type: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        logprobs: Option<Value>,
    }

    let mut parts = Vec::new();
    let mut has_function_call = false;
    let mut logprobs = Vec::new();

    for item in output {
        let mut wire = match serde_json::from_value::<OutputItemWire>(item.clone()) {
            Ok(wire) => wire,
            Err(_) => continue,
        };

        let item_type = std::mem::take(&mut wire.item_type);

        match item_type.as_str() {
            "reasoning" => {
                let summary = wire
                    .summary
                    .into_iter()
                    .filter_map(|entry| serde_json::from_value::<SummaryTextWire>(entry).ok())
                    .map(|entry| entry.text)
                    .filter(|text| !text.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut provider_options = HashMap::new();
                if !wire.id.is_empty() {
                    provider_options.insert("itemId".to_string(), Value::String(wire.id));
                }
                if let Some(encrypted) = wire.encrypted_content {
                    provider_options
                        .insert("encryptedContent".to_string(), Value::String(encrypted));
                }

                parts.push(ContentPart {
                    content_type: "reasoning".to_string(),
                    text: Some(summary),
                    provider_options: if provider_options.is_empty() {
                        None
                    } else {
                        Some(provider_options)
                    },
                    ..Default::default()
                });
            }
            "message" => {
                for content in wire.content {
                    let Ok(content) = serde_json::from_value::<MessageContentWire>(content) else {
                        continue;
                    };
                    if content.content_type != "output_text" {
                        continue;
                    }
                    if !content.text.is_empty() {
                        parts.push(ContentPart {
                            content_type: "text".to_string(),
                            text: Some(content.text),
                            ..Default::default()
                        });
                    }
                    if let Some(logprobs_value) = content.logprobs {
                        if let Ok(parsed) =
                            serde_json::from_value::<Vec<LogprobEntry>>(logprobs_value)
                        {
                            if !parsed.is_empty() {
                                logprobs.push(parsed);
                            }
                        }
                    }
                }
            }
            "function_call" => {
                let arguments = match wire.arguments {
                    Some(Value::String(raw)) => parse_json_or_string(raw),
                    Some(other) => other,
                    None => json!({}),
                };
                parts.push(ContentPart {
                    content_type: "tool_use".to_string(),
                    tool_use: Some(ToolUse {
                        id: wire.call_id,
                        name: wire.name,
                        input: arguments,
                    }),
                    ..Default::default()
                });
                has_function_call = true;
            }
            "web_search_call" => {
                let id = wire.id;
                let action = wire.action.unwrap_or_else(|| json!({}));
                parts.push(provider_executed_tool_parts(
                    id,
                    "web_search_call",
                    action.clone(),
                    action,
                ));
                has_function_call = true;
            }
            "file_search_call" => {
                let output = json!({
                    "queries": wire.queries.unwrap_or_else(|| json!([])),
                    "results": wire.results.unwrap_or(Value::Null),
                });
                parts.push(provider_executed_tool_parts(
                    wire.id,
                    "file_search_call",
                    json!({}),
                    output,
                ));
                has_function_call = true;
            }
            "code_interpreter_call" => {
                let input = json!({
                    "code": wire.code.unwrap_or(Value::Null),
                    "container_id": wire.container_id.unwrap_or(Value::Null),
                });
                let output = json!({
                    "outputs": wire.outputs.unwrap_or(Value::Null),
                });
                parts.push(provider_executed_tool_parts(
                    wire.id,
                    "code_interpreter_call",
                    input,
                    output,
                ));
                has_function_call = true;
            }
            "image_generation_call" => {
                let output = json!({
                    "result": wire.result.unwrap_or(Value::Null),
                });
                parts.push(provider_executed_tool_parts(
                    wire.id,
                    "image_generation_call",
                    json!({}),
                    output,
                ));
                has_function_call = true;
            }
            "local_shell_call" => {
                let action = wire.action.unwrap_or_else(|| json!({}));
                parts.push(ContentPart {
                    content_type: "tool_use".to_string(),
                    tool_use: Some(ToolUse {
                        id: wire.call_id,
                        name: "local_shell".to_string(),
                        input: json!({ "action": action }),
                    }),
                    ..Default::default()
                });
                has_function_call = true;
            }
            "computer_call" => {
                let output = json!({
                    "status": wire.status.unwrap_or(Value::Null),
                });
                parts.push(provider_executed_tool_parts(
                    wire.id,
                    "computer_call",
                    json!({}),
                    output,
                ));
                has_function_call = true;
            }
            _ => {}
        }
    }

    (parts, has_function_call, logprobs)
}

fn provider_executed_tool_parts(
    id: String,
    tool_name: &str,
    input: Value,
    output: Value,
) -> ContentPart {
    let mut provider_options = HashMap::new();
    provider_options.insert("providerExecuted".to_string(), Value::Bool(true));

    ContentPart {
        content_type: "tool_result".to_string(),
        tool_use: Some(ToolUse {
            id: id.clone(),
            name: tool_name.to_string(),
            input,
        }),
        tool_result: Some(ToolResult {
            tool_use_id: id,
            content: serde_json::to_string(&output).unwrap_or_default(),
            is_error: Some(false),
        }),
        provider_options: Some(provider_options),
        ..Default::default()
    }
}

pub(super) fn usage_to_stream_usage(usage: &ResponsesUsage) -> StreamUsage {
    StreamUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        reasoning_tokens: usage
            .output_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens)
            .unwrap_or(0),
        cache_read_tokens: usage
            .input_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens)
            .unwrap_or(0),
        cache_write_tokens: 0,
    }
}

pub(super) fn push_include(include: &mut Vec<ResponsesIncludeValue>, value: ResponsesIncludeValue) {
    if !include.contains(&value) {
        include.push(value);
    }
}

pub(super) fn insert_opt_string(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        obj.insert(key.to_string(), Value::String(value));
    }
}

pub(super) fn insert_opt_u64(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<u64>,
) {
    if let Some(value) = value {
        obj.insert(key.to_string(), Value::Number(value.into()));
    }
}

pub(super) fn insert_opt_bool(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<bool>,
) {
    if let Some(value) = value {
        obj.insert(key.to_string(), Value::Bool(value));
    }
}

pub(super) fn insert_opt_value(
    obj: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<Value>,
) {
    if let Some(value) = value {
        obj.insert(key.to_string(), value);
    }
}

fn parse_json_or_string(raw: String) -> Value {
    serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw))
}

pub(super) fn drain_next_sse_frame(buffer: &mut String) -> Option<String> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    let (idx, len) = match (lf, crlf) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_a), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };

    let frame = buffer[..idx].to_string();
    buffer.drain(..idx + len);
    Some(frame)
}

pub(super) fn extract_sse_data(frame: &str) -> Option<String> {
    let mut data_lines = Vec::new();
    for raw_line in frame.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

pub(super) fn finish_reason_label(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ContentFilter => "content-filter",
        FinishReason::ToolCalls => "tool-calls",
        FinishReason::Error => "error",
        FinishReason::Unknown => "unknown",
    }
}
