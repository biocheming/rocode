use serde_json::{json, Value};
use std::collections::HashMap;

use rocode_core::contracts::provider::ProviderFinishReasonWire;
use rocode_core::contracts::provider::ProviderToolCallNameWire;

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
                        tool_name: ProviderToolCallNameWire::WebSearchCall.as_str().to_string(),
                        tool_call_id: id.clone(),
                        code_interpreter: None,
                    },
                );
                events.push(StreamEvent::ToolInputStart {
                    id,
                    tool_name: ProviderToolCallNameWire::WebSearchCall.as_str().to_string(),
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
                        tool_name: ProviderToolCallNameWire::CodeInterpreterCall
                            .as_str()
                            .to_string(),
                        tool_call_id: id.clone(),
                        code_interpreter: Some(CodeInterpreterState { container_id }),
                    },
                );
                events.push(StreamEvent::ToolInputStart {
                    id: id.clone(),
                    tool_name: ProviderToolCallNameWire::CodeInterpreterCall
                        .as_str()
                        .to_string(),
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
                    name: ProviderToolCallNameWire::FileSearchCall.as_str().to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: ProviderToolCallNameWire::FileSearchCall.as_str().to_string(),
                    input: json!({}),
                });
            }
            OutputItemAddedItem::ImageGenerationCall { id } => {
                events.push(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name: ProviderToolCallNameWire::ImageGenerationCall.as_str().to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: ProviderToolCallNameWire::ImageGenerationCall.as_str().to_string(),
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
                    name: ProviderToolCallNameWire::ComputerCall.as_str().to_string(),
                });
                events.push(StreamEvent::ToolCallEnd {
                    id,
                    name: ProviderToolCallNameWire::ComputerCall.as_str().to_string(),
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
                    name: ProviderToolCallNameWire::WebSearchCall.as_str().to_string(),
                    input: input.clone(),
                });
                events.push(StreamEvent::ToolResult {
                    tool_call_id: id,
                    tool_name: ProviderToolCallNameWire::WebSearchCall.as_str().to_string(),
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
                        name: ProviderToolCallNameWire::CodeInterpreterCall
                            .as_str()
                            .to_string(),
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
                    tool_name: ProviderToolCallNameWire::CodeInterpreterCall
                        .as_str()
                        .to_string(),
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
                    tool_name: ProviderToolCallNameWire::FileSearchCall.as_str().to_string(),
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
                    tool_name: ProviderToolCallNameWire::ImageGenerationCall
                        .as_str()
                        .to_string(),
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
                    name: ProviderToolCallNameWire::LocalShell.as_str().to_string(),
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
                    tool_name: ProviderToolCallNameWire::ComputerCall.as_str().to_string(),
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
                tool_name: ProviderToolCallNameWire::ImageGenerationCall
                    .as_str()
                    .to_string(),
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
    let mut parts = Vec::new();
    let mut has_function_call = false;
    let mut logprobs = Vec::new();

    for item in output {
        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
            continue;
        };
        match item_type {
            "reasoning" => {
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let encrypted = item
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let summary = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();

                let mut provider_options = HashMap::new();
                if !id.is_empty() {
                    provider_options.insert("itemId".to_string(), Value::String(id));
                }
                if let Some(encrypted) = encrypted {
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
                for content in item
                    .get("content")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                {
                    let Some(content_type) = content.get("type").and_then(Value::as_str) else {
                        continue;
                    };
                    if content_type == "output_text" {
                        let text = content
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if !text.is_empty() {
                            parts.push(ContentPart {
                                content_type: "text".to_string(),
                                text: Some(text),
                                ..Default::default()
                            });
                        }
                        if let Some(lp) = content.get("logprobs").cloned() {
                            if let Ok(parsed) = serde_json::from_value::<Vec<LogprobEntry>>(lp) {
                                if !parsed.is_empty() {
                                    logprobs.push(parsed);
                                }
                            }
                        }
                    }
                }
            }
            "function_call" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                parts.push(ContentPart {
                    content_type: "tool_use".to_string(),
                    tool_use: Some(ToolUse {
                        id: call_id,
                        name,
                        input: parse_json_or_string(arguments.to_string()),
                    }),
                    ..Default::default()
                });
                has_function_call = true;
            }
            other => {
                // Provider tool call items (OpenAI Responses output items).
                //
                // Only treat *_call items as provider tool calls to avoid accidentally
                // interpreting unrelated types.
                if !other.ends_with("_call") && !other.ends_with("-call") {
                    continue;
                }

                let Some(provider_tool) = ProviderToolCallNameWire::parse(other) else {
                    continue;
                };

                match provider_tool {
                    ProviderToolCallNameWire::WebSearchCall => {
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let action =
                            item.get("action").cloned().unwrap_or_else(|| json!({}));
                        parts.push(provider_executed_tool_parts(
                            id,
                            provider_tool.as_str(),
                            action.clone(),
                            action,
                        ));
                        has_function_call = true;
                    }
                    ProviderToolCallNameWire::FileSearchCall => {
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let output = json!({
                            "queries": item.get("queries").cloned().unwrap_or_else(|| json!([])),
                            "results": item.get("results").cloned().unwrap_or(Value::Null),
                        });
                        parts.push(provider_executed_tool_parts(
                            id,
                            provider_tool.as_str(),
                            json!({}),
                            output,
                        ));
                        has_function_call = true;
                    }
                    ProviderToolCallNameWire::CodeInterpreterCall => {
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let input = json!({
                            "code": item.get("code").cloned().unwrap_or(Value::Null),
                            "container_id": item.get("container_id").cloned().unwrap_or(Value::Null),
                        });
                        let output = json!({
                            "outputs": item.get("outputs").cloned().unwrap_or(Value::Null),
                        });
                        parts.push(provider_executed_tool_parts(
                            id,
                            provider_tool.as_str(),
                            input,
                            output,
                        ));
                        has_function_call = true;
                    }
                    ProviderToolCallNameWire::ImageGenerationCall => {
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let output = json!({
                            "result": item.get("result").cloned().unwrap_or(Value::Null),
                        });
                        parts.push(provider_executed_tool_parts(
                            id,
                            provider_tool.as_str(),
                            json!({}),
                            output,
                        ));
                        has_function_call = true;
                    }
                    ProviderToolCallNameWire::ComputerCall => {
                        let id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let output = json!({
                            "status": item.get("status").cloned().unwrap_or(Value::Null),
                        });
                        parts.push(provider_executed_tool_parts(
                            id,
                            provider_tool.as_str(),
                            json!({}),
                            output,
                        ));
                        has_function_call = true;
                    }
                    ProviderToolCallNameWire::LocalShell => {
                        let call_id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let action =
                            item.get("action").cloned().unwrap_or_else(|| json!({}));
                        parts.push(ContentPart {
                            content_type: "tool_use".to_string(),
                            tool_use: Some(ToolUse {
                                id: call_id,
                                name: provider_tool.as_str().to_string(),
                                input: json!({ "action": action }),
                            }),
                            ..Default::default()
                        });
                        has_function_call = true;
                    }
                }
            }
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
        FinishReason::Stop => ProviderFinishReasonWire::Stop.as_str(),
        FinishReason::Length => ProviderFinishReasonWire::Length.as_str(),
        FinishReason::ContentFilter => ProviderFinishReasonWire::ContentFilter.as_str(),
        FinishReason::ToolCalls => ProviderFinishReasonWire::ToolCalls.as_str(),
        FinishReason::Error => ProviderFinishReasonWire::Error.as_str(),
        FinishReason::Unknown => ProviderFinishReasonWire::Unknown.as_str(),
    }
}
