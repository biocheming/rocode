#[cfg(test)]
use crate::output_blocks::SchedulerStageBlock;
use crate::output_blocks::{
    tool_web_fields, tool_web_header, tool_web_preview, tool_web_summary, BlockTone, MessageBlock,
    MessagePhase, OutputBlock, QueueItemBlock, ReasoningBlock, Role, SessionEventBlock,
    SessionEventField, StatusBlock, ToolBlock, ToolPhase, ToolStructuredDetail,
};
use rocode_agent::{AgentRenderEvent, AgentRenderOutcome, AgentToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct AgentPresenterConfig {
    pub tool_progress_limit: usize,
    pub tool_result_limit: usize,
    pub tool_error_limit: usize,
    pub tool_end_limit: usize,
}

impl Default for AgentPresenterConfig {
    fn default() -> Self {
        Self {
            tool_progress_limit: 96,
            tool_result_limit: 120,
            tool_error_limit: 120,
            tool_end_limit: 96,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresentedAgentOutput {
    pub blocks: Vec<OutputBlock>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub stream_error: Option<String>,
}

pub fn present_agent_outcome(
    outcome: AgentRenderOutcome,
    config: AgentPresenterConfig,
) -> PresentedAgentOutput {
    let blocks = outcome
        .events
        .into_iter()
        .filter_map(|event| map_render_event_to_block(event, config))
        .collect();

    PresentedAgentOutput {
        blocks,
        prompt_tokens: outcome.prompt_tokens,
        completion_tokens: outcome.completion_tokens,
        stream_error: outcome.stream_error,
    }
}

pub fn map_render_event_to_block(
    event: AgentRenderEvent,
    config: AgentPresenterConfig,
) -> Option<OutputBlock> {
    match event {
        AgentRenderEvent::AssistantStart => {
            Some(OutputBlock::Message(MessageBlock::start(Role::Assistant)))
        }
        AgentRenderEvent::AssistantDelta(text) => {
            if text.is_empty() {
                None
            } else {
                Some(OutputBlock::Message(MessageBlock::delta(
                    Role::Assistant,
                    text,
                )))
            }
        }
        AgentRenderEvent::AssistantEnd => {
            Some(OutputBlock::Message(MessageBlock::end(Role::Assistant)))
        }
        AgentRenderEvent::ToolStart { name, .. } => Some(OutputBlock::Tool(ToolBlock::start(name))),
        AgentRenderEvent::ToolProgress { name, input, .. } => Some(OutputBlock::Tool(
            ToolBlock::running(name, truncate_text(&input, config.tool_progress_limit)),
        )),
        AgentRenderEvent::ToolEnd { name, input, .. } => {
            let structured = extract_tool_input_structured(&name, &input);
            let mut block = ToolBlock::done(
                name,
                Some(truncate_text(&input.to_string(), config.tool_end_limit)),
            );
            if let Some(s) = structured {
                block = block.with_structured(s);
            }
            Some(OutputBlock::Tool(block))
        }
        AgentRenderEvent::ToolResult {
            tool_name, output, ..
        } => {
            let mut detail = if output.title.trim().is_empty() {
                output.output.clone()
            } else {
                format!("{}: {}", output.title, output.output)
            };
            detail = truncate_text(&detail, config.tool_result_limit);
            let structured = extract_tool_result_structured(&tool_name, &output);
            let mut block = ToolBlock::done(tool_name, Some(detail));
            if let Some(s) = structured {
                block = block.with_structured(s);
            }
            Some(OutputBlock::Tool(block))
        }
        AgentRenderEvent::ToolError {
            tool_name, error, ..
        } => Some(OutputBlock::Tool(ToolBlock::error(
            tool_name,
            truncate_text(&error, config.tool_error_limit),
        ))),
        AgentRenderEvent::ReasoningStart => Some(OutputBlock::Reasoning(ReasoningBlock::start())),
        AgentRenderEvent::ReasoningDelta(text) => {
            if text.is_empty() {
                None
            } else {
                Some(OutputBlock::Reasoning(ReasoningBlock::delta(text)))
            }
        }
        AgentRenderEvent::ReasoningEnd => Some(OutputBlock::Reasoning(ReasoningBlock::end())),
    }
}

pub fn history_tool_call_to_web(
    tool_call_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    status: Option<&str>,
    raw: Option<&str>,
) -> serde_json::Value {
    let normalized_status = status.unwrap_or("pending");
    let detail = history_tool_call_detail(input, raw, normalized_status);
    let structured = extract_tool_input_structured(tool_name, input);
    let phase = match normalized_status {
        "running" => ToolPhase::Running,
        "completed" => ToolPhase::Done,
        "error" => ToolPhase::Error,
        _ => ToolPhase::Start,
    };

    let mut block = ToolBlock {
        name: tool_name.to_string(),
        phase,
        detail,
        structured: None,
    };
    if let Some(structured) = structured {
        block = block.with_structured(structured);
    }

    let mut web = output_block_to_web(&OutputBlock::Tool(block));
    if let serde_json::Value::Object(ref mut map) = web {
        map.insert("id".to_string(), json!(tool_call_id));
    }
    apply_history_tool_call_display_override(&mut web, tool_name, input);
    web
}

pub fn history_tool_result_to_web(
    tool_call_id: &str,
    tool_name: &str,
    title: Option<&str>,
    content: &str,
    is_error: bool,
    metadata: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    let output = AgentToolOutput {
        output: content.to_string(),
        title: title.unwrap_or_default().to_string(),
        metadata: metadata.clone(),
    };
    let detail = history_tool_result_detail(title, content);
    let structured = extract_tool_result_structured(tool_name, &output);
    let mut block = if is_error {
        ToolBlock::error(
            tool_name.to_string(),
            detail.unwrap_or_else(|| content.to_string()),
        )
    } else {
        ToolBlock::done(tool_name.to_string(), detail)
    };
    if let Some(structured) = structured {
        block = block.with_structured(structured);
    }
    let mut web = output_block_to_web(&OutputBlock::Tool(block));
    if let serde_json::Value::Object(ref mut map) = web {
        map.insert("id".to_string(), json!(tool_call_id));
    }
    apply_history_tool_result_display_override(&mut web, tool_name, title, metadata);
    apply_history_tool_result_interaction(&mut web, tool_name, title, content, is_error);
    web
}

pub fn history_session_event_to_web(
    event: &str,
    title: impl Into<String>,
    status: Option<&str>,
    summary: Option<String>,
    fields: Vec<(String, String, Option<String>)>,
    body: Option<String>,
) -> serde_json::Value {
    output_block_to_web(&OutputBlock::SessionEvent(SessionEventBlock {
        event: event.to_string(),
        title: title.into(),
        status: status.map(str::to_string),
        summary,
        fields: fields
            .into_iter()
            .map(|(label, value, tone)| SessionEventField { label, value, tone })
            .collect(),
        body,
    }))
}

fn history_tool_call_detail(
    input: &serde_json::Value,
    raw: Option<&str>,
    status: &str,
) -> Option<String> {
    if let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(raw.to_string());
    }

    match status {
        "completed" | "error" => None,
        _ => {
            if input.is_null() {
                return None;
            }
            if let Some(obj) = input.as_object() {
                if obj.is_empty() {
                    return None;
                }
            }
            if let Some(arr) = input.as_array() {
                if arr.is_empty() {
                    return None;
                }
            }
            if let Some(text) = input.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            Some(input.to_string())
        }
    }
}

fn history_tool_result_detail(title: Option<&str>, content: &str) -> Option<String> {
    match title.map(str::trim).filter(|value| !value.is_empty()) {
        Some(title) => Some(format!("{title}: {content}")),
        None if content.trim().is_empty() => None,
        None => Some(content.to_string()),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TodoItem {
    #[serde(default)]
    content: String,
    #[serde(default)]
    status: Option<String>,
}

fn apply_history_tool_call_display_override(
    web: &mut serde_json::Value,
    tool_name: &str,
    input: &serde_json::Value,
) {
    match tool_name {
        "question" => {
            #[derive(Debug, Deserialize)]
            struct QuestionToolInput {
                #[serde(rename = "questions", deserialize_with = "deserialize_questions")]
                questions: Vec<QuestionDef>,
            }

            #[derive(Debug, Deserialize)]
            struct QuestionDef {
                question: String,
                #[serde(default)]
                header: Option<String>,
            }

            fn deserialize_questions<'de, D>(deserializer: D) -> Result<Vec<QuestionDef>, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = serde_json::Value::deserialize(deserializer)?;
                fn parse_questions_value(
                    value: &serde_json::Value,
                ) -> Result<Vec<QuestionDef>, String> {
                    match value {
                        serde_json::Value::Array(_) => {
                            serde_json::from_value::<Vec<QuestionDef>>(value.clone())
                                .map_err(|e| format!("failed to parse questions array: {e}"))
                        }
                        serde_json::Value::Object(_) => {
                            serde_json::from_value::<QuestionDef>(value.clone())
                                .map(|q| vec![q])
                                .map_err(|e| format!("failed to parse question object: {e}"))
                        }
                        serde_json::Value::String(raw) => {
                            if let Ok(list) = serde_json::from_str::<Vec<QuestionDef>>(raw) {
                                return Ok(list);
                            }
                            if let Ok(single) = serde_json::from_str::<QuestionDef>(raw) {
                                return Ok(vec![single]);
                            }
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) {
                                return parse_questions_value(&parsed);
                            }
                            Err("questions must be an array/object or a JSON string representing them"
                                .into())
                        }
                        _ => Err("questions must be an array/object or a JSON string".into()),
                    }
                }
                parse_questions_value(&value).map_err(serde::de::Error::custom)
            }

            let Ok(parsed) = serde_json::from_value::<QuestionToolInput>(input.clone()) else {
                return;
            };
            if parsed.questions.is_empty() {
                return;
            }
            let summary = Some(if parsed.questions.len() == 1 {
                "1 question requested".to_string()
            } else {
                format!("{} questions requested", parsed.questions.len())
            });
            let fields = parsed
                .questions
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let label = item
                        .header
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Question {}", index + 1));
                    to_value_or_null(DisplayFieldValue {
                        label,
                        value: item.question.clone(),
                    })
                })
                .collect::<Vec<_>>();
            apply_display_override(web, summary, fields, None);
        }
        "todowrite" | "todo_write" => {
            #[derive(Debug, Deserialize)]
            struct TodoWriteToolInput {
                #[serde(default)]
                todos: Vec<TodoItem>,
            }

            let Ok(parsed) = serde_json::from_value::<TodoWriteToolInput>(input.clone()) else {
                return;
            };
            let summary = Some(format!("{} todo items proposed", parsed.todos.len()));
            let fields = todo_summary_fields_from_array(&parsed.todos);
            let preview = todo_preview_from_array(&parsed.todos);
            apply_display_override(web, summary, fields, preview);
        }
        "todoread" | "todo_read" => {
            apply_display_override(
                web,
                Some("Read current todo list".to_string()),
                Vec::new(),
                None,
            );
        }
        _ => {}
    }
}

fn apply_history_tool_result_display_override(
    web: &mut serde_json::Value,
    tool_name: &str,
    title: Option<&str>,
    metadata: &HashMap<String, serde_json::Value>,
) {
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
            Some(serde_json::Value::String(value)) => value.parse::<u64>().ok(),
            _ => None,
        })
    }

    match tool_name {
        "question" => {
            #[derive(Debug, Deserialize)]
            struct DisplayField {
                key: String,
                #[serde(default)]
                value: Option<String>,
            }

            fn deserialize_vec_display_field_lossy<'de, D>(
                deserializer: D,
            ) -> Result<Vec<DisplayField>, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = Option::<serde_json::Value>::deserialize(deserializer)?;
                let Some(value) = value else {
                    return Ok(Vec::new());
                };
                Ok(serde_json::from_value::<Vec<DisplayField>>(value).unwrap_or_default())
            }

            #[derive(Debug, Default, Deserialize)]
            struct QuestionToolMetadataWire {
                #[serde(
                    default,
                    rename = "display.summary",
                    deserialize_with = "deserialize_opt_string_lossy"
                )]
                display_summary: Option<String>,
                #[serde(
                    default,
                    rename = "display.fields",
                    deserialize_with = "deserialize_vec_display_field_lossy"
                )]
                display_fields: Vec<DisplayField>,
            }

            let wire = serde_json::to_value(metadata)
                .ok()
                .and_then(|value| serde_json::from_value::<QuestionToolMetadataWire>(value).ok())
                .unwrap_or_default();

            let summary = wire.display_summary.or_else(|| title.map(str::to_string));
            let fields = wire
                .display_fields
                .into_iter()
                .map(|field| {
                    to_value_or_null(DisplayFieldValue {
                        label: field.key,
                        value: field.value.unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            apply_display_override(web, summary, fields, None);
        }
        "todowrite" | "todo_write" | "todoread" | "todo_read" => {
            fn deserialize_vec_todo_items_lossy<'de, D>(
                deserializer: D,
            ) -> Result<Vec<TodoItem>, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = Option::<serde_json::Value>::deserialize(deserializer)?;
                let Some(value) = value else {
                    return Ok(Vec::new());
                };
                Ok(serde_json::from_value::<Vec<TodoItem>>(value).unwrap_or_default())
            }

            #[derive(Debug, Default, Deserialize)]
            struct TodoToolMetadataWire {
                #[serde(default, deserialize_with = "deserialize_vec_todo_items_lossy")]
                todos: Vec<TodoItem>,
                #[serde(default, deserialize_with = "deserialize_opt_u64_lossy")]
                count: Option<u64>,
            }

            let wire = serde_json::to_value(metadata)
                .ok()
                .and_then(|value| serde_json::from_value::<TodoToolMetadataWire>(value).ok())
                .unwrap_or_default();

            let todos = wire.todos;
            let summary = title
                .map(str::to_string)
                .or_else(|| wire.count.map(|count| format!("{count} todo items")));
            let fields = todo_summary_fields_from_array(&todos);
            let preview = todo_preview_from_array(&todos);
            apply_display_override(web, summary, fields, preview);
        }
        _ => {}
    }
}

fn apply_history_tool_result_interaction(
    web: &mut serde_json::Value,
    tool_name: &str,
    title: Option<&str>,
    content: &str,
    is_error: bool,
) {
    if tool_name != "question" {
        return;
    }
    let status = if is_error {
        let lower = format!(
            "{} {}",
            title.unwrap_or_default().to_ascii_lowercase(),
            content.to_ascii_lowercase()
        );
        if lower.contains("reject") {
            "rejected"
        } else if lower.contains("cancel") {
            "cancelled"
        } else {
            "error"
        }
    } else {
        "answered"
    };
    let Some(map) = web.as_object_mut() else {
        return;
    };
    map.insert(
        "interaction".to_string(),
        to_value_or_null(QuestionInteractionReadonly {
            interaction_type: "question",
            status: status.to_string(),
            can_reply: false,
            can_reject: false,
        }),
    );
}

fn apply_display_override(
    web: &mut serde_json::Value,
    summary: Option<String>,
    fields: Vec<serde_json::Value>,
    preview: Option<serde_json::Value>,
) {
    let Some(map) = web.as_object_mut() else {
        return;
    };
    let display = map
        .entry("display".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut();
    let Some(display) = display else {
        return;
    };
    if let Some(summary) = summary {
        display.insert("summary".to_string(), serde_json::Value::String(summary));
    }
    if !fields.is_empty() {
        display.insert("fields".to_string(), serde_json::Value::Array(fields));
    }
    if let Some(preview) = preview {
        display.insert("preview".to_string(), preview);
    }
}

fn todo_summary_fields_from_array(todos: &[TodoItem]) -> Vec<serde_json::Value> {
    if todos.is_empty() {
        return Vec::new();
    }
    let mut pending = 0_u64;
    let mut in_progress = 0_u64;
    let mut completed = 0_u64;
    for todo in todos {
        match todo.status.as_deref().unwrap_or("pending") {
            "completed" => completed += 1,
            "in_progress" | "in-progress" | "in progress" => in_progress += 1,
            _ => pending += 1,
        }
    }
    vec![
        to_value_or_null(DisplayFieldValue {
            label: "Count".to_string(),
            value: todos.len().to_string(),
        }),
        to_value_or_null(DisplayFieldValue {
            label: "Pending".to_string(),
            value: pending.to_string(),
        }),
        to_value_or_null(DisplayFieldValue {
            label: "In Progress".to_string(),
            value: in_progress.to_string(),
        }),
        to_value_or_null(DisplayFieldValue {
            label: "Completed".to_string(),
            value: completed.to_string(),
        }),
    ]
}

fn todo_preview_from_array(todos: &[TodoItem]) -> Option<serde_json::Value> {
    if todos.is_empty() {
        return None;
    }
    let lines = todos
        .iter()
        .take(8)
        .filter_map(|todo| {
            let content = todo.content.trim();
            if content.is_empty() {
                return None;
            }
            let status = todo.status.as_deref().unwrap_or("pending");
            Some(format!("- [{}] {}", status, content))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    Some(to_value_or_null(TextPreviewPayload {
        kind: "text",
        text: lines.join("\n"),
        truncated: todos.len() > lines.len(),
    }))
}

// ── Structured detail extraction ──────────────────────────────────────

/// Extract structured detail from tool call input arguments (for ToolStart/ToolEnd).
/// The `input` is the JSON value of the tool call arguments.
fn extract_tool_input_structured(
    tool_name: &str,
    input: &serde_json::Value,
) -> Option<ToolStructuredDetail> {
    #[derive(Debug, Deserialize)]
    struct FilePathOnlyInput {
        #[serde(default, alias = "filePath")]
        file_path: String,
    }

    #[derive(Debug, Deserialize)]
    struct BashCommandInput {
        #[serde(default, alias = "cmd")]
        command: String,
    }

    #[derive(Debug, Deserialize)]
    struct PatternOnlyInput {
        #[serde(default)]
        pattern: String,
    }

    match tool_name {
        "edit" | "multiedit" => {
            let file_path = serde_json::from_value::<FilePathOnlyInput>(input.clone())
                .map(|input| input.file_path)
                .unwrap_or_default();
            Some(ToolStructuredDetail::FileEdit {
                file_path,
                diff_preview: None,
            })
        }
        "write" => {
            let file_path = serde_json::from_value::<FilePathOnlyInput>(input.clone())
                .map(|input| input.file_path)
                .unwrap_or_default();
            Some(ToolStructuredDetail::FileWrite {
                file_path,
                bytes: None,
                lines: None,
                diff_preview: None,
            })
        }
        "read" => {
            let file_path = serde_json::from_value::<FilePathOnlyInput>(input.clone())
                .map(|input| input.file_path)
                .unwrap_or_default();
            Some(ToolStructuredDetail::FileRead {
                file_path,
                total_lines: None,
                truncated: false,
            })
        }
        "bash" => {
            let command_preview = serde_json::from_value::<BashCommandInput>(input.clone())
                .map(|input| input.command)
                .unwrap_or_default();
            Some(ToolStructuredDetail::BashExec {
                command_preview,
                exit_code: None,
                output_preview: None,
                truncated: false,
            })
        }
        "grep" => {
            let pattern = serde_json::from_value::<PatternOnlyInput>(input.clone())
                .map(|input| input.pattern)
                .unwrap_or_default();
            Some(ToolStructuredDetail::Search {
                pattern,
                matches: None,
                truncated: false,
            })
        }
        "glob" => {
            let pattern = serde_json::from_value::<PatternOnlyInput>(input.clone())
                .map(|input| input.pattern)
                .unwrap_or_default();
            Some(ToolStructuredDetail::Search {
                pattern,
                matches: None,
                truncated: false,
            })
        }
        _ => None,
    }
}

/// Extract structured detail from tool result metadata (for ToolResult).
fn extract_tool_result_structured(
    tool_name: &str,
    output: &AgentToolOutput,
) -> Option<ToolStructuredDetail> {
    let meta = &output.metadata;
    match tool_name {
        "edit" | "multiedit" => {
            let file_path = meta_str(meta, "filepath").unwrap_or_default();
            let diff_preview = meta_str(meta, "diff");
            Some(ToolStructuredDetail::FileEdit {
                file_path,
                diff_preview,
            })
        }
        "write" => {
            let file_path = meta_str(meta, "filepath").unwrap_or_default();
            let bytes = meta_u64(meta, "bytes");
            let lines = meta_u64(meta, "lines");
            let diff_preview = meta_str(meta, "diff");
            Some(ToolStructuredDetail::FileWrite {
                file_path,
                bytes,
                lines,
                diff_preview,
            })
        }
        "read" => {
            let file_path = meta_str(meta, "filepath").unwrap_or_default();
            let total_lines = meta_u64(meta, "total_lines");
            let truncated = meta_bool(meta, "truncated");
            Some(ToolStructuredDetail::FileRead {
                file_path,
                total_lines,
                truncated,
            })
        }
        "bash" => {
            let command_preview = String::new(); // command is in tool input, not result metadata
            let exit_code = meta_i64(meta, "exit_code");
            // Use the tool output text as output preview for bash
            let output_preview = if output.output.trim().is_empty() {
                None
            } else {
                Some(output.output.clone())
            };
            let truncated = meta_bool(meta, "truncated");
            Some(ToolStructuredDetail::BashExec {
                command_preview,
                exit_code,
                output_preview,
                truncated,
            })
        }
        "grep" => {
            let pattern = String::new(); // pattern is in tool input
            let matches = meta_u64(meta, "matches");
            let truncated = meta_bool(meta, "truncated");
            Some(ToolStructuredDetail::Search {
                pattern,
                matches,
                truncated,
            })
        }
        "glob" => {
            let pattern = String::new();
            let matches = meta_u64(meta, "count");
            let truncated = meta_bool(meta, "truncated");
            Some(ToolStructuredDetail::Search {
                pattern,
                matches,
                truncated,
            })
        }
        _ => None,
    }
}

// ── Metadata helpers ──────────────────────────────────────────────────

fn meta_str(meta: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn meta_u64(meta: &HashMap<String, serde_json::Value>, key: &str) -> Option<u64> {
    meta.get(key).and_then(|v| v.as_u64())
}

fn meta_i64(meta: &HashMap<String, serde_json::Value>, key: &str) -> Option<i64> {
    meta.get(key).and_then(|v| v.as_i64())
}

fn meta_bool(meta: &HashMap<String, serde_json::Value>, key: &str) -> bool {
    meta.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

#[derive(Serialize)]
struct WebStatusBlock<'a> {
    kind: &'static str,
    tone: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct WebMessageBlock<'a> {
    kind: &'static str,
    role: &'a str,
    phase: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct WebReasoningBlock<'a> {
    kind: &'static str,
    phase: &'a str,
    text: &'a str,
}

#[derive(Serialize)]
struct WebToolDisplayField {
    label: String,
    value: String,
}

#[derive(Serialize)]
struct WebToolDisplayPreview {
    kind: String,
    text: String,
    truncated: bool,
}

#[derive(Serialize)]
struct WebToolDisplay {
    header: String,
    summary: Option<String>,
    fields: Vec<WebToolDisplayField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<WebToolDisplayPreview>,
}

#[derive(Serialize)]
struct WebToolBlock<'a> {
    kind: &'static str,
    name: &'a str,
    phase: &'a str,
    detail: Option<&'a str>,
    display: WebToolDisplay,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct WebSessionEventField {
    label: String,
    value: String,
    tone: Option<String>,
}

#[derive(Serialize)]
struct WebSessionEventBlock {
    kind: &'static str,
    event: String,
    title: String,
    status: Option<String>,
    summary: Option<String>,
    fields: Vec<WebSessionEventField>,
    body: Option<String>,
}

#[derive(Serialize)]
struct WebQueueItemDisplay {
    summary: String,
}

#[derive(Serialize)]
struct WebQueueItemBlock {
    kind: &'static str,
    position: usize,
    text: String,
    display: WebQueueItemDisplay,
}

#[derive(Serialize)]
struct WebSchedulerDecisionSpec {
    version: String,
    show_header_divider: bool,
    field_order: String,
    field_label_emphasis: String,
    status_palette: String,
    section_spacing: String,
    update_policy: String,
}

#[derive(Serialize)]
struct WebSchedulerDecisionField {
    label: String,
    value: String,
    tone: Option<String>,
}

#[derive(Serialize)]
struct WebSchedulerDecisionSection {
    title: String,
    body: String,
}

#[derive(Serialize)]
struct WebSchedulerDecision {
    kind: String,
    title: String,
    spec: WebSchedulerDecisionSpec,
    fields: Vec<WebSchedulerDecisionField>,
    sections: Vec<WebSchedulerDecisionSection>,
}

#[derive(Serialize)]
struct WebSchedulerStageBlock {
    kind: &'static str,
    stage_id: Option<String>,
    profile: Option<String>,
    stage: String,
    title: String,
    text: String,
    stage_index: Option<u64>,
    stage_total: Option<u64>,
    step: Option<u64>,
    status: Option<String>,
    focus: Option<String>,
    last_event: Option<String>,
    waiting_on: Option<String>,
    activity: Option<String>,
    available_skill_count: Option<u64>,
    available_agent_count: Option<u64>,
    available_category_count: Option<u64>,
    active_skills: Vec<String>,
    active_agents: Vec<String>,
    active_categories: Vec<String>,
    done_agent_count: u32,
    total_agent_count: u32,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    child_session_id: Option<String>,
    decision: Option<WebSchedulerDecision>,
}

#[derive(Serialize)]
struct WebInspectEvent {
    ts: i64,
    event_type: String,
    execution_id: Option<String>,
    stage_id: Option<String>,
}

#[derive(Serialize)]
struct WebInspectBlock {
    kind: &'static str,
    stage_ids: Vec<String>,
    filter_stage_id: Option<String>,
    events: Vec<WebInspectEvent>,
}

#[derive(Serialize)]
struct DisplayFieldValue {
    label: String,
    value: String,
}

#[derive(Serialize)]
struct QuestionInteractionReadonly {
    #[serde(rename = "type")]
    interaction_type: &'static str,
    status: String,
    can_reply: bool,
    can_reject: bool,
}

#[derive(Serialize)]
struct TextPreviewPayload {
    kind: &'static str,
    text: String,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebToolStructuredDetail {
    FileEdit {
        file_path: String,
        diff_preview: Option<String>,
    },
    FileWrite {
        file_path: String,
        bytes: Option<u64>,
        lines: Option<u64>,
        diff_preview: Option<String>,
    },
    FileRead {
        file_path: String,
        total_lines: Option<u64>,
        truncated: bool,
    },
    BashExec {
        command_preview: String,
        exit_code: Option<i64>,
        output_preview: Option<String>,
        truncated: bool,
    },
    Search {
        pattern: String,
        matches: Option<u64>,
        truncated: bool,
    },
    Generic,
}

fn to_value_or_null<T: Serialize>(value: T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

pub fn output_block_to_web(block: &OutputBlock) -> serde_json::Value {
    match block {
        OutputBlock::Status(StatusBlock { tone, text }) => to_value_or_null(WebStatusBlock {
            kind: "status",
            tone: tone_to_web(tone),
            text,
        }),
        OutputBlock::Message(MessageBlock { role, phase, text }) => {
            to_value_or_null(WebMessageBlock {
                kind: "message",
                role: role_to_web(role),
                phase: phase_to_web(phase),
                text,
            })
        }
        OutputBlock::Reasoning(ReasoningBlock { phase, text }) => {
            to_value_or_null(WebReasoningBlock {
                kind: "reasoning",
                phase: phase_to_web(phase),
                text,
            })
        }
        OutputBlock::Tool(ToolBlock {
            name,
            phase,
            detail,
            structured,
        }) => {
            let tool = ToolBlock {
                name: name.clone(),
                phase: *phase,
                detail: detail.clone(),
                structured: structured.clone(),
            };
            to_value_or_null(WebToolBlock {
                kind: "tool",
                name,
                phase: tool_phase_to_web(phase),
                detail: detail.as_deref(),
                display: WebToolDisplay {
                    header: tool_web_header(&tool),
                    summary: tool_web_summary(&tool),
                    fields: tool_web_fields(&tool)
                        .into_iter()
                        .map(|field| WebToolDisplayField {
                            label: field.label,
                            value: field.value,
                        })
                        .collect(),
                    preview: tool_web_preview(&tool).map(|preview| WebToolDisplayPreview {
                        kind: preview.kind,
                        text: preview.text,
                        truncated: preview.truncated,
                    }),
                },
                structured: structured.as_ref().map(structured_to_web),
            })
        }
        OutputBlock::SessionEvent(SessionEventBlock {
            event,
            title,
            status,
            summary,
            fields,
            body,
        }) => to_value_or_null(WebSessionEventBlock {
            kind: "session_event",
            event: event.clone(),
            title: title.clone(),
            status: status.clone(),
            summary: summary.clone(),
            fields: fields
                .iter()
                .map(|field| WebSessionEventField {
                    label: field.label.clone(),
                    value: field.value.clone(),
                    tone: field.tone.clone(),
                })
                .collect(),
            body: body.clone(),
        }),
        OutputBlock::QueueItem(QueueItemBlock { position, text }) => {
            to_value_or_null(WebQueueItemBlock {
                kind: "queue_item",
                position: *position,
                text: text.clone(),
                display: WebQueueItemDisplay {
                    summary: format!("Queued [{}] {}", position, text),
                },
            })
        }
        OutputBlock::SchedulerStage(stage) => {
            let decision = stage
                .decision
                .as_ref()
                .map(|decision| WebSchedulerDecision {
                    kind: decision.kind.clone(),
                    title: decision.title.clone(),
                    spec: WebSchedulerDecisionSpec {
                        version: decision.spec.version.clone(),
                        show_header_divider: decision.spec.show_header_divider,
                        field_order: decision.spec.field_order.clone(),
                        field_label_emphasis: decision.spec.field_label_emphasis.clone(),
                        status_palette: decision.spec.status_palette.clone(),
                        section_spacing: decision.spec.section_spacing.clone(),
                        update_policy: decision.spec.update_policy.clone(),
                    },
                    fields: decision
                        .fields
                        .iter()
                        .map(|field| WebSchedulerDecisionField {
                            label: field.label.clone(),
                            value: field.value.clone(),
                            tone: field.tone.clone(),
                        })
                        .collect(),
                    sections: decision
                        .sections
                        .iter()
                        .map(|section| WebSchedulerDecisionSection {
                            title: section.title.clone(),
                            body: section.body.clone(),
                        })
                        .collect(),
                });

            to_value_or_null(WebSchedulerStageBlock {
                kind: "scheduler_stage",
                stage_id: stage.stage_id.clone(),
                profile: stage.profile.clone(),
                stage: stage.stage.clone(),
                title: stage.title.clone(),
                text: stage.text.clone(),
                stage_index: stage.stage_index,
                stage_total: stage.stage_total,
                step: stage.step,
                status: stage.status.clone(),
                focus: stage.focus.clone(),
                last_event: stage.last_event.clone(),
                waiting_on: stage.waiting_on.clone(),
                activity: stage.activity.clone(),
                available_skill_count: stage.available_skill_count,
                available_agent_count: stage.available_agent_count,
                available_category_count: stage.available_category_count,
                active_skills: stage.active_skills.clone(),
                active_agents: stage.active_agents.clone(),
                active_categories: stage.active_categories.clone(),
                done_agent_count: stage.done_agent_count,
                total_agent_count: stage.total_agent_count,
                prompt_tokens: stage.prompt_tokens,
                completion_tokens: stage.completion_tokens,
                reasoning_tokens: stage.reasoning_tokens,
                cache_read_tokens: stage.cache_read_tokens,
                cache_write_tokens: stage.cache_write_tokens,
                child_session_id: stage.child_session_id.clone(),
                decision,
            })
        }
        OutputBlock::Inspect(inspect) => to_value_or_null(WebInspectBlock {
            kind: "inspect",
            stage_ids: inspect.stage_ids.clone(),
            filter_stage_id: inspect.filter_stage_id.clone(),
            events: inspect
                .events
                .iter()
                .map(|e| WebInspectEvent {
                    ts: e.ts,
                    event_type: e.event_type.clone(),
                    execution_id: e.execution_id.clone(),
                    stage_id: e.stage_id.clone(),
                })
                .collect(),
        }),
    }
}

pub fn output_blocks_to_web(blocks: &[OutputBlock]) -> Vec<serde_json::Value> {
    blocks.iter().map(output_block_to_web).collect()
}

pub fn render_agent_event_to_web(
    event: AgentRenderEvent,
    config: AgentPresenterConfig,
) -> Option<serde_json::Value> {
    let tool_id = match &event {
        AgentRenderEvent::ToolStart { id, .. }
        | AgentRenderEvent::ToolProgress { id, .. }
        | AgentRenderEvent::ToolEnd { id, .. } => Some(id.clone()),
        AgentRenderEvent::ToolResult { tool_call_id, .. }
        | AgentRenderEvent::ToolError { tool_call_id, .. } => Some(tool_call_id.clone()),
        _ => None,
    };

    let mut web = output_block_to_web(&map_render_event_to_block(event, config)?);
    if let (Some(id), serde_json::Value::Object(map)) = (tool_id, &mut web) {
        map.insert("id".to_string(), serde_json::Value::String(id));
    }
    Some(web)
}

fn tone_to_web(tone: &BlockTone) -> &'static str {
    match tone {
        BlockTone::Title => "title",
        BlockTone::Normal => "normal",
        BlockTone::Muted => "muted",
        BlockTone::Success => "success",
        BlockTone::Warning => "warning",
        BlockTone::Error => "error",
    }
}

fn role_to_web(role: &Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
    }
}

fn phase_to_web(phase: &MessagePhase) -> &'static str {
    match phase {
        MessagePhase::Start => "start",
        MessagePhase::Delta => "delta",
        MessagePhase::End => "end",
        MessagePhase::Full => "full",
    }
}

fn tool_phase_to_web(phase: &ToolPhase) -> &'static str {
    match phase {
        ToolPhase::Start => "start",
        ToolPhase::Running => "running",
        ToolPhase::Done => "done",
        ToolPhase::Error => "error",
    }
}

fn structured_to_web(detail: &ToolStructuredDetail) -> serde_json::Value {
    match detail {
        ToolStructuredDetail::FileEdit {
            file_path,
            diff_preview,
        } => to_value_or_null(WebToolStructuredDetail::FileEdit {
            file_path: file_path.clone(),
            diff_preview: diff_preview.clone(),
        }),
        ToolStructuredDetail::FileWrite {
            file_path,
            bytes,
            lines,
            diff_preview,
        } => to_value_or_null(WebToolStructuredDetail::FileWrite {
            file_path: file_path.clone(),
            bytes: *bytes,
            lines: *lines,
            diff_preview: diff_preview.clone(),
        }),
        ToolStructuredDetail::FileRead {
            file_path,
            total_lines,
            truncated,
        } => to_value_or_null(WebToolStructuredDetail::FileRead {
            file_path: file_path.clone(),
            total_lines: *total_lines,
            truncated: *truncated,
        }),
        ToolStructuredDetail::BashExec {
            command_preview,
            exit_code,
            output_preview,
            truncated,
        } => to_value_or_null(WebToolStructuredDetail::BashExec {
            command_preview: command_preview.clone(),
            exit_code: *exit_code,
            output_preview: output_preview.clone(),
            truncated: *truncated,
        }),
        ToolStructuredDetail::Search {
            pattern,
            matches,
            truncated,
        } => to_value_or_null(WebToolStructuredDetail::Search {
            pattern: pattern.clone(),
            matches: *matches,
            truncated: *truncated,
        }),
        ToolStructuredDetail::Generic => to_value_or_null(WebToolStructuredDetail::Generic),
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }

    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= max_len {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocode_agent::AgentToolOutput;
    use std::collections::HashMap;

    #[test]
    fn maps_render_events_to_blocks() {
        let block = map_render_event_to_block(
            AgentRenderEvent::ToolError {
                tool_call_id: "tc1".to_string(),
                tool_name: "bash".to_string(),
                error: "failed".to_string(),
                metadata: HashMap::new(),
            },
            AgentPresenterConfig::default(),
        )
        .expect("tool error should map to block");

        match block {
            OutputBlock::Tool(tool) => {
                assert_eq!(tool.name, "bash");
                assert_eq!(tool.phase, ToolPhase::Error);
                assert_eq!(tool.detail.as_deref(), Some("failed"));
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn presents_outcome_and_preserves_tokens() {
        let outcome = AgentRenderOutcome {
            events: vec![
                AgentRenderEvent::AssistantStart,
                AgentRenderEvent::AssistantDelta("hello".to_string()),
                AgentRenderEvent::AssistantEnd,
                AgentRenderEvent::ToolResult {
                    tool_call_id: "t1".to_string(),
                    tool_name: "read".to_string(),
                    output: AgentToolOutput {
                        output: "ok".to_string(),
                        title: String::new(),
                        metadata: HashMap::new(),
                    },
                },
            ],
            prompt_tokens: 12,
            completion_tokens: 34,
            stream_error: None,
        };

        let rendered = present_agent_outcome(outcome, AgentPresenterConfig::default());
        assert_eq!(rendered.blocks.len(), 4);
        assert_eq!(rendered.prompt_tokens, 12);
        assert_eq!(rendered.completion_tokens, 34);
        assert!(rendered.stream_error.is_none());
    }

    #[test]
    fn converts_output_block_to_web_shape() {
        let block = OutputBlock::Message(MessageBlock::delta(Role::Assistant, "hello"));
        let web = output_block_to_web(&block);
        assert_eq!(web.get("kind").and_then(|v| v.as_str()), Some("message"));
        assert_eq!(web.get("phase").and_then(|v| v.as_str()), Some("delta"));
        assert_eq!(web.get("role").and_then(|v| v.as_str()), Some("assistant"));
    }

    #[test]
    fn queue_item_block_to_web_shape() {
        let web = output_block_to_web(&OutputBlock::QueueItem(
            crate::output_blocks::QueueItemBlock {
                position: 4,
                text: "finish docs sync".to_string(),
            },
        ));
        assert_eq!(
            web.get("kind").and_then(|value| value.as_str()),
            Some("queue_item")
        );
        assert_eq!(
            web.get("position").and_then(|value| value.as_u64()),
            Some(4)
        );
        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("summary"))
                .and_then(|value| value.as_str()),
            Some("Queued [4] finish docs sync")
        );
    }

    #[test]
    fn scheduler_stage_web_shape_includes_decision_block() {
        let web = output_block_to_web(&OutputBlock::SchedulerStage(Box::new(
            SchedulerStageBlock {
                stage_id: None,
                profile: Some("atlas".to_string()),
                stage: "coordination-gate".to_string(),
                title: "Atlas · Coordination Gate".to_string(),
                text: String::new(),
                stage_index: Some(3),
                stage_total: Some(4),
                step: Some(2),
                status: Some("waiting".to_string()),
                focus: Some("verification".to_string()),
                last_event: Some("Question started".to_string()),
                waiting_on: Some("user".to_string()),
                activity: Some("Question (1)".to_string()),
                loop_budget: None,
                available_skill_count: Some(4),
                available_agent_count: Some(3),
                available_category_count: Some(2),
                active_skills: vec!["frontend-ui-ux".to_string()],
                active_agents: vec!["build".to_string()],
                active_categories: vec!["frontend".to_string()],
                done_agent_count: 0,
                total_agent_count: 0,
                prompt_tokens: Some(1200),
                completion_tokens: Some(320),
                reasoning_tokens: Some(40),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                decision: Some(crate::output_blocks::SchedulerDecisionBlock {
                    kind: "gate".to_string(),
                    title: "Decision".to_string(),
                    spec: crate::output_blocks::default_scheduler_decision_render_spec(),
                    fields: vec![crate::output_blocks::SchedulerDecisionField {
                        label: "Outcome".to_string(),
                        value: "continue".to_string(),
                        tone: Some("status".to_string()),
                    }],
                    sections: vec![crate::output_blocks::SchedulerDecisionSection {
                        title: "Final Response".to_string(),
                        body: "Done.".to_string(),
                    }],
                }),
                child_session_id: None,
            },
        )));
        assert_eq!(
            web.get("kind").and_then(|value| value.as_str()),
            Some("scheduler_stage")
        );
        let decision = web.get("decision").expect("decision should exist");
        assert_eq!(
            web.get("available_skill_count")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
        assert_eq!(
            web.get("active_agents")
                .and_then(|value| value.as_array())
                .and_then(|values| values.first())
                .and_then(|value| value.as_str()),
            Some("build")
        );
        assert_eq!(
            web.get("prompt_tokens").and_then(|value| value.as_u64()),
            Some(1200)
        );
        assert_eq!(
            web.get("completion_tokens")
                .and_then(|value| value.as_u64()),
            Some(320)
        );
        assert_eq!(
            decision.get("title").and_then(|value| value.as_str()),
            Some("Decision")
        );
        assert_eq!(
            decision
                .get("fields")
                .and_then(|value| value.as_array())
                .and_then(|fields| fields.first())
                .and_then(|field| field.get("label"))
                .and_then(|value| value.as_str()),
            Some("Outcome")
        );
    }

    #[test]
    fn render_agent_event_to_web_includes_tool_id() {
        let web = render_agent_event_to_web(
            AgentRenderEvent::ToolStart {
                id: "tool_123".to_string(),
                name: "read".to_string(),
            },
            AgentPresenterConfig::default(),
        )
        .expect("tool event should produce web block");
        assert_eq!(web.get("kind").and_then(|v| v.as_str()), Some("tool"));
        assert_eq!(web.get("id").and_then(|v| v.as_str()), Some("tool_123"));
    }

    #[test]
    fn history_tool_result_to_web_preserves_tool_id() {
        let web = history_tool_result_to_web(
            "call_123",
            "bash",
            Some("stdout"),
            "ok",
            false,
            &HashMap::new(),
        );
        assert_eq!(web.get("kind").and_then(|v| v.as_str()), Some("tool"));
        assert_eq!(web.get("id").and_then(|v| v.as_str()), Some("call_123"));
    }

    #[test]
    fn history_question_result_to_web_uses_display_fields() {
        let mut metadata = HashMap::new();
        metadata.insert("display.summary".to_string(), json!("1 question answered"));
        metadata.insert(
            "display.fields".to_string(),
            json!([{ "key": "Scope", "value": "Proceed" }]),
        );

        let web = history_tool_result_to_web(
            "call_question_1",
            "question",
            Some("User response received"),
            "{\n  \"answers\": [\"Proceed\"]\n}",
            false,
            &metadata,
        );

        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("summary"))
                .and_then(|value| value.as_str()),
            Some("1 question answered")
        );
        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("fields"))
                .and_then(|value| value.as_array())
                .and_then(|fields| fields.first())
                .and_then(|field| field.get("label"))
                .and_then(|value| value.as_str()),
            Some("Scope")
        );
    }

    #[test]
    fn history_todo_result_to_web_uses_preview_list() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "todos".to_string(),
            json!([
                { "content": "Add tests", "status": "pending", "priority": "high" },
                { "content": "Refactor server route", "status": "completed", "priority": "medium" }
            ]),
        );
        metadata.insert("count".to_string(), json!(2));

        let web = history_tool_result_to_web(
            "call_todo_1",
            "todo_write",
            Some("Updated Todo List (2 items)"),
            "irrelevant",
            false,
            &metadata,
        );

        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("summary"))
                .and_then(|value| value.as_str()),
            Some("Updated Todo List (2 items)")
        );
        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("preview"))
                .and_then(|value| value.get("text"))
                .and_then(|value| value.as_str()),
            Some("- [pending] Add tests\n- [completed] Refactor server route")
        );
    }

    #[test]
    fn history_session_event_to_web_serializes_event_card() {
        let web = history_session_event_to_web(
            "subtask",
            "Subtask · inspect scheduler",
            Some("pending"),
            Some("Subtask `task_1` is `pending`.".to_string()),
            vec![
                ("ID".to_string(), "task_1".to_string(), None),
                (
                    "Description".to_string(),
                    "inspect scheduler".to_string(),
                    None,
                ),
            ],
            None,
        );

        assert_eq!(
            web.get("kind").and_then(|value| value.as_str()),
            Some("session_event")
        );
        assert_eq!(
            web.get("event").and_then(|value| value.as_str()),
            Some("subtask")
        );
        assert_eq!(
            web.get("fields")
                .and_then(|value| value.as_array())
                .map(|fields| fields.len()),
            Some(2)
        );
    }

    #[test]
    fn history_question_result_to_web_marks_answered_interaction() {
        let web = history_tool_result_to_web(
            "call_question_2",
            "question",
            Some("User response received"),
            "{\"answers\":[\"Proceed\"]}",
            false,
            &HashMap::new(),
        );
        assert_eq!(
            web.get("interaction")
                .and_then(|value| value.get("status"))
                .and_then(|value| value.as_str()),
            Some("answered")
        );
    }

    // ── Phase 2: Structured extraction tests ─────────────────────────

    #[test]
    fn tool_result_edit_extracts_structured_diff() {
        let mut meta = HashMap::new();
        meta.insert("filepath".to_string(), json!("/tmp/src/main.rs"));
        meta.insert(
            "diff".to_string(),
            json!("--- a/main.rs\n+++ b/main.rs\n@@ -1 +1 @@\n-old\n+new"),
        );
        meta.insert("replacements".to_string(), json!(1));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc1".to_string(),
                tool_name: "edit".to_string(),
                output: AgentToolOutput {
                    output: "edited".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                assert_eq!(tool.phase, ToolPhase::Done);
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::FileEdit {
                        file_path,
                        diff_preview,
                    } => {
                        assert_eq!(file_path, "/tmp/src/main.rs");
                        assert!(diff_preview.is_some());
                        assert!(diff_preview.unwrap().contains("+new"));
                    }
                    _ => panic!("expected FileEdit structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_result_bash_extracts_exit_code_and_output() {
        let mut meta = HashMap::new();
        meta.insert("exit_code".to_string(), json!(0));
        meta.insert("truncated".to_string(), json!(false));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc2".to_string(),
                tool_name: "bash".to_string(),
                output: AgentToolOutput {
                    output: "hello world\n".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::BashExec {
                        exit_code,
                        output_preview,
                        truncated,
                        ..
                    } => {
                        assert_eq!(exit_code, Some(0));
                        assert!(output_preview.is_some());
                        assert!(!truncated);
                    }
                    _ => panic!("expected BashExec structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_result_write_extracts_bytes_and_lines() {
        let mut meta = HashMap::new();
        meta.insert("filepath".to_string(), json!("/tmp/new_file.rs"));
        meta.insert("bytes".to_string(), json!(256));
        meta.insert("lines".to_string(), json!(12));
        meta.insert("diff".to_string(), json!("+line1\n+line2"));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc3".to_string(),
                tool_name: "write".to_string(),
                output: AgentToolOutput {
                    output: "written".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::FileWrite {
                        file_path,
                        bytes,
                        lines,
                        diff_preview,
                    } => {
                        assert_eq!(file_path, "/tmp/new_file.rs");
                        assert_eq!(bytes, Some(256));
                        assert_eq!(lines, Some(12));
                        assert!(diff_preview.is_some());
                    }
                    _ => panic!("expected FileWrite structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_result_read_extracts_total_lines() {
        let mut meta = HashMap::new();
        meta.insert("filepath".to_string(), json!("/tmp/read.rs"));
        meta.insert("total_lines".to_string(), json!(150));
        meta.insert("truncated".to_string(), json!(true));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc4".to_string(),
                tool_name: "read".to_string(),
                output: AgentToolOutput {
                    output: "contents".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::FileRead {
                        file_path,
                        total_lines,
                        truncated,
                    } => {
                        assert_eq!(file_path, "/tmp/read.rs");
                        assert_eq!(total_lines, Some(150));
                        assert!(truncated);
                    }
                    _ => panic!("expected FileRead structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_result_grep_extracts_matches() {
        let mut meta = HashMap::new();
        meta.insert("matches".to_string(), json!(42));
        meta.insert("truncated".to_string(), json!(false));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc5".to_string(),
                tool_name: "grep".to_string(),
                output: AgentToolOutput {
                    output: "results".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::Search {
                        matches, truncated, ..
                    } => {
                        assert_eq!(matches, Some(42));
                        assert!(!truncated);
                    }
                    _ => panic!("expected Search structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_end_edit_extracts_file_path_from_input() {
        let block = map_render_event_to_block(
            AgentRenderEvent::ToolEnd {
                id: "te1".to_string(),
                name: "edit".to_string(),
                input: json!({
                    "file_path": "/src/lib.rs",
                    "old_string": "old",
                    "new_string": "new"
                }),
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::FileEdit { file_path, .. } => {
                        assert_eq!(file_path, "/src/lib.rs");
                    }
                    _ => panic!("expected FileEdit structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_end_bash_extracts_command_from_input() {
        let block = map_render_event_to_block(
            AgentRenderEvent::ToolEnd {
                id: "te2".to_string(),
                name: "bash".to_string(),
                input: json!({
                    "command": "cargo test --all"
                }),
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                let structured = tool.structured.expect("should have structured detail");
                match structured {
                    ToolStructuredDetail::BashExec {
                        command_preview, ..
                    } => {
                        assert_eq!(command_preview, "cargo test --all");
                    }
                    _ => panic!("expected BashExec structured detail"),
                }
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn tool_result_unknown_tool_has_no_structured() {
        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc6".to_string(),
                tool_name: "custom_mcp_tool".to_string(),
                output: AgentToolOutput {
                    output: "result".to_string(),
                    title: String::new(),
                    metadata: HashMap::new(),
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        match block {
            OutputBlock::Tool(tool) => {
                assert!(tool.structured.is_none());
            }
            _ => panic!("expected tool block"),
        }
    }

    #[test]
    fn web_output_includes_structured_for_tool_result() {
        let mut meta = HashMap::new();
        meta.insert("filepath".to_string(), json!("/src/main.rs"));
        meta.insert("diff".to_string(), json!("+line"));

        let block = map_render_event_to_block(
            AgentRenderEvent::ToolResult {
                tool_call_id: "tc7".to_string(),
                tool_name: "edit".to_string(),
                output: AgentToolOutput {
                    output: "done".to_string(),
                    title: String::new(),
                    metadata: meta,
                },
            },
            AgentPresenterConfig::default(),
        )
        .unwrap();

        let web = output_block_to_web(&block);
        let structured = web
            .get("structured")
            .expect("web output should have structured");
        assert_eq!(
            structured.get("type").and_then(|v| v.as_str()),
            Some("file_edit")
        );
        assert_eq!(
            structured.get("file_path").and_then(|v| v.as_str()),
            Some("/src/main.rs")
        );
        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("header"))
                .and_then(|value| value.as_str()),
            Some("Edit(/src/main.rs)")
        );
        assert_eq!(
            web.get("display")
                .and_then(|value| value.get("preview"))
                .and_then(|value| value.get("kind"))
                .and_then(|value| value.as_str()),
            Some("diff")
        );
    }
}
