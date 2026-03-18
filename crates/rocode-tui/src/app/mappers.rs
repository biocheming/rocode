use super::*;

use rocode_core::contracts::mcp::McpConnectionStatusWire;

pub(super) fn apply_incremental_session_sync(
    session_ctx: &mut crate::context::SessionContext,
    session_id: &str,
    session: &SessionInfo,
    mapped_messages: Vec<Message>,
) {
    session_ctx.upsert_session(map_api_session(session));
    session_ctx.upsert_messages_incremental(session_id, mapped_messages);

    if let Some(revert_info) = session.revert.as_ref().map(map_api_revert) {
        session_ctx
            .revert
            .insert(session_id.to_string(), revert_info);
    } else {
        session_ctx.revert.remove(session_id);
    }
}

pub(super) fn map_api_session(session: &SessionInfo) -> Session {
    Session {
        id: session.id.clone(),
        title: session.title.clone(),
        created_at: Utc
            .timestamp_millis_opt(session.time.created)
            .single()
            .unwrap_or_else(Utc::now),
        updated_at: Utc
            .timestamp_millis_opt(session.time.updated)
            .single()
            .unwrap_or_else(Utc::now),
        parent_id: session.parent_id.clone(),
        share: None,
        metadata: session.metadata.clone(),
    }
}

pub(super) fn map_api_message(message: &MessageInfo) -> Message {
    let keep_synthetic_text = message.mode.as_deref() == Some("compaction");
    let parts: Vec<ContextMessagePart> = message
        .parts
        .iter()
        .filter_map(|part| map_api_message_part(part, keep_synthetic_text))
        .collect();

    Message {
        id: message.id.clone(),
        role: match message.role.as_str() {
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        },
        content: parts
            .iter()
            .map(message_part_text)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        created_at: Utc
            .timestamp_millis_opt(message.created_at)
            .single()
            .unwrap_or_else(Utc::now),
        completed_at: message
            .completed_at
            .and_then(|ts| Utc.timestamp_millis_opt(ts).single()),
        agent: message.agent.clone(),
        model: message.model.clone(),
        mode: message.mode.clone(),
        finish: message.finish.clone(),
        error: message.error.clone(),
        cost: message.cost,
        tokens: TokenUsage {
            input: message.tokens.input,
            output: message.tokens.output,
            reasoning: message.tokens.reasoning,
            cache_read: message.tokens.cache_read,
            cache_write: message.tokens.cache_write,
        },
        metadata: message.metadata.clone(),
        parts,
    }
}

pub(super) fn map_api_revert(revert: &SessionRevertInfo) -> RevertInfo {
    RevertInfo {
        message_id: revert.message_id.clone(),
        part_id: revert.part_id.clone(),
        snapshot: revert.snapshot.clone(),
        diff: revert.diff.clone(),
    }
}

fn map_api_message_part(
    part: &crate::api::MessagePart,
    keep_synthetic_text: bool,
) -> Option<ContextMessagePart> {
    if let Some(text) = &part.text {
        if part.ignored == Some(true) {
            return None;
        }
        if part.part_type == "reasoning" {
            return Some(ContextMessagePart::Reasoning { text: text.clone() });
        }
        // Skip synthetic text parts (auto-continue prompts, etc.)
        if part.synthetic == Some(true) && !keep_synthetic_text {
            return None;
        }
        return Some(ContextMessagePart::Text { text: text.clone() });
    }

    if let Some(file) = &part.file {
        return Some(ContextMessagePart::File {
            path: file.filename.clone(),
            mime: file.mime.clone(),
        });
    }

    if let Some(tool_call) = &part.tool_call {
        let arguments = if let Some(value) = tool_call.input.as_str() {
            value.to_string()
        } else {
            tool_call.input.to_string()
        };
        return Some(ContextMessagePart::ToolCall {
            id: tool_call.id.clone(),
            name: tool_call.name.clone(),
            arguments,
        });
    }

    if let Some(tool_result) = &part.tool_result {
        return Some(ContextMessagePart::ToolResult {
            id: tool_result.tool_call_id.clone(),
            result: tool_result.content.clone(),
            is_error: tool_result.is_error,
            title: tool_result.title.clone(),
            metadata: tool_result.metadata.clone(),
        });
    }

    None
}

fn message_part_text(part: &ContextMessagePart) -> String {
    match part {
        ContextMessagePart::Text { text } => text.clone(),
        ContextMessagePart::Reasoning { text } => format!("[reasoning] {}", text),
        ContextMessagePart::ToolCall {
            name, arguments, ..
        } => format!("[tool:{}] {}", name, arguments),
        ContextMessagePart::ToolResult {
            result, is_error, ..
        } => {
            if *is_error {
                return format!("[tool-error] {}", result);
            }
            format!("[tool-result] {}", result)
        }
        ContextMessagePart::File { path, .. } => format!("[file] {}", path),
        ContextMessagePart::Image { url } => format!("[image] {}", url),
    }
}

pub(super) fn infer_task_kind_from_message(message: &Message) -> TaskKind {
    let Some(last_part) = message.parts.last() else {
        return TaskKind::LlmResponse;
    };

    match last_part {
        ContextMessagePart::Text { .. } | ContextMessagePart::Reasoning { .. } => {
            TaskKind::LlmResponse
        }
        ContextMessagePart::ToolCall { name, .. } => task_kind_from_tool_name(name),
        ContextMessagePart::ToolResult { id, .. } => message
            .parts
            .iter()
            .rev()
            .find_map(|part| match part {
                ContextMessagePart::ToolCall {
                    id: call_id, name, ..
                } if call_id == id => Some(task_kind_from_tool_name(name)),
                _ => None,
            })
            .unwrap_or(TaskKind::ToolCall),
        ContextMessagePart::File { .. } => TaskKind::FileRead,
        ContextMessagePart::Image { .. } => TaskKind::LlmResponse,
    }
}

fn task_kind_from_tool_name(name: &str) -> TaskKind {
    let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.is_empty() {
        return TaskKind::ToolCall;
    }

    if normalized.contains("read")
        || normalized.contains("grep")
        || normalized.contains("glob")
        || normalized.contains("list")
        || normalized == "ls"
    {
        return TaskKind::FileRead;
    }
    if normalized.contains("write")
        || normalized.contains("edit")
        || normalized.contains("patch")
        || normalized.contains("todo")
    {
        return TaskKind::FileWrite;
    }
    if normalized.contains("bash")
        || normalized.contains("shell")
        || normalized.contains("exec")
        || normalized.contains("command")
    {
        return TaskKind::CommandExec;
    }

    TaskKind::ToolCall
}

pub(super) fn map_mcp_status(server: &McpStatusInfo) -> McpConnectionStatus {
    match McpConnectionStatusWire::parse(server.status.as_str()) {
        Some(McpConnectionStatusWire::Connected) => McpConnectionStatus::Connected,
        Some(McpConnectionStatusWire::Failed) => McpConnectionStatus::Failed,
        Some(McpConnectionStatusWire::NeedsAuth) => McpConnectionStatus::NeedsAuth,
        Some(McpConnectionStatusWire::NeedsClientRegistration) => {
            McpConnectionStatus::NeedsClientRegistration
        }
        Some(McpConnectionStatusWire::Disabled) => McpConnectionStatus::Disabled,
        Some(McpConnectionStatusWire::Disconnected) | None => McpConnectionStatus::Disconnected,
    }
}

pub(super) fn map_api_run_status(status: &crate::api::SessionStatusInfo) -> SessionStatus {
    if status.busy {
        if status.status.eq_ignore_ascii_case("retry") {
            return SessionStatus::Retrying {
                message: status.message.clone().unwrap_or_default(),
                attempt: status.attempt.unwrap_or(0),
                next: status.next.unwrap_or_default(),
            };
        }
        return SessionStatus::Running;
    }
    SessionStatus::Idle
}

pub(super) fn agent_color_from_name(
    theme: &crate::theme::Theme,
    agent_name: &str,
) -> ratatui::style::Color {
    if theme.agent_colors.is_empty() {
        return theme.primary;
    }
    let mut hasher = DefaultHasher::new();
    agent_name.hash(&mut hasher);
    let idx = (hasher.finish() as usize) % theme.agent_colors.len();
    theme.agent_colors[idx]
}

pub(super) fn provider_from_model(model: &str) -> Option<String> {
    let model = model.trim();
    let (provider, _) = model
        .split_once('/')
        .or_else(|| model.split_once(':'))
        .unwrap_or((model, ""));
    if provider.is_empty() || provider == model {
        return None;
    }
    Some(provider.to_string())
}

pub(super) fn map_api_todo(item: &crate::api::ApiTodoItem) -> crate::context::TodoItem {
    use crate::context::{TodoItem, TodoStatus};
    let status = TodoStatus::parse(item.status.as_str()).unwrap_or(TodoStatus::Pending);
    TodoItem {
        content: item.content.clone(),
        status,
    }
}

pub(super) fn map_api_diff(item: &crate::api::ApiDiffEntry) -> crate::context::DiffEntry {
    use crate::context::DiffEntry;
    DiffEntry {
        file: item.path.clone(),
        additions: item.additions as u32,
        deletions: item.deletions as u32,
    }
}
