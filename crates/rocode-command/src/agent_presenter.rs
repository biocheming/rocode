use crate::output_blocks::{
    BlockTone, MessageBlock, MessagePhase, MessageRole, OutputBlock, StatusBlock, ToolBlock,
    ToolPhase, ToolStructuredDetail,
};
use rocode_agent::{AgentRenderEvent, AgentRenderOutcome, AgentToolOutput};
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
        AgentRenderEvent::AssistantStart => Some(OutputBlock::Message(MessageBlock::start(
            MessageRole::Assistant,
        ))),
        AgentRenderEvent::AssistantDelta(text) => {
            if text.is_empty() {
                None
            } else {
                Some(OutputBlock::Message(MessageBlock::delta(
                    MessageRole::Assistant,
                    text,
                )))
            }
        }
        AgentRenderEvent::AssistantEnd => Some(OutputBlock::Message(MessageBlock::end(
            MessageRole::Assistant,
        ))),
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
    }
}

// ── Structured detail extraction ──────────────────────────────────────

/// Extract structured detail from tool call input arguments (for ToolStart/ToolEnd).
/// The `input` is the JSON value of the tool call arguments.
fn extract_tool_input_structured(
    tool_name: &str,
    input: &serde_json::Value,
) -> Option<ToolStructuredDetail> {
    match tool_name {
        "edit" | "multiedit" => {
            let file_path = input
                .get("file_path")
                .or_else(|| input.get("filePath"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolStructuredDetail::FileEdit {
                file_path,
                diff_preview: None,
            })
        }
        "write" => {
            let file_path = input
                .get("file_path")
                .or_else(|| input.get("filePath"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolStructuredDetail::FileWrite {
                file_path,
                bytes: None,
                lines: None,
                diff_preview: None,
            })
        }
        "read" => {
            let file_path = input
                .get("file_path")
                .or_else(|| input.get("filePath"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolStructuredDetail::FileRead {
                file_path,
                total_lines: None,
                truncated: false,
            })
        }
        "bash" => {
            let command_preview = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolStructuredDetail::BashExec {
                command_preview,
                exit_code: None,
                output_preview: None,
                truncated: false,
            })
        }
        "grep" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ToolStructuredDetail::Search {
                pattern,
                matches: None,
                truncated: false,
            })
        }
        "glob" => {
            let pattern = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
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

pub fn output_block_to_web(block: &OutputBlock) -> serde_json::Value {
    match block {
        OutputBlock::Status(StatusBlock { tone, text }) => json!({
            "kind": "status",
            "tone": tone_to_web(tone),
            "text": text,
        }),
        OutputBlock::Message(MessageBlock { role, phase, text }) => json!({
            "kind": "message",
            "role": role_to_web(role),
            "phase": phase_to_web(phase),
            "text": text,
        }),
        OutputBlock::Tool(ToolBlock {
            name,
            phase,
            detail,
            structured,
        }) => {
            let mut obj = serde_json::json!({
                "kind": "tool",
                "name": name,
                "phase": tool_phase_to_web(phase),
                "detail": detail,
            });
            if let Some(ref s) = structured {
                if let serde_json::Value::Object(ref mut map) = obj {
                    map.insert("structured".to_string(), structured_to_web(s));
                }
            }
            obj
        }
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

fn role_to_web(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
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
        } => json!({
            "type": "file_edit",
            "file_path": file_path,
            "diff_preview": diff_preview,
        }),
        ToolStructuredDetail::FileWrite {
            file_path,
            bytes,
            lines,
            diff_preview,
        } => json!({
            "type": "file_write",
            "file_path": file_path,
            "bytes": bytes,
            "lines": lines,
            "diff_preview": diff_preview,
        }),
        ToolStructuredDetail::FileRead {
            file_path,
            total_lines,
            truncated,
        } => json!({
            "type": "file_read",
            "file_path": file_path,
            "total_lines": total_lines,
            "truncated": truncated,
        }),
        ToolStructuredDetail::BashExec {
            command_preview,
            exit_code,
            output_preview,
            truncated,
        } => json!({
            "type": "bash_exec",
            "command_preview": command_preview,
            "exit_code": exit_code,
            "output_preview": output_preview,
            "truncated": truncated,
        }),
        ToolStructuredDetail::Search {
            pattern,
            matches,
            truncated,
        } => json!({
            "type": "search",
            "pattern": pattern,
            "matches": matches,
            "truncated": truncated,
        }),
        ToolStructuredDetail::Generic => json!({
            "type": "generic",
        }),
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
        let block = OutputBlock::Message(MessageBlock::delta(MessageRole::Assistant, "hello"));
        let web = output_block_to_web(&block);
        assert_eq!(web.get("kind").and_then(|v| v.as_str()), Some("message"));
        assert_eq!(web.get("phase").and_then(|v| v.as_str()), Some("delta"));
        assert_eq!(web.get("role").and_then(|v| v.as_str()), Some("assistant"));
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
    }
}
