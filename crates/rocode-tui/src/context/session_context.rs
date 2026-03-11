use chrono::{DateTime, Utc};
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

#[derive(Clone, Debug)]
pub enum SessionStatus {
    Idle,
    Running,
    Retrying {
        message: String,
        attempt: u32,
        next: i64,
    },
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Idle
    }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
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
}
