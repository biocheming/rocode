use chrono::{DateTime, Utc};
use rocode_permission::SessionPermissionRuleset;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::{MessagePart, Role, SessionMessage};

fn default_session_active() -> bool {
    false
}

static NEXT_SESSION_ID: AtomicI64 = AtomicI64::new(1);

fn new_session_id() -> String {
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

// ============================================================================
// Session Core
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// 会话主键 ID（全局唯一），当前实现格式通常为 `ses_<uuid>`。
    pub id: String,
    /// 会话工作目录（通常是项目内路径），用于命令执行与上下文定位。
    pub directory: String,
    /// 父会话 ID。
    ///
    /// - `None`：根会话/普通新会话
    /// - `Some(id)`：由 `child()` 创建的子会话
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// 会话标题（可自动生成，也可被用户或模型更新）。
    pub title: String,
    /// 会话数据版本号（schema/version 标记），用于兼容与迁移。
    pub version: String,
    /// 会话关键时间信息（创建、更新）。
    pub time: SessionTime,
    /// 会话消息列表（按写入顺序存放，包含 user/assistant 等消息）。
    pub messages: Vec<SessionMessage>,
    /// 代码变更摘要（新增/删除/文件数及可选 diff 列表）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SessionSummary>,
    /// 分享链接（例如公开访问 URL）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<String>,
    /// 回滚信息（用于撤销到某条消息/分片，或携带 snapshot/diff）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert: Option<SessionRevert>,
    /// 权限规则（allow/deny/mode），用于约束工具调用或路径访问范围。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionRuleset>,
    /// 用量统计（token 与成本），通常在推理完成后更新。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<SessionUsage>,
    /// 会话是否处于运行中（AI 正在处理请求）。
    ///
    /// - `true`: 正在运行（busy/retrying）
    /// - `false`: 空闲（idle）
    #[serde(default = "default_session_active")]
    pub active: bool,
    /// 扩展元数据容器。
    ///
    /// 用于存放非核心但有业务意义的键值（例如自动标题细化标记、模型/调度器信息等）。
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 会话进入内存缓存的时间（`DateTime<Utc>`），用于运行期缓存淘汰判断。
    ///
    /// 该字段不会被序列化输出（由 `serde(skip_serializing)` 控制）。
    #[serde(default, skip_serializing)]
    pub cached_at: DateTime<Utc>,
}

impl Session {
    const VERSION: &'static str = "1.0.0";
    const AUTO_TITLE_PENDING_REFINE_KEY: &'static str = "auto_title_pending_refine";

    /// Create a new session
    pub fn new(directory: impl Into<String>) -> Self {
        let now = Utc::now();

        Self {
            id: new_session_id(),
            directory: directory.into(),
            parent_id: None,
            title: format!("New session - {}", now.to_rfc3339()),
            version: Self::VERSION.to_string(),
            time: SessionTime::default(),
            messages: Vec::new(),
            summary: None,
            share: None,
            revert: None,
            permission: None,
            usage: None,
            active: false,
            metadata: HashMap::new(),
            cached_at: now,
        }
    }

    /// Create a child session
    pub fn child(parent: &Session) -> Self {
        let now = Utc::now();

        Self {
            id: new_session_id(),
            directory: parent.directory.clone(),
            parent_id: Some(parent.id.clone()),
            title: format!("Child session - {}", now.to_rfc3339()),
            version: Self::VERSION.to_string(),
            time: SessionTime::default(),
            messages: Vec::new(),
            summary: None,
            share: None,
            revert: None,
            permission: parent.permission.clone(),
            usage: None,
            active: false,
            metadata: HashMap::new(),
            cached_at: now,
        }
    }

    /// Check if title is a default generated title
    pub fn is_default_title(&self) -> bool {
        let prefix = if self.parent_id.is_some() {
            "Child session - "
        } else {
            "New session - "
        };

        if !self.title.starts_with(prefix) {
            return false;
        }

        let timestamp_part = &self.title[prefix.len()..];
        chrono::DateTime::parse_from_rfc3339(timestamp_part).is_ok()
    }

    /// Whether the current title is an auto-generated placeholder that may be
    /// replaced by the refined LLM-generated title after the first assistant turn.
    pub fn allows_auto_title_regeneration(&self) -> bool {
        self.is_default_title()
            || self
                .metadata
                .get(Self::AUTO_TITLE_PENDING_REFINE_KEY)
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
    }

    /// Get a forked title
    pub fn get_forked_title(&self) -> String {
        // Simple implementation without regex dependency
        if self.title.ends_with(")") && self.title.contains(" (fork #") {
            if let Some(pos) = self.title.rfind(" (fork #") {
                let base = &self.title[..pos];
                let num_part = &self.title[pos + 8..self.title.len() - 1];
                if let Ok(num) = num_part.parse::<u32>() {
                    return format!("{} (fork #{})", base, num + 1);
                }
            }
        }
        format!("{} (fork #1)", self.title)
    }

    /// Touch the session (update timestamp)
    pub fn touch(&mut self) {
        let now = Utc::now();
        self.time.updated = now.timestamp_millis();
    }

    // ========================================================================
    // Message Operations
    // ========================================================================

    /// Add a user message
    pub fn add_user_message(&mut self, text: impl Into<String>) -> &mut SessionMessage {
        let msg = SessionMessage::user(&self.id, text);
        self.messages.push(msg);
        self.touch();
        self.messages.last_mut().unwrap()
    }

    /// Add a synthetic user message with optional attachments.
    pub fn add_synthetic_user_message(
        &mut self,
        text: impl Into<String>,
        attachments: &[crate::FilePart],
    ) -> &mut SessionMessage {
        let mut msg = SessionMessage::user(&self.id, text);
        msg.mark_text_parts_synthetic();
        for attachment in attachments {
            msg.add_file(
                attachment.url.clone(),
                attachment
                    .filename
                    .clone()
                    .unwrap_or_else(|| "attachment".to_string()),
                attachment.mime.clone(),
            );
        }
        self.messages.push(msg);
        self.touch();
        self.messages.last_mut().unwrap()
    }

    /// Add an assistant message
    pub fn add_assistant_message(&mut self) -> &mut SessionMessage {
        let msg = SessionMessage::assistant(&self.id);
        self.messages.push(msg);
        self.touch();
        self.messages.last_mut().unwrap()
    }

    /// Get the last user message
    pub fn last_user_message(&self) -> Option<&SessionMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::User))
    }

    /// Get the last assistant message
    pub fn last_assistant_message(&self) -> Option<&SessionMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant))
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get a message by ID
    pub fn get_message(&self, id: &str) -> Option<&SessionMessage> {
        self.messages.iter().find(|m| m.id == id)
    }

    /// Get a mutable message by ID
    pub fn get_message_mut(&mut self, id: &str) -> Option<&mut SessionMessage> {
        self.messages.iter_mut().find(|m| m.id == id)
    }

    /// Remove a message by ID
    pub fn remove_message(&mut self, id: &str) -> Option<SessionMessage> {
        if let Some(pos) = self.messages.iter().position(|m| m.id == id) {
            let msg = self.messages.remove(pos);
            self.touch();
            Some(msg)
        } else {
            None
        }
    }

    // ========================================================================
    // Part-Level Operations
    // ========================================================================

    /// Update a message by replacing it entirely
    pub fn update_message(&mut self, msg: SessionMessage) -> Option<&SessionMessage> {
        if let Some(pos) = self.messages.iter().position(|m| m.id == msg.id) {
            self.messages[pos] = msg;
            self.touch();
            Some(&self.messages[pos])
        } else {
            // New message - append
            self.messages.push(msg);
            self.touch();
            self.messages.last()
        }
    }

    /// Update a specific part within a message
    pub fn update_part(&mut self, msg_id: &str, part: MessagePart) -> Option<&MessagePart> {
        let part_id = part.id.clone();
        let msg = self.get_message_mut(msg_id)?;
        if let Some(pos) = msg.parts.iter().position(|p| p.id == part_id) {
            msg.parts[pos] = part;
        } else {
            msg.parts.push(part);
        }
        self.touch();
        // Return reference to the part
        let msg = self.get_message(msg_id)?;
        msg.parts.iter().find(|p| p.id == part_id)
    }

    /// Remove a specific part from a message
    pub fn remove_part(&mut self, msg_id: &str, part_id: &str) -> Option<MessagePart> {
        let msg = self.get_message_mut(msg_id)?;
        if let Some(pos) = msg.parts.iter().position(|p| p.id == part_id) {
            let removed = msg.parts.remove(pos);
            self.touch();
            Some(removed)
        } else {
            None
        }
    }

    // ========================================================================
    // Usage Aggregation
    // ========================================================================

    /// Aggregate usage across all assistant messages in the session
    pub fn get_usage(&self) -> SessionUsage {
        let mut usage = SessionUsage::default();
        for msg in &self.messages {
            if matches!(msg.role, Role::Assistant) {
                if let Some(ref msg_usage) = msg.usage {
                    usage.input_tokens += msg_usage.input_tokens;
                    usage.output_tokens += msg_usage.output_tokens;
                    usage.reasoning_tokens += msg_usage.reasoning_tokens;
                    usage.cache_write_tokens += msg_usage.cache_write_tokens;
                    usage.cache_read_tokens += msg_usage.cache_read_tokens;
                    usage.total_cost += msg_usage.total_cost;
                }
            }
        }
        usage
    }

    /// Share the session (set share URL)
    pub fn share_session(&mut self, url: impl Into<String>) {
        self.share = Some(url.into());
        self.touch();
    }

    /// Unshare the session
    pub fn unshare_session(&mut self) {
        self.share = None;
        self.touch();
    }

    /// Compute diff summary from messages
    pub fn diff(&self) -> Vec<FileDiff> {
        self.summary
            .as_ref()
            .and_then(|s| s.diffs.clone())
            .unwrap_or_default()
    }

    // ========================================================================
    // Setters
    // ========================================================================

    /// Set the title
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
        self.metadata.remove(Self::AUTO_TITLE_PENDING_REFINE_KEY);
        self.touch();
    }

    /// Set an immediate auto-generated title that should still be replaced by
    /// the refined LLM title after the first completed turn.
    pub fn set_auto_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
        self.metadata.insert(
            Self::AUTO_TITLE_PENDING_REFINE_KEY.to_string(),
            serde_json::Value::Bool(true),
        );
        self.touch();
    }

    /// Set the permission ruleset
    pub fn set_permission(&mut self, permission: PermissionRuleset) {
        self.permission = Some(permission);
        self.touch();
    }

    /// Set the revert information
    pub fn set_revert(&mut self, revert: SessionRevert) {
        self.revert = Some(revert);
        self.touch();
    }

    /// Clear the revert information
    pub fn clear_revert(&mut self) {
        self.revert = None;
        self.touch();
    }

    /// Set the summary
    pub fn set_summary(&mut self, summary: SessionSummary) {
        self.summary = Some(summary);
        self.touch();
    }

    /// Set the share information
    pub fn set_share(&mut self, share: impl Into<String>) {
        self.share = Some(share.into());
        self.touch();
    }

    /// Clear the share information
    pub fn clear_share(&mut self) {
        self.share = None;
        self.touch();
    }

    /// Update usage statistics
    pub fn update_usage(&mut self, usage: SessionUsage) {
        self.usage = Some(usage);
        self.touch();
    }

    /// Set running-state flag.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        self.touch();
    }
}

// ============================================================================
// Session Components
// ============================================================================

/// Time tracking for session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTime {
    /// 会话创建时间（Unix 毫秒时间戳）。
    ///
    /// 该值用于会话排序、列表展示，以及从持久化数据恢复创建时刻。
    pub created: i64,
    /// 会话最近一次变更时间（Unix 毫秒时间戳）。
    ///
    /// 任何会修改会话状态/内容的操作通常会调用 `touch()` 来更新该字段。
    pub updated: i64,
}

impl Default for SessionTime {
    fn default() -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            created: now,
            updated: now,
        }
    }
}

/// Usage statistics for a session
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionUsage {
    /// 输入 token 数。
    ///
    /// 通常对应用户消息与系统上下文传入模型时消耗的 token。
    pub input_tokens: u64,
    /// 输出 token 数。
    ///
    /// 通常对应模型生成内容（assistant 回复）消耗的 token。
    pub output_tokens: u64,
    /// 推理 token 数。
    ///
    /// 某些模型会单独统计 reasoning/思维链相关消耗。
    pub reasoning_tokens: u64,
    /// 缓存写入 token 数。
    ///
    /// 表示写入 prompt cache 或类似缓存机制时计费/统计的 token。
    pub cache_write_tokens: u64,
    /// 缓存读取 token 数。
    ///
    /// 表示命中缓存并读取时统计的 token。
    pub cache_read_tokens: u64,
    /// 总成本（货币值）。
    ///
    /// 单位由上层计费体系决定（如 USD），通常为聚合估算值。
    pub total_cost: f64,
}

/// Summary of changes in a session
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionSummary {
    /// 会话内代码总新增行数。
    ///
    /// 通常由所有文件级 diff 的 `additions` 聚合得到，用于快速展示改动规模。
    pub additions: u64,
    /// 会话内代码总删除行数。
    ///
    /// 与 `additions` 配合可反映净变更趋势（净增长或净减少）。
    pub deletions: u64,
    /// 会话涉及变更的文件总数。
    ///
    /// 该值用于列表/统计展示，避免每次都遍历完整 diff 集合。
    pub files: u64,
    /// 文件级 diff 明细列表。
    ///
    /// 为 `None` 时表示仅存储聚合统计；有值时可用于逐文件渲染改动摘要。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diffs: Option<Vec<FileDiff>>,
}

/// File diff entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// 发生变更的文件路径（通常是相对项目根目录的路径）。
    pub path: String,
    /// 当前文件新增行数。
    pub additions: u64,
    /// 当前文件删除行数。
    pub deletions: u64,
}

/// Revert information for undo functionality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRevert {
    /// 回滚目标消息 ID。
    ///
    /// 表示撤销操作至少定位到哪条消息。
    pub message_id: String,
    /// 回滚目标分片（part）ID。
    ///
    /// 为 `None` 时表示以消息级回滚；有值时表示精确到消息内部某个 part。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    /// 可选完整快照数据。
    ///
    /// 当需要快速恢复到某一状态时可直接应用快照，而不必重放增量变更。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    /// 可选增量 diff 数据。
    ///
    /// 与 `snapshot` 二选一或互补使用，具体由上层回滚策略决定。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// Session-level permission memory model.
///
/// Canonical source: `rocode_permission::SessionPermissionRuleset`.
pub type PermissionRuleset = SessionPermissionRuleset;

// ============================================================================
// Persistence Plan
// ============================================================================

/// Persistence action derived from a session snapshot.
///
/// This lets callers keep all write-policy decisions centralized in
/// `rocode-session`, while storage backends only execute the action.
#[derive(Debug, Clone)]
pub enum SessionPersistPlan {
    /// Persist only session metadata (do not overwrite message history).
    MetadataOnly(Session),
    /// Persist session metadata and full message history transactionally.
    Full {
        /// 待持久化的会话元数据快照。
        session: Session,
        /// 待持久化的完整消息列表。
        messages: Vec<SessionMessage>,
    },
}

impl SessionPersistPlan {
    /// Build a persistence plan from an in-memory session snapshot.
    ///
    /// `hydrated=true` means message history is currently resident in memory and
    /// safe to flush as source of truth. `hydrated=false` keeps DB history intact
    /// by writing metadata only.
    pub fn from_snapshot(mut session: Session, hydrated: bool) -> Self {
        if hydrated || !session.messages.is_empty() {
            let messages = std::mem::take(&mut session.messages);
            Self::Full { session, messages }
        } else {
            Self::MetadataOnly(session)
        }
    }

    pub fn session(&self) -> &Session {
        match self {
            Self::MetadataOnly(session) => session,
            Self::Full { session, .. } => session,
        }
    }

    pub fn messages(&self) -> Option<&[SessionMessage]> {
        match self {
            Self::MetadataOnly(_) => None,
            Self::Full { messages, .. } => Some(messages),
        }
    }
}
