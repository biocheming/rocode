use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
use chrono::Utc;
use rocode_core::bus::{Bus, BusEventDef};
use rocode_core::contracts::events::BusEventName;
#[cfg(test)]
use rocode_core::contracts::wire::keys as wire_keys;
use rocode_plugin::{HookContext, HookEvent};

#[cfg(test)]
use crate::MessageUsage;
use crate::{MessagePart, SessionMessage};

pub use crate::session_model::{
    FileDiff, PermissionRuleset, Session, SessionPersistPlan, SessionRevert, SessionSummary,
    SessionTime, SessionUsage,
};

// ============================================================================
// Bus Event Definitions (matches TS Session.Event)
// ============================================================================

pub static SESSION_CREATED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::SessionCreated.as_str());
pub static SESSION_UPDATED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::SessionUpdated.as_str());
pub static SESSION_DELETED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::SessionDeleted.as_str());
pub static SESSION_DIFF_EVENT: BusEventDef = BusEventDef::new(BusEventName::SessionDiff.as_str());
pub static SESSION_ERROR_EVENT: BusEventDef = BusEventDef::new(BusEventName::SessionError.as_str());

// Message-level events (matches TS MessageV2.Event)
pub static MESSAGE_UPDATED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::MessageUpdated.as_str());
pub static MESSAGE_REMOVED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::MessageRemoved.as_str());
pub static PART_UPDATED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::MessagePartUpdated.as_str());
pub static PART_REMOVED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::MessagePartRemoved.as_str());
pub static PART_DELTA_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::MessagePartDelta.as_str());
pub static COMMAND_EXECUTED_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::CommandExecuted.as_str());

pub fn sanitize_display_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_pseudo_invoke = false;
    let mut previous_blank = false;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with("minimax:tool_call") {
            continue;
        }
        if trimmed.starts_with("<invoke ") {
            in_pseudo_invoke = true;
            continue;
        }
        if in_pseudo_invoke {
            if trimmed.starts_with("</invoke>") {
                in_pseudo_invoke = false;
            }
            continue;
        }
        if trimmed.starts_with("<parameter ") || trimmed.starts_with("</invoke>") {
            continue;
        }

        if trimmed.is_empty() {
            if previous_blank {
                continue;
            }
            previous_blank = true;
            lines.push(String::new());
            continue;
        }

        previous_blank = false;
        lines.push(raw_line.to_string());
    }

    lines.join("\n").trim().to_string()
}

// ============================================================================
// Session Event Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionEvent {
    Created {
        info: Session,
    },
    Updated {
        info: Session,
    },
    Deleted {
        info: Session,
    },
    Diff {
        session_id: String,
        diff: Vec<FileDiff>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        error: SessionError,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ============================================================================
// Session Status
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum RunStatus {
    #[default]
    Idle,
    Busy,
    Retrying {
        attempt: u32,
        #[serde(default)]
        message: String,
        /// Timestamp (millis) of the next retry attempt.
        #[serde(default)]
        next: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStateEvent {
    StatusChanged {
        session_id: String,
        status: RunStatus,
    },
    Error {
        session_id: String,
        error: String,
    },
}

pub struct SessionStateManager {
    states: HashMap<String, RunStatus>,
}

impl SessionStateManager {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    pub fn set(&mut self, session_id: &str, status: RunStatus) {
        self.states.insert(session_id.to_string(), status);
    }

    pub fn get(&self, session_id: &str) -> RunStatus {
        self.states.get(session_id).cloned().unwrap_or_default()
    }

    pub fn is_busy(&self, session_id: &str) -> bool {
        matches!(
            self.get(session_id),
            RunStatus::Busy | RunStatus::Retrying { .. }
        )
    }

    pub fn assert_not_busy(&self, session_id: &str) -> Result<(), BusyError> {
        if self.is_busy(session_id) {
            return Err(BusyError {
                session_id: session_id.to_string(),
            });
        }
        Ok(())
    }

    pub fn set_busy(&mut self, session_id: &str) {
        self.set(session_id, RunStatus::Busy);
    }

    pub fn set_retrying(&mut self, session_id: &str, attempt: u32, message: String, next: i64) {
        self.set(
            session_id,
            RunStatus::Retrying {
                attempt,
                message,
                next,
            },
        );
    }

    pub fn set_idle(&mut self, session_id: &str) {
        self.set(session_id, RunStatus::Idle);
    }

    pub fn remove(&mut self, session_id: &str) {
        self.states.remove(session_id);
    }

    pub fn busy_sessions(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, s)| matches!(s, RunStatus::Busy | RunStatus::Retrying { .. }))
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// List all session statuses.
    /// Matches TS `SessionStatus.list()` returning all tracked states.
    pub fn list(&self) -> &HashMap<String, RunStatus> {
        &self.states
    }
}

impl Default for SessionStateManager {
    fn default() -> Self {
        Self::new()
    }
}

// Session model definitions were extracted to `session_model.rs`.

// ============================================================================
// Session Manager
// ============================================================================

pub struct SessionManager {
    sessions: HashMap<String, Session>,
    events: Vec<SessionEvent>,
    bus: Option<Arc<Bus>>,
}

#[derive(Serialize)]
struct InfoEnvelope {
    info: serde_json::Value,
}

#[derive(Serialize)]
struct PartEnvelope {
    part: serde_json::Value,
}

#[derive(Serialize)]
struct PartDeltaEvent<'a> {
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    #[serde(rename = "messageID")]
    message_id: &'a str,
    #[serde(rename = "partID")]
    part_id: &'a str,
    field: &'a str,
    delta: &'a str,
}

#[derive(Serialize)]
struct CommandExecutedEvent<'a> {
    name: &'a str,
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    arguments: Vec<String>,
    #[serde(rename = "messageID")]
    message_id: &'a str,
}

#[derive(Serialize)]
struct MessageRemovedEvent<'a> {
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    #[serde(rename = "messageID")]
    message_id: &'a str,
}

#[derive(Serialize)]
struct PartRemovedEvent<'a> {
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    #[serde(rename = "messageID")]
    message_id: &'a str,
    #[serde(rename = "partID")]
    part_id: &'a str,
}

#[derive(Serialize)]
struct SessionErrorEvent<'a> {
    error: serde_json::Value,
    #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
}

#[derive(Serialize)]
struct SessionDiffEvent<'a> {
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    diff: serde_json::Value,
}

fn value_or_null<T: Serialize>(value: T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            events: Vec::new(),
            bus: None,
        }
    }

    /// Create a new SessionManager with a Bus for event publishing
    pub fn with_bus(bus: Arc<Bus>) -> Self {
        Self {
            sessions: HashMap::new(),
            events: Vec::new(),
            bus: Some(bus),
        }
    }

    /// Publish an event to the Bus (fire-and-forget from sync context)
    fn publish_event(&self, def: &'static BusEventDef, properties: serde_json::Value) {
        if let Some(ref bus) = self.bus {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let bus = bus.clone();
                handle.spawn(async move {
                    bus.publish(def, properties).await;
                });
            }
        }
    }

    /// Publish a session info event (Created/Updated/Deleted)
    fn publish_session_event(&self, def: &'static BusEventDef, session: &Session) {
        if let Ok(mut json) = serde_json::to_value(session) {
            if let Some(share_url) = json
                .get("share")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
            {
                json["share"] = serde_json::json!({ "url": share_url });
            }
            self.publish_event(def, value_or_null(InfoEnvelope { info: json }));
        }
    }

    /// Publish a message event
    fn publish_message_event(&self, def: &'static BusEventDef, msg: &SessionMessage) {
        if let Ok(json) = serde_json::to_value(msg) {
            self.publish_event(def, value_or_null(InfoEnvelope { info: json }));
        }
    }

    /// Publish a part event
    fn publish_part_event(&self, def: &'static BusEventDef, part: &MessagePart) {
        if let Ok(json) = serde_json::to_value(part) {
            self.publish_event(def, value_or_null(PartEnvelope { part: json }));
        }
    }

    /// Publish a part delta event (streaming text updates)
    pub fn publish_part_delta(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
        field: &str,
        delta: &str,
    ) {
        self.publish_event(
            &PART_DELTA_EVENT,
            value_or_null(PartDeltaEvent {
                session_id,
                message_id,
                part_id,
                field,
                delta,
            }),
        );
    }

    /// Create a new session
    pub fn create(&mut self, directory: impl Into<String>) -> Session {
        let session = Session::new(directory);
        self.sessions.insert(session.id.clone(), session.clone());
        self.events.push(SessionEvent::Created {
            info: session.clone(),
        });

        // Publish to Bus
        self.publish_session_event(&SESSION_CREATED_EVENT, &session);

        // Plugin hook: session.start — notify plugins of new session
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let session_id = session.id.clone();
            handle.spawn(async move {
                rocode_plugin::trigger(
                    HookContext::new(HookEvent::SessionStart).with_session(&session_id),
                )
                .await;
            });
        }

        session
    }

    /// Create a child session
    pub fn create_child(&mut self, parent_id: &str) -> Option<Session> {
        let parent = self.sessions.get(parent_id)?;
        let child = Session::child(parent);
        let child_id = child.id.clone();
        self.sessions.insert(child_id, child.clone());
        self.events.push(SessionEvent::Created {
            info: child.clone(),
        });
        self.publish_session_event(&SESSION_CREATED_EVENT, &child);
        Some(child)
    }

    /// Fork a session at a specific message
    pub fn fork(&mut self, session_id: &str, message_id: Option<&str>) -> Option<Session> {
        let original = self.sessions.get(session_id)?;
        let forked_title = original.get_forked_title();

        let mut forked = Session::child(original);
        forked.parent_id = None;
        forked.title = forked_title;

        if let Some(msg_id) = message_id {
            for msg in &original.messages {
                if msg.id == msg_id {
                    break;
                }
                forked.messages.push(msg.clone());
            }
        } else {
            forked.messages = original.messages.clone();
        }

        let forked_id = forked.id.clone();
        self.sessions.insert(forked_id, forked.clone());
        self.events.push(SessionEvent::Created {
            info: forked.clone(),
        });
        self.publish_session_event(&SESSION_CREATED_EVENT, &forked);
        Some(forked)
    }

    /// Set share info and publish session.updated.
    pub fn share(&mut self, session_id: &str, url: impl Into<String>) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.set_share(url);
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Clear share info and publish session.updated.
    pub fn unshare(&mut self, session_id: &str) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.clear_share();
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Set permission rules and publish session.updated.
    pub fn set_permission(
        &mut self,
        session_id: &str,
        permission: PermissionRuleset,
    ) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.set_permission(permission);
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Set revert info and publish session.updated.
    pub fn set_revert(&mut self, session_id: &str, revert: SessionRevert) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.set_revert(revert);
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Clear revert info and publish session.updated.
    pub fn clear_revert(&mut self, session_id: &str) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.clear_revert();
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Set summary and publish session.updated.
    pub fn set_summary(&mut self, session_id: &str, summary: SessionSummary) -> Option<Session> {
        let updated = {
            let session = self.sessions.get_mut(session_id)?;
            session.set_summary(summary);
            session.clone()
        };
        self.events.push(SessionEvent::Updated {
            info: updated.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &updated);
        Some(updated)
    }

    /// Publish command.executed event.
    pub fn publish_command_executed(
        &self,
        command_name: &str,
        session_id: &str,
        arguments: Vec<String>,
        message_id: &str,
    ) {
        self.publish_event(
            &COMMAND_EXECUTED_EVENT,
            value_or_null(CommandExecutedEvent {
                name: command_name,
                session_id,
                arguments,
                message_id,
            }),
        );
    }

    /// Get a session by ID
    pub fn get(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    /// Get a mutable session by ID
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    /// Mutate a session through a single gateway API.
    ///
    /// This is intended to be the primary write entrypoint for callers outside
    /// this crate so that state changes remain traceable and centralized.
    pub fn mutate_session<R>(
        &mut self,
        session_id: &str,
        mutator: impl FnOnce(&mut Session) -> R,
    ) -> Option<R> {
        let (result, snapshot) = {
            let session = self.sessions.get_mut(session_id)?;
            let result = mutator(session);
            (result, session.clone())
        };

        self.events.push(SessionEvent::Updated {
            info: snapshot.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &snapshot);
        Some(result)
    }

    /// Add a user message to a session via manager gateway.
    pub fn add_user_message(
        &mut self,
        session_id: &str,
        text: impl Into<String>,
    ) -> Option<SessionMessage> {
        self.mutate_session(session_id, move |session| {
            session.add_user_message(text).clone()
        })
    }

    /// Add an assistant message to a session via manager gateway.
    pub fn add_assistant_message(&mut self, session_id: &str) -> Option<SessionMessage> {
        self.mutate_session(session_id, |session| {
            session.add_assistant_message().clone()
        })
    }

    /// Set a metadata key on a session and touch it.
    pub fn set_metadata_value(
        &mut self,
        session_id: &str,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Option<()> {
        self.mutate_session(session_id, move |session| {
            session.metadata.insert(key.into(), value);
            session.touch();
        })
    }

    /// Remove a metadata key from a session and touch it.
    pub fn remove_metadata_key(&mut self, session_id: &str, key: &str) -> Option<()> {
        self.mutate_session(session_id, move |session| {
            session.metadata.remove(key);
            session.touch();
        })
    }

    /// List all sessions
    pub fn list(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }

    /// List sessions with filters
    pub fn list_filtered(&self, filter: SessionFilter) -> Vec<&Session> {
        self.list_filtered_with_total(filter).1
    }

    /// List sessions with filters, returning `(total, items)`.
    ///
    /// `total` is the count after filtering but before pagination.
    pub fn list_filtered_with_total(&self, filter: SessionFilter) -> (usize, Vec<&Session>) {
        let search = filter.search.as_deref().map(|s| s.to_lowercase());

        let mut sessions: Vec<&Session> = self
            .sessions
            .values()
            .filter(|s| {
                if let Some(ref dir) = filter.directory {
                    if s.directory != *dir {
                        return false;
                    }
                }
                if filter.roots && s.parent_id.is_some() {
                    return false;
                }
                if let Some(start) = filter.start {
                    if s.time.updated < start {
                        return false;
                    }
                }
                if let Some(ref search) = search {
                    if !s.title.to_lowercase().contains(search) {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Match storage ordering: most recently updated first.
        sessions.sort_by(|a, b| {
            b.time
                .updated
                .cmp(&a.time.updated)
                .then_with(|| b.time.created.cmp(&a.time.created))
                .then_with(|| a.id.cmp(&b.id))
        });

        let total = sessions.len();
        let offset = filter.offset.unwrap_or(0);
        let mut paged: Vec<&Session> = sessions.into_iter().skip(offset).collect();
        if let Some(limit) = filter.limit {
            paged.truncate(limit);
        }
        (total, paged)
    }

    /// Get children of a session
    pub fn children(&self, parent_id: &str) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    /// Delete a session
    pub fn delete(&mut self, id: &str) -> Option<Session> {
        let children: Vec<String> = self.children(id).iter().map(|s| s.id.clone()).collect();
        for child_id in children {
            self.delete(&child_id);
        }

        let session = self.sessions.remove(id)?;
        self.events.push(SessionEvent::Deleted {
            info: session.clone(),
        });

        // Publish to Bus
        self.publish_session_event(&SESSION_DELETED_EVENT, &session);

        // Plugin hook: session.end — notify plugins of session deletion
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let session_id = session.id.clone();
            handle.spawn(async move {
                rocode_plugin::trigger(
                    HookContext::new(HookEvent::SessionEnd).with_session(&session_id),
                )
                .await;
            });
        }

        Some(session)
    }

    /// Update a session
    pub fn update(&mut self, session: Session) {
        let id = session.id.clone();
        self.sessions.insert(id, session.clone());
        self.events.push(SessionEvent::Updated {
            info: session.clone(),
        });
        self.publish_session_event(&SESSION_UPDATED_EVENT, &session);
    }

    /// Get events (and clear them)
    pub fn drain_events(&mut self) -> Vec<SessionEvent> {
        self.events.drain(..).collect()
    }

    /// Get session count
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    // ========================================================================
    // Message/Part Operations with Bus Publishing
    // ========================================================================

    /// Update a message in a session and publish Bus event
    pub fn update_message(&mut self, session_id: &str, msg: SessionMessage) -> Option<()> {
        self.mutate_session(session_id, |session| {
            session.update_message(msg.clone());
        })?;
        self.publish_message_event(&MESSAGE_UPDATED_EVENT, &msg);
        Some(())
    }

    /// Remove a message from a session and publish Bus event
    pub fn remove_message(&mut self, session_id: &str, message_id: &str) -> Option<SessionMessage> {
        let msg =
            self.mutate_session(session_id, |session| session.remove_message(message_id))??;
        self.publish_event(
            &MESSAGE_REMOVED_EVENT,
            value_or_null(MessageRemovedEvent {
                session_id,
                message_id,
            }),
        );
        Some(msg)
    }

    /// Update a part in a message and publish Bus event
    pub fn update_part(
        &mut self,
        session_id: &str,
        message_id: &str,
        part: MessagePart,
    ) -> Option<()> {
        let updated = self.mutate_session(session_id, |session| {
            session.update_part(message_id, part.clone()).is_some()
        })?;
        if !updated {
            return None;
        }
        self.publish_part_event(&PART_UPDATED_EVENT, &part);
        Some(())
    }

    /// Remove a part from a message and publish Bus event
    pub fn remove_part(
        &mut self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
    ) -> Option<MessagePart> {
        let part = self.mutate_session(session_id, |session| {
            session.remove_part(message_id, part_id)
        })??;
        self.publish_event(
            &PART_REMOVED_EVENT,
            value_or_null(PartRemovedEvent {
                session_id,
                message_id,
                part_id,
            }),
        );
        Some(part)
    }

    /// Publish a session error event
    pub fn publish_error(&self, session_id: Option<&str>, error: serde_json::Value) {
        self.publish_event(
            &SESSION_ERROR_EVENT,
            value_or_null(SessionErrorEvent { error, session_id }),
        );
    }

    /// Publish a session diff event
    pub fn publish_diff(&self, session_id: &str, diffs: &[FileDiff]) {
        if let Ok(diff_json) = serde_json::to_value(diffs) {
            self.publish_event(
                &SESSION_DIFF_EVENT,
                value_or_null(SessionDiffEvent {
                    session_id,
                    diff: diff_json,
                }),
            );
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Set the Bus for event publishing (can be called after construction)
    pub fn set_bus(&mut self, bus: Arc<Bus>) {
        self.bus = Some(bus);
    }
}

/// Filter options for listing sessions
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub directory: Option<String>,
    pub roots: bool,
    pub start: Option<i64>,
    pub search: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

// ============================================================================
// Busy Error
// ============================================================================

#[derive(Debug, Clone)]
pub struct BusyError {
    pub session_id: String,
}

impl std::fmt::Display for BusyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session {} is busy", self.session_id)
    }
}

impl std::error::Error for BusyError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::time::{timeout, Duration};

    #[test]
    fn test_session_creation() {
        let session = Session::new("/path/to/project");
        assert!(session.id.starts_with("ses_"));
        assert!(session.title.starts_with("New session"));
        assert!(session.parent_id.is_none());
        assert!(!session.active);
    }

    #[test]
    fn test_child_session() {
        let parent = Session::new("/path/to/project");
        let child = Session::child(&parent);

        assert!(child.parent_id.is_some());
        assert_eq!(child.parent_id.unwrap(), parent.id);
        assert!(child.title.starts_with("Child session"));
    }

    #[test]
    fn test_add_messages() {
        let mut session = Session::new("/path/to/project");

        session.add_user_message("Hello");
        assert_eq!(session.message_count(), 1);

        session.add_assistant_message();
        assert_eq!(session.message_count(), 2);
    }

    #[test]
    fn test_session_manager() {
        let mut manager = SessionManager::new();

        let session = manager.create("/path/to/project");
        assert!(manager.get(&session.id).is_some());
        assert_eq!(manager.count(), 1);

        let child = manager.create_child(&session.id).unwrap();
        assert!(child.parent_id.is_some());

        manager.delete(&session.id);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_fork_title() {
        let session = Session::new("/path/to/project");
        let title1 = session.get_forked_title();
        assert!(title1.ends_with("(fork #1)"));

        let temp = Session {
            title: title1,
            ..session.clone()
        };
        let title2 = temp.get_forked_title();
        assert!(title2.ends_with("(fork #2)"));
    }

    #[test]
    fn test_auto_title_can_be_refined_but_manual_title_cannot() {
        let mut session = Session::new("/path");
        assert!(session.allows_auto_title_regeneration());

        session.set_auto_title("Immediate Title");
        assert!(session.allows_auto_title_regeneration());

        session.set_title("Manual Title");
        assert!(!session.allows_auto_title_regeneration());
    }

    #[test]
    fn test_sanitize_display_text_strips_pseudo_tool_markup() {
        let cleaned = sanitize_display_text(
            "before\nminimax:tool_call (minimax:tool_call)\n<invoke name=\"Bash\">\n<parameter name=\"command\">pwd</parameter>\n</invoke>\nafter",
        );
        assert_eq!(cleaned, "before\nafter");
    }

    #[test]
    fn test_update_message() {
        let mut session = Session::new("/path");
        let msg = session.add_user_message("Hello");
        let msg_id = msg.id.clone();

        let updated = session.get_message(&msg_id).unwrap().clone();
        session.update_message(updated);
        assert!(session.get_message(&msg_id).is_some());
    }

    #[test]
    fn test_update_message_new() {
        let mut session = Session::new("/path");
        let new_msg = SessionMessage::user(&session.id, "Brand new");
        let new_id = new_msg.id.clone();
        session.update_message(new_msg);
        assert!(session.get_message(&new_id).is_some());
        assert_eq!(session.message_count(), 1);
    }

    #[test]
    fn test_update_part() {
        let mut session = Session::new("/path");
        let msg = session.add_user_message("Hello");
        let msg_id = msg.id.clone();
        let part_id = msg.parts[0].id.clone();

        // Update existing part
        let replacement = MessagePart {
            id: part_id.clone(),
            part_type: crate::PartType::Text {
                text: "Updated".into(),
                synthetic: None,
                ignored: None,
            },
            created_at: Utc::now(),
            message_id: None,
        };
        let result = session.update_part(&msg_id, replacement);
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, part_id);
    }

    #[test]
    fn test_remove_part() {
        let mut session = Session::new("/path");
        let msg = session.add_user_message("Hello");
        let msg_id = msg.id.clone();
        let part_id = msg.parts[0].id.clone();

        let removed = session.remove_part(&msg_id, &part_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, part_id);
        assert_eq!(session.get_message(&msg_id).unwrap().parts.len(), 0);
    }

    #[test]
    fn test_remove_part_not_found() {
        let mut session = Session::new("/path");
        let msg = session.add_user_message("Hello");
        let msg_id = msg.id.clone();

        let removed = session.remove_part(&msg_id, "nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_share_unshare() {
        let mut session = Session::new("/path");

        session.share_session("https://example.com/share/123");
        assert!(session.share.is_some());
        assert_eq!(
            session.share.as_deref(),
            Some("https://example.com/share/123")
        );

        session.unshare_session();
        assert!(session.share.is_none());
    }

    #[test]
    fn test_get_usage_empty() {
        let session = Session::new("/path");
        let usage = session.get_usage();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_cost, 0.0);
    }

    #[test]
    fn test_get_usage_aggregation() {
        let mut session = Session::new("/path");

        // Add an assistant message with usage
        let msg = session.add_assistant_message();
        msg.usage = Some(MessageUsage {
            input_tokens: 100,
            output_tokens: 50,
            reasoning_tokens: 10,
            cache_write_tokens: 20,
            cache_read_tokens: 30,
            total_cost: 0.005,
        });

        // Add another assistant message with usage
        let msg2 = session.add_assistant_message();
        msg2.usage = Some(MessageUsage {
            input_tokens: 200,
            output_tokens: 100,
            reasoning_tokens: 20,
            cache_write_tokens: 40,
            cache_read_tokens: 60,
            total_cost: 0.010,
        });

        // Add a user message (should not be counted)
        let user_msg = session.add_user_message("test");
        user_msg.usage = Some(MessageUsage {
            input_tokens: 999,
            output_tokens: 999,
            reasoning_tokens: 999,
            cache_write_tokens: 999,
            cache_read_tokens: 999,
            total_cost: 999.0,
        });

        let usage = session.get_usage();
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 150);
        assert_eq!(usage.reasoning_tokens, 30);
        assert_eq!(usage.cache_write_tokens, 60);
        assert_eq!(usage.cache_read_tokens, 90);
        assert!((usage.total_cost - 0.015).abs() < f64::EPSILON);
    }

    #[test]
    fn test_diff_empty() {
        let session = Session::new("/path");
        let diffs = session.diff();
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_diff_with_summary() {
        let mut session = Session::new("/path");
        session.set_summary(SessionSummary {
            additions: 10,
            deletions: 5,
            files: 2,
            diffs: Some(vec![
                FileDiff {
                    path: "src/main.rs".into(),
                    additions: 7,
                    deletions: 3,
                },
                FileDiff {
                    path: "src/lib.rs".into(),
                    additions: 3,
                    deletions: 2,
                },
            ]),
        });

        let diffs = session.diff();
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].path, "src/main.rs");
    }

    #[tokio::test]
    async fn session_share_publishes_updated_event() {
        let bus = Arc::new(Bus::new());
        let mut manager = SessionManager::with_bus(bus.clone());
        let session = manager.create("/path");
        let mut rx = bus.subscribe_channel();

        let updated = manager
            .share(&session.id, "https://share.opencode.ai/test")
            .expect("session should exist");
        assert_eq!(
            updated.share.as_deref(),
            Some("https://share.opencode.ai/test")
        );

        let event = timeout(Duration::from_secs(1), async {
            loop {
                let event = rx.recv().await.expect("event channel closed");
                if event.event_type == SESSION_UPDATED_EVENT.event_type {
                    break event;
                }
            }
        })
        .await
        .expect("event timeout");
        assert_eq!(event.event_type, SESSION_UPDATED_EVENT.event_type);
        assert_eq!(event.properties["info"]["id"], session.id);
        assert_eq!(
            event.properties["info"]["share"]["url"],
            "https://share.opencode.ai/test"
        );
    }

    #[tokio::test]
    async fn command_executed_event_is_published() {
        let bus = Arc::new(Bus::new());
        let manager = SessionManager::with_bus(bus.clone());
        let mut rx = bus.subscribe_channel();

        manager.publish_command_executed(
            "review",
            "session-1",
            vec!["--fast".to_string()],
            "message-1",
        );

        let event = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event timeout")
            .expect("event channel closed");
        assert_eq!(event.event_type, COMMAND_EXECUTED_EVENT.event_type);
        assert_eq!(event.properties["name"], "review");
        assert_eq!(event.properties[wire_keys::SESSION_ID], "session-1");
        assert_eq!(event.properties["arguments"][0], "--fast");
        assert_eq!(event.properties[wire_keys::MESSAGE_ID], "message-1");
    }
}
