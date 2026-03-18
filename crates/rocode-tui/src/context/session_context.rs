use chrono::{DateTime, Utc};
use rocode_command::output_blocks::SchedulerStageBlock;
use rocode_core::contracts::output_blocks::{
    MessagePhaseWire, MessageRoleWire, OutputBlockKind, ToolPhaseWire,
};
use rocode_core::contracts::scheduler::keys as scheduler_keys;
use rocode_core::contracts::scheduler::SchedulerStageStatus;
pub use rocode_core::contracts::todo::TodoStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub parent_id: Option<String>,
    pub share: Option<ShareInfo>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShareInfo {
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub finish: Option<String>,
    pub error: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cost: f64,
    pub tokens: TokenUsage,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub parts: Vec<MessagePart>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MessagePart {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    File {
        path: String,
        mime: String,
    },
    Image {
        url: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        result: String,
        is_error: bool,
        title: Option<String>,
        metadata: Option<HashMap<String, serde_json::Value>>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct SessionContext {
    pub sessions: HashMap<String, Session>,
    pub messages: HashMap<String, Vec<Message>>,
    pub message_index: HashMap<String, HashMap<String, usize>>,
    pub current_session_id: Option<String>,
    pub session_status: HashMap<String, SessionStatus>,
    pub session_diff: HashMap<String, Vec<DiffEntry>>,
    pub todos: HashMap<String, Vec<TodoItem>>,
    pub revert: HashMap<String, RevertInfo>,
}

#[derive(Clone, Debug, Default)]
pub enum SessionStatus {
    #[default]
    Idle,
    Running,
    Retrying {
        message: String,
        attempt: u32,
        next: i64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiffEntry {
    pub file: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevertInfo {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Clone, Debug)]
pub struct ChildSessionInfo {
    pub session_id: String,
    pub stage_name: String,
    pub stage_title: String,
    pub stage_id: Option<String>,
    pub stage_index: Option<u64>,
    pub stage_total: Option<u64>,
    pub status: String,
}

pub fn collect_child_sessions(messages: &[Message]) -> Vec<ChildSessionInfo> {
    let mut seen = HashMap::new();
    for msg in messages {
        let meta = match msg.metadata.as_ref() {
            Some(m) => m,
            None => continue,
        };
        let child_id = match meta
            .get(scheduler_keys::CHILD_SESSION_ID)
            .and_then(|v| v.as_str())
        {
            Some(id) => id.to_string(),
            None => continue,
        };
        let stage_name = meta
            .get(scheduler_keys::STAGE)
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let stage_title = meta
            .get(scheduler_keys::STAGE_TITLE)
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| stage_name.clone());
        let stage_index = meta
            .get(scheduler_keys::STAGE_INDEX)
            .and_then(|v| v.as_u64());
        let stage_total = meta
            .get(scheduler_keys::STAGE_TOTAL)
            .and_then(|v| v.as_u64());
        let status = meta
            .get(scheduler_keys::STATUS)
            .and_then(|v| v.as_str())
            .unwrap_or(SchedulerStageStatus::Running.as_str())
            .to_string();
        let stage_id = meta
            .get("stage_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let info = ChildSessionInfo {
            session_id: child_id.clone(),
            stage_name,
            stage_title,
            stage_id,
            stage_index,
            stage_total,
            status,
        };
        seen.insert(child_id, info);
    }

    let mut result: Vec<ChildSessionInfo> = seen.into_values().collect();
    result.sort_by(|a, b| {
        a.stage_index
            .unwrap_or(u64::MAX)
            .cmp(&b.stage_index.unwrap_or(u64::MAX))
    });
    result
}

impl SessionContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_session(&self) -> Option<&Session> {
        self.current_session_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    pub fn current_messages(&self) -> Vec<&Message> {
        self.current_session_id
            .as_ref()
            .and_then(|id| self.messages.get(id))
            .map(|m| m.iter().collect())
            .unwrap_or_default()
    }

    pub fn create_session(&mut self, title: Option<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let session = Session {
            id: id.clone(),
            title: title.unwrap_or_else(|| "New Session".to_string()),
            created_at: now,
            updated_at: now,
            parent_id: None,
            share: None,
            metadata: None,
        };
        self.sessions.insert(id.clone(), session);
        self.messages.insert(id.clone(), Vec::new());
        self.message_index.insert(id.clone(), HashMap::new());
        self.session_status.insert(id.clone(), SessionStatus::Idle);
        self.current_session_id = Some(id.clone());
        id
    }

    pub fn upsert_session(&mut self, session: Session) {
        let id = session.id.clone();
        self.sessions.insert(id.clone(), session);
        self.messages.entry(id.clone()).or_default();
        self.message_index.entry(id.clone()).or_default();
        self.session_status
            .entry(id.clone())
            .or_insert(SessionStatus::Idle);
        // Only set current_session_id if no session is active yet.
        // Callers that want to switch sessions should use set_current_session_id().
        if self.current_session_id.is_none() {
            self.current_session_id = Some(id);
        }
    }

    /// Explicitly switch the current session to the given id.
    pub fn set_current_session_id(&mut self, id: String) {
        self.current_session_id = Some(id);
    }

    pub fn set_messages(&mut self, session_id: &str, messages: Vec<Message>) {
        let mut index = HashMap::with_capacity(messages.len());
        for (pos, message) in messages.iter().enumerate() {
            index.insert(message.id.clone(), pos);
        }
        self.messages.insert(session_id.to_string(), messages);
        self.message_index.insert(session_id.to_string(), index);
    }

    pub fn add_message(&mut self, session_id: &str, message: Message) {
        self.upsert_message(session_id, message);
    }

    pub fn upsert_messages_incremental(&mut self, session_id: &str, incoming: Vec<Message>) {
        for message in incoming {
            self.upsert_message(session_id, message);
        }
    }

    pub fn upsert_message(&mut self, session_id: &str, message: Message) {
        let messages = self.messages.entry(session_id.to_string()).or_default();
        let index = self
            .message_index
            .entry(session_id.to_string())
            .or_default();
        if let Some(existing_pos) = index.get(&message.id).copied() {
            if existing_pos < messages.len() {
                messages[existing_pos] = message;
                return;
            }
            // Index drift should be rare; rebuild once to recover.
            index.clear();
            for (pos, msg) in messages.iter().enumerate() {
                index.insert(msg.id.clone(), pos);
            }
        }
        let message_id = message.id.clone();
        messages.push(message);
        index.insert(message_id, messages.len().saturating_sub(1));
    }

    pub fn set_status(&mut self, session_id: &str, status: SessionStatus) {
        self.session_status.insert(session_id.to_string(), status);
    }

    pub fn status(&self, session_id: &str) -> &SessionStatus {
        self.session_status
            .get(session_id)
            .unwrap_or(&SessionStatus::Idle)
    }

    /// Incrementally update reasoning content for a message during streaming.
    /// This allows real-time display of thinking content before the message is complete.
    pub fn update_reasoning_incremental(
        &mut self,
        session_id: &str,
        message_id: &str,
        phase: &str,
        text: &str,
    ) {
        if message_id.is_empty() {
            tracing::warn!("update_reasoning_incremental called with empty message_id for session {session_id}");
            return;
        }
        let messages = self.messages.entry(session_id.to_string()).or_default();
        let index = self
            .message_index
            .entry(session_id.to_string())
            .or_default();

        // If the message doesn't exist yet (streaming hasn't synced it),
        // create a placeholder assistant message so reasoning can accumulate.
        if !index.contains_key(message_id) {
            let pos = messages.len();
            messages.push(Message {
                id: message_id.to_string(),
                role: MessageRole::Assistant,
                content: String::new(),
                created_at: chrono::Utc::now(),
                agent: None,
                model: None,
                mode: None,
                finish: None,
                error: None,
                completed_at: None,
                cost: 0.0,
                tokens: TokenUsage::default(),
                metadata: None,
                parts: Vec::new(),
            });
            index.insert(message_id.to_string(), pos);
        }

        let Some(&pos) = index.get(message_id) else {
            return;
        };
        let Some(message) = messages.get_mut(pos) else {
            return;
        };

        // Find or create a Reasoning part
        match MessagePhaseWire::parse(phase) {
            Some(MessagePhaseWire::Start) => {
                // Initialize or reset reasoning content
                // Check if there's already a Reasoning part
                let has_reasoning = message
                    .parts
                    .iter()
                    .any(|p| matches!(p, MessagePart::Reasoning { .. }));
                if !has_reasoning {
                    message.parts.push(MessagePart::Reasoning {
                        text: String::new(),
                    });
                }
            }
            Some(MessagePhaseWire::Delta) => {
                // Append reasoning text
                for part in &mut message.parts {
                    if let MessagePart::Reasoning {
                        text: ref mut existing,
                    } = part
                    {
                        existing.push_str(text);
                        break;
                    }
                }
            }
            Some(MessagePhaseWire::End) => {
                // Reasoning complete - nothing special to do, the text is already there
            }
            _ => {}
        }
    }

    pub fn apply_output_block_incremental(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        self.ensure_streaming_session(session_id, None, None);

        let Some(kind_raw) = payload.get("kind").and_then(|value| value.as_str()) else {
            return;
        };
        let Some(kind) = OutputBlockKind::parse(kind_raw) else {
            return;
        };

        match kind {
            OutputBlockKind::Message => self.apply_message_block(session_id, block_id, payload),
            OutputBlockKind::Reasoning => self.apply_reasoning_block(session_id, block_id, payload),
            OutputBlockKind::Tool => self.apply_tool_block(session_id, block_id, payload),
            OutputBlockKind::SchedulerStage => {
                self.apply_scheduler_stage_block(session_id, block_id, payload)
            }
            _ => return,
        }

        if let Some(session) = self.sessions.get_mut(session_id) {
            session.updated_at = Utc::now();
        }
    }

    fn apply_message_block(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let role_raw = payload
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or(MessageRoleWire::Assistant.as_str());
        let role = match MessageRoleWire::parse(role_raw) {
            Some(MessageRoleWire::System) => MessageRole::System,
            Some(MessageRoleWire::User) => MessageRole::User,
            _ => MessageRole::Assistant,
        };

        let phase_raw = payload
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or(MessagePhaseWire::Delta.as_str());
        let phase = MessagePhaseWire::parse(phase_raw);
        let text = payload
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        let pos = self.ensure_message_for_block(session_id, block_id, role.clone());
        let Some(message) = self
            .messages
            .get_mut(session_id)
            .and_then(|messages| messages.get_mut(pos))
        else {
            return;
        };

        match phase {
            Some(MessagePhaseWire::Start) => {
                message.role = role;
                message.content.clear();
                message
                    .parts
                    .retain(|part| !matches!(part, MessagePart::Text { .. }));
            }
            Some(MessagePhaseWire::Delta) => {
                if let Some(MessagePart::Text { text: existing }) = message
                    .parts
                    .iter_mut()
                    .rev()
                    .find(|part| matches!(part, MessagePart::Text { .. }))
                {
                    existing.push_str(text);
                } else {
                    message.parts.push(MessagePart::Text {
                        text: text.to_string(),
                    });
                }
            }
            Some(MessagePhaseWire::Full) => {
                message.role = role;
                message
                    .parts
                    .retain(|part| !matches!(part, MessagePart::Text { .. }));
                message.parts.push(MessagePart::Text {
                    text: text.to_string(),
                });
            }
            Some(MessagePhaseWire::End) => {}
            _ => {}
        }

        Self::refresh_message_content(message);
    }

    fn apply_reasoning_block(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        // When block_id is None or empty, fall back to the last assistant message
        // for this session so reasoning is not silently discarded.
        let fallback_id;
        let message_id = match block_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                fallback_id = self
                    .messages
                    .get(session_id)
                    .and_then(|msgs| {
                        msgs.iter()
                            .rev()
                            .find(|m| m.role == MessageRole::Assistant)
                            .map(|m| m.id.clone())
                    })
                    .unwrap_or_else(|| format!("_reasoning_{session_id}"));
                &fallback_id
            }
        };
        let phase = payload
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or(MessagePhaseWire::Delta.as_str());
        let text = payload
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        self.update_reasoning_incremental(session_id, message_id, phase, text);
    }

    fn apply_tool_block(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let tool_call_id = block_id.unwrap_or_default();
        let tool_name = payload
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool");
        let phase_raw = payload
            .get("phase")
            .and_then(|value| value.as_str())
            .unwrap_or(ToolPhaseWire::Running.as_str());
        let phase = ToolPhaseWire::parse(phase_raw);
        let detail = payload
            .get("detail")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        let pos = self.ensure_message_for_block(session_id, None, MessageRole::Assistant);
        let Some(message) = self
            .messages
            .get_mut(session_id)
            .and_then(|messages| messages.get_mut(pos))
        else {
            return;
        };

        match phase {
            Some(ToolPhaseWire::Start) | Some(ToolPhaseWire::Running) => {
                let arguments = detail;
                if let Some(MessagePart::ToolCall {
                    name,
                    arguments: existing,
                    ..
                }) = message.parts.iter_mut().find(|part| {
                    matches!(
                        part,
                        MessagePart::ToolCall { id, .. } if id == tool_call_id
                    )
                }) {
                    *name = tool_name.to_string();
                    *existing = arguments;
                } else {
                    message.parts.push(MessagePart::ToolCall {
                        id: tool_call_id.to_string(),
                        name: tool_name.to_string(),
                        arguments,
                    });
                }
            }
            Some(ToolPhaseWire::Done) | Some(ToolPhaseWire::Error) => {
                let is_error = matches!(phase, Some(ToolPhaseWire::Error));
                if let Some(part) = message.parts.iter_mut().find(|part| {
                    matches!(
                        part,
                        MessagePart::ToolResult { id, .. } if id == tool_call_id
                    )
                }) {
                    if let MessagePart::ToolResult {
                        result,
                        is_error: part_is_error,
                        title,
                        ..
                    } = part
                    {
                        *result = detail.clone();
                        *part_is_error = is_error;
                        *title = Some(tool_name.to_string());
                    }
                } else {
                    message.parts.push(MessagePart::ToolResult {
                        id: tool_call_id.to_string(),
                        result: detail.clone(),
                        is_error,
                        title: Some(tool_name.to_string()),
                        metadata: None,
                    });
                }
            }
            _ => {}
        }

        Self::refresh_message_content(message);
    }

    fn apply_scheduler_stage_block(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        payload: &serde_json::Value,
    ) {
        let Ok(block) = serde_json::from_value::<SchedulerStageBlock>(payload.clone()) else {
            return;
        };

        let pos = self.ensure_message_for_block(session_id, block_id, MessageRole::Assistant);
        let Some(message) = self
            .messages
            .get_mut(session_id)
            .and_then(|messages| messages.get_mut(pos))
        else {
            return;
        };

        message.role = MessageRole::Assistant;
        message
            .parts
            .retain(|part| !matches!(part, MessagePart::Text { .. }));
        message.parts.push(MessagePart::Text {
            text: block.text.clone(),
        });
        message.metadata = Some(Self::scheduler_stage_metadata_from_block(&block));
        Self::refresh_message_content(message);

        if let Some(child_session_id) = block.child_session_id.as_deref() {
            let child_title = format!("Stage: {}", block.title);
            self.ensure_streaming_session(
                child_session_id,
                Some(session_id.to_string()),
                Some(child_title),
            );
        }
    }

    fn ensure_message_for_block(
        &mut self,
        session_id: &str,
        block_id: Option<&str>,
        role: MessageRole,
    ) -> usize {
        let messages = self.messages.entry(session_id.to_string()).or_default();
        let index = self
            .message_index
            .entry(session_id.to_string())
            .or_default();

        if let Some(message_id) = block_id.filter(|value| !value.is_empty()) {
            if let Some(existing) = index.get(message_id).copied() {
                return existing;
            }

            let pos = messages.len();
            messages.push(Self::streaming_placeholder_message(message_id, role));
            index.insert(message_id.to_string(), pos);
            return pos;
        }

        if let Some((pos, _)) = messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, message)| message.role == role)
        {
            return pos;
        }

        let generated_id = format!("streaming_{}", Utc::now().timestamp_millis());
        let pos = messages.len();
        messages.push(Self::streaming_placeholder_message(&generated_id, role));
        index.insert(generated_id, pos);
        pos
    }

    fn ensure_streaming_session(
        &mut self,
        session_id: &str,
        parent_id: Option<String>,
        title: Option<String>,
    ) {
        self.messages.entry(session_id.to_string()).or_default();
        self.message_index
            .entry(session_id.to_string())
            .or_default();
        self.session_status
            .entry(session_id.to_string())
            .or_insert(SessionStatus::Idle);

        let entry = self
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session {
                id: session_id.to_string(),
                title: title.clone().unwrap_or_else(|| "Live Session".to_string()),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                parent_id: parent_id.clone(),
                share: None,
                metadata: None,
            });

        if let Some(parent_id) = parent_id {
            entry.parent_id = Some(parent_id);
        }
        if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
            entry.title = title;
        }
    }

    fn streaming_placeholder_message(message_id: &str, role: MessageRole) -> Message {
        Message {
            id: message_id.to_string(),
            role,
            content: String::new(),
            created_at: Utc::now(),
            agent: None,
            model: None,
            mode: None,
            finish: None,
            error: None,
            completed_at: None,
            cost: 0.0,
            tokens: TokenUsage::default(),
            metadata: None,
            parts: Vec::new(),
        }
    }

    fn refresh_message_content(message: &mut Message) {
        message.content = message
            .parts
            .iter()
            .map(|part| match part {
                MessagePart::Text { text } => text.clone(),
                MessagePart::Reasoning { text } => format!("[reasoning] {}", text),
                MessagePart::ToolCall {
                    name, arguments, ..
                } => {
                    if arguments.trim().is_empty() {
                        format!("[tool:{}]", name)
                    } else {
                        format!("[tool:{}] {}", name, arguments)
                    }
                }
                MessagePart::ToolResult {
                    result, is_error, ..
                } => {
                    if *is_error {
                        format!("[tool-error] {}", result)
                    } else {
                        format!("[tool-result] {}", result)
                    }
                }
                MessagePart::File { path, .. } => format!("[file] {}", path),
                MessagePart::Image { url } => format!("[image] {}", url),
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }

    fn scheduler_stage_metadata_from_block(
        block: &SchedulerStageBlock,
    ) -> HashMap<String, serde_json::Value> {
        let mut metadata = HashMap::new();
        metadata.insert(
            scheduler_keys::PROFILE.to_string(),
            serde_json::json!(block.profile.clone()),
        );
        metadata.insert(
            scheduler_keys::RESOLVED_PROFILE.to_string(),
            serde_json::json!(block.profile.clone()),
        );
        metadata.insert(
            scheduler_keys::STAGE.to_string(),
            serde_json::json!(block.stage.clone()),
        );
        metadata.insert(
            scheduler_keys::STAGE_TITLE.to_string(),
            serde_json::json!(block.title.clone()),
        );
        metadata.insert(scheduler_keys::EMITTED.to_string(), serde_json::json!(true));
        if let Some(value) = block.stage_id.clone() {
            metadata.insert(
                scheduler_keys::STAGE_ID.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.stage_index {
            metadata.insert(
                scheduler_keys::STAGE_INDEX.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.stage_total {
            metadata.insert(
                scheduler_keys::STAGE_TOTAL.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.step {
            metadata.insert(scheduler_keys::STEP.to_string(), serde_json::json!(value));
        }
        if let Some(value) = block.status.clone() {
            metadata.insert(scheduler_keys::STATUS.to_string(), serde_json::json!(value));
        }
        if let Some(value) = block.focus.clone() {
            metadata.insert(scheduler_keys::FOCUS.to_string(), serde_json::json!(value));
        }
        if let Some(value) = block.last_event.clone() {
            metadata.insert(
                scheduler_keys::LAST_EVENT.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.waiting_on.clone() {
            metadata.insert(
                scheduler_keys::WAITING_ON.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.activity.clone() {
            metadata.insert(
                scheduler_keys::ACTIVITY.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.loop_budget.clone() {
            metadata.insert(
                scheduler_keys::LOOP_BUDGET.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.available_skill_count {
            metadata.insert(
                scheduler_keys::AVAILABLE_SKILL_COUNT.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.available_agent_count {
            metadata.insert(
                scheduler_keys::AVAILABLE_AGENT_COUNT.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.available_category_count {
            metadata.insert(
                scheduler_keys::AVAILABLE_CATEGORY_COUNT.to_string(),
                serde_json::json!(value),
            );
        }
        metadata.insert(
            scheduler_keys::ACTIVE_SKILLS.to_string(),
            serde_json::json!(block.active_skills.clone()),
        );
        metadata.insert(
            scheduler_keys::ACTIVE_AGENTS.to_string(),
            serde_json::json!(block.active_agents.clone()),
        );
        metadata.insert(
            scheduler_keys::ACTIVE_CATEGORIES.to_string(),
            serde_json::json!(block.active_categories.clone()),
        );
        metadata.insert(
            scheduler_keys::DONE_AGENT_COUNT.to_string(),
            serde_json::json!(block.done_agent_count),
        );
        metadata.insert(
            scheduler_keys::TOTAL_AGENT_COUNT.to_string(),
            serde_json::json!(block.total_agent_count),
        );
        if let Some(value) = block.prompt_tokens {
            metadata.insert(
                scheduler_keys::PROMPT_TOKENS.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.completion_tokens {
            metadata.insert(
                scheduler_keys::COMPLETION_TOKENS.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.reasoning_tokens {
            metadata.insert(
                scheduler_keys::REASONING_TOKENS.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.cache_read_tokens {
            metadata.insert(
                scheduler_keys::CACHE_READ_TOKENS.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.cache_write_tokens {
            metadata.insert(
                scheduler_keys::CACHE_WRITE_TOKENS.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.child_session_id.clone() {
            metadata.insert(
                scheduler_keys::CHILD_SESSION_ID.to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = block.decision.as_ref() {
            metadata.insert(
                scheduler_keys::DECISION.to_string(),
                serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
            );
        }
        metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scheduler_stage_output_block_updates_parent_metadata_and_child_session() {
        let mut ctx = SessionContext::new();
        ctx.upsert_session(Session {
            id: "parent".to_string(),
            title: "Parent".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            parent_id: None,
            share: None,
            metadata: None,
        });

        ctx.apply_output_block_incremental(
            "parent",
            Some("stage-message-1"),
            &json!({
                "kind": OutputBlockKind::SchedulerStage.as_str(),
                "stage_id": "stage-1",
                "profile": "atlas",
                "stage": "execution-orchestration",
                "title": "Execution Orchestration",
                "text": "child stage running",
                "stage_index": 2,
                "stage_total": 5,
                "status": SchedulerStageStatus::Running.as_str(),
                "child_session_id": "child-1",
                "active_agents": [],
                "active_skills": [],
                "active_categories": [],
                "done_agent_count": 0,
                "total_agent_count": 0
            }),
        );

        let parent_messages = ctx.messages.get("parent").expect("parent messages");
        let stage_message = parent_messages
            .iter()
            .find(|message| message.id == "stage-message-1")
            .expect("stage message");
        let metadata = stage_message.metadata.as_ref().expect("stage metadata");
        assert_eq!(
            metadata
                .get(scheduler_keys::CHILD_SESSION_ID)
                .and_then(|value| value.as_str()),
            Some("child-1")
        );

        let child = ctx.sessions.get("child-1").expect("child session created");
        assert_eq!(child.parent_id.as_deref(), Some("parent"));
        assert_eq!(child.title, "Stage: Execution Orchestration");
    }

    #[test]
    fn child_output_block_creates_background_session_cache() {
        let mut ctx = SessionContext::new();

        ctx.apply_output_block_incremental(
            "child-1",
            Some("assistant-1"),
            &json!({
                "kind": OutputBlockKind::Message.as_str(),
                "phase": MessagePhaseWire::Delta.as_str(),
                "role": MessageRoleWire::Assistant.as_str(),
                "text": "hello child"
            }),
        );

        let child = ctx
            .sessions
            .get("child-1")
            .expect("child session placeholder");
        assert_eq!(child.id, "child-1");

        let message = ctx
            .messages
            .get("child-1")
            .and_then(|messages| messages.iter().find(|message| message.id == "assistant-1"))
            .expect("child assistant message");
        assert!(message.content.contains("hello child"));
    }

    #[test]
    fn message_start_preserves_existing_reasoning_parts() {
        let mut ctx = SessionContext::new();

        ctx.apply_output_block_incremental(
            "session-1",
            Some("assistant-1"),
            &json!({
                "kind": OutputBlockKind::Reasoning.as_str(),
                "phase": MessagePhaseWire::Start.as_str(),
                "text": ""
            }),
        );
        ctx.apply_output_block_incremental(
            "session-1",
            Some("assistant-1"),
            &json!({
                "kind": OutputBlockKind::Reasoning.as_str(),
                "phase": MessagePhaseWire::Delta.as_str(),
                "text": "thinking..."
            }),
        );
        ctx.apply_output_block_incremental(
            "session-1",
            Some("assistant-1"),
            &json!({
                "kind": OutputBlockKind::Message.as_str(),
                "phase": MessagePhaseWire::Start.as_str(),
                "role": MessageRoleWire::Assistant.as_str(),
                "text": ""
            }),
        );

        let message = ctx
            .messages
            .get("session-1")
            .and_then(|messages| messages.iter().find(|message| message.id == "assistant-1"))
            .expect("assistant message");

        assert!(message.parts.iter().any(|part| {
            matches!(
                part,
                MessagePart::Reasoning { text } if text == "thinking..."
            )
        }));
    }
}
