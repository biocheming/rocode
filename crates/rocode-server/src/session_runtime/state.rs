//! Per-session aggregated runtime state.
//!
//! `SessionRuntimeState` is the server's single authoritative projection of
//! what a session is *doing right now*. It is maintained incrementally by the
//! existing lifecycle hooks (`SessionSchedulerLifecycleHook`, question/permission
//! routes) and exposed via `GET /session/{id}/runtime`.
//!
//! Design constraints (from the ROCode Constitution):
//! - Article 5 — unique state ownership: the `RuntimeStateStore` is the sole
//!   owner; consumers read through its API.
//! - Article 8 — observability: every active execution aspect must be
//!   reflected here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// ── Primary state struct ────────────────────────────────────────────────────

/// Aggregated runtime snapshot for a single session.
///
/// Fields are kept intentionally flat and cheap to clone so that the
/// `GET /session/{id}/runtime` endpoint can return a snapshot without
/// holding the lock across serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntimeState {
    pub session_id: String,
    pub run_status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_reason: Option<PendingReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_message_id: Option<String>,
    pub active_tools: Vec<ActiveToolSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<PendingQuestionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_permission: Option<PendingPermissionSummary>,
    pub child_sessions: Vec<ChildSessionSummary>,
}

impl SessionRuntimeState {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            run_status: RunStatus::Idle,
            pending_reason: None,
            error_message: None,
            current_message_id: None,
            active_tools: Vec::new(),
            pending_question: None,
            pending_permission: None,
            child_sessions: Vec::new(),
        }
    }
}

// ── Supporting types ────────────────────────────────────────────────────────

/// Coarse run-status for the session, derived from lifecycle hooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    WaitingOnTool,
    #[serde(alias = "waiting_on_user")]
    Pending,
    Cancelling,
    Error,
}

/// Why a session is currently in `RunStatus::Pending`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    Question,
    Permission,
    QuestionAndPermission,
}

impl PendingReason {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "question" => Some(Self::Question),
            "permission" => Some(Self::Permission),
            "question_and_permission" => Some(Self::QuestionAndPermission),
            _ => None,
        }
    }
}

impl Default for RunStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// Summary of a currently executing tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveToolSummary {
    pub tool_call_id: String,
    pub tool_name: String,
    /// Monotonic timestamp (epoch millis) when the tool started.
    pub started_at: i64,
}

/// Summary of a pending question awaiting user answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingQuestionSummary {
    pub request_id: String,
    pub questions: serde_json::Value,
}

/// Summary of a pending permission request awaiting user decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermissionSummary {
    pub permission_id: String,
    pub info: serde_json::Value,
}

/// Summary of an attached child session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildSessionSummary {
    pub child_id: String,
    pub parent_id: String,
}

// ── Store ───────────────────────────────────────────────────────────────────

/// Process-wide store of per-session runtime state.
///
/// Uses `tokio::sync::RwLock` for read-heavy access (SSE consumers poll,
/// REST endpoint reads) with infrequent writes (lifecycle hooks).
#[derive(Debug)]
pub struct RuntimeStateStore {
    states: RwLock<HashMap<String, SessionRuntimeState>>,
}

impl RuntimeStateStore {
    fn derive_pending_reason(state: &SessionRuntimeState) -> Option<PendingReason> {
        match (
            state.pending_question.is_some(),
            state.pending_permission.is_some(),
        ) {
            (true, true) => Some(PendingReason::QuestionAndPermission),
            (true, false) => Some(PendingReason::Question),
            (false, true) => Some(PendingReason::Permission),
            (false, false) => None,
        }
    }

    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
        }
    }

    /// Get a cloned snapshot of a session's runtime state.
    pub async fn get(&self, session_id: &str) -> Option<SessionRuntimeState> {
        let guard = self.states.read().await;
        guard.get(session_id).cloned()
    }

    /// Apply a mutation to a session's runtime state.
    ///
    /// If the session does not yet exist in the store, a new default entry
    /// is created before the mutation is applied.
    pub async fn update<F>(&self, session_id: &str, f: F)
    where
        F: FnOnce(&mut SessionRuntimeState),
    {
        let mut guard = self.states.write().await;
        let state = guard
            .entry(session_id.to_string())
            .or_insert_with(|| SessionRuntimeState::new(session_id));
        f(state);
    }

    /// Remove a session's runtime state (e.g. on session delete).
    pub async fn remove(&self, session_id: &str) {
        let mut guard = self.states.write().await;
        guard.remove(session_id);
    }

    // ── Convenience mutators ────────────────────────────────────────────

    /// Mark the session as running with the given message id.
    pub async fn mark_running(&self, session_id: &str, message_id: Option<String>) {
        self.update(session_id, |s| {
            s.run_status = RunStatus::Running;
            s.pending_reason = None;
            s.error_message = None;
            s.current_message_id = message_id;
        })
        .await;
    }

    /// Mark the session as idle, clearing transient state.
    pub async fn mark_idle(&self, session_id: &str) {
        self.update(session_id, |s| {
            s.run_status = RunStatus::Idle;
            s.pending_reason = None;
            s.error_message = None;
            s.current_message_id = None;
            s.active_tools.clear();
            s.pending_question = None;
            s.pending_permission = None;
            // child_sessions are NOT cleared here — they persist until
            // explicit detach events.
        })
        .await;
    }

    /// Register a tool call start.
    pub async fn tool_started(&self, session_id: &str, tool_call_id: &str, tool_name: &str) {
        self.update(session_id, |s| {
            s.run_status = RunStatus::WaitingOnTool;
            s.pending_reason = None;
            s.error_message = None;
            s.active_tools.push(ActiveToolSummary {
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                started_at: chrono::Utc::now().timestamp_millis(),
            });
        })
        .await;
    }

    /// Register a tool call end.
    pub async fn tool_ended(&self, session_id: &str, tool_call_id: &str) {
        self.update(session_id, |s| {
            s.active_tools.retain(|t| t.tool_call_id != tool_call_id);
            // If no more tools are active, revert to Running.
            if s.active_tools.is_empty() && s.run_status == RunStatus::WaitingOnTool {
                s.run_status = RunStatus::Running;
            }
        })
        .await;
    }

    /// Set a pending question.
    pub async fn question_created(
        &self,
        session_id: &str,
        request_id: &str,
        questions: serde_json::Value,
    ) {
        self.update(session_id, |s| {
            s.pending_question = Some(PendingQuestionSummary {
                request_id: request_id.to_string(),
                questions,
            });
            s.pending_reason = Self::derive_pending_reason(s);
            s.run_status = RunStatus::Pending;
        })
        .await;
    }

    /// Clear a pending question.
    pub async fn question_resolved(&self, session_id: &str) {
        self.update(session_id, |s| {
            s.pending_question = None;
            s.pending_reason = Self::derive_pending_reason(s);
            // Revert to Running only if not waiting on something else.
            s.run_status = if s.pending_reason.is_some() {
                RunStatus::Pending
            } else {
                RunStatus::Running
            };
        })
        .await;
    }

    /// Set a pending permission request.
    pub async fn permission_requested(
        &self,
        session_id: &str,
        permission_id: &str,
        info: serde_json::Value,
    ) {
        self.update(session_id, |s| {
            s.pending_permission = Some(PendingPermissionSummary {
                permission_id: permission_id.to_string(),
                info,
            });
            s.pending_reason = Self::derive_pending_reason(s);
            s.run_status = RunStatus::Pending;
        })
        .await;
    }

    /// Clear a pending permission request.
    pub async fn permission_resolved(&self, session_id: &str) {
        self.update(session_id, |s| {
            s.pending_permission = None;
            s.pending_reason = Self::derive_pending_reason(s);
            // Revert to Running only if not waiting on something else.
            s.run_status = if s.pending_reason.is_some() {
                RunStatus::Pending
            } else {
                RunStatus::Running
            };
        })
        .await;
    }

    /// Mark the session as pending with an explicit reason.
    pub async fn mark_pending(&self, session_id: &str, reason: String) {
        self.update(session_id, |s| {
            s.run_status = RunStatus::Pending;
            s.pending_reason = PendingReason::from_str(reason.as_str());
            s.error_message = None;
        })
        .await;
    }

    /// Mark the session as ended with an error message.
    pub async fn mark_error(&self, session_id: &str, error_message: String) {
        self.update(session_id, |s| {
            s.run_status = RunStatus::Error;
            s.error_message = Some(error_message);
            s.pending_reason = None;
            s.current_message_id = None;
            s.active_tools.clear();
            s.pending_question = None;
            s.pending_permission = None;
        })
        .await;
    }

    /// Register a child session attachment.
    pub async fn child_attached(&self, parent_id: &str, child_id: &str) {
        self.update(parent_id, |s| {
            // Avoid duplicates.
            if !s.child_sessions.iter().any(|c| c.child_id == child_id) {
                s.child_sessions.push(ChildSessionSummary {
                    child_id: child_id.to_string(),
                    parent_id: parent_id.to_string(),
                });
            }
        })
        .await;
    }

    /// Unregister a child session detachment.
    pub async fn child_detached(&self, parent_id: &str, child_id: &str) {
        self.update(parent_id, |s| {
            s.child_sessions.retain(|c| c.child_id != child_id);
        })
        .await;
    }
}

impl Default for RuntimeStateStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_session_starts_idle() {
        let store = RuntimeStateStore::new();
        let state = store.get("ses_1").await;
        assert!(state.is_none(), "unknown session returns None");

        store.mark_idle("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Idle);
        assert!(state.active_tools.is_empty());
    }

    #[tokio::test]
    async fn mark_running_then_idle_clears_transient_state() {
        let store = RuntimeStateStore::new();

        store
            .mark_running("ses_1", Some("msg_001".to_string()))
            .await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Running);
        assert_eq!(state.current_message_id.as_deref(), Some("msg_001"));

        store.tool_started("ses_1", "tc_1", "bash").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::WaitingOnTool);
        assert_eq!(state.active_tools.len(), 1);

        store.mark_idle("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Idle);
        assert!(state.active_tools.is_empty());
        assert!(state.current_message_id.is_none());
    }

    #[tokio::test]
    async fn tool_end_reverts_to_running_when_no_more_tools() {
        let store = RuntimeStateStore::new();
        store.mark_running("ses_1", None).await;
        store.tool_started("ses_1", "tc_1", "read").await;
        store.tool_started("ses_1", "tc_2", "write").await;
        assert_eq!(store.get("ses_1").await.unwrap().active_tools.len(), 2);

        store.tool_ended("ses_1", "tc_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.active_tools.len(), 1);
        // Still WaitingOnTool because tc_2 is active.
        assert_eq!(state.run_status, RunStatus::WaitingOnTool);

        store.tool_ended("ses_1", "tc_2").await;
        let state = store.get("ses_1").await.unwrap();
        assert!(state.active_tools.is_empty());
        assert_eq!(state.run_status, RunStatus::Running);
    }

    #[tokio::test]
    async fn question_and_permission_lifecycle() {
        let store = RuntimeStateStore::new();
        store.mark_running("ses_1", None).await;

        store
            .question_created("ses_1", "q_1", serde_json::json!([{"question": "ok?"}]))
            .await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Pending);
        assert_eq!(state.pending_reason, Some(PendingReason::Question));
        assert!(state.pending_question.is_some());

        store.question_resolved("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Running);
        assert!(state.pending_question.is_none());

        store
            .permission_requested("ses_1", "perm_1", serde_json::json!({"tool": "bash"}))
            .await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Pending);
        assert_eq!(state.pending_reason, Some(PendingReason::Permission));
        assert!(state.pending_permission.is_some());

        store.permission_resolved("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Running);
        assert!(state.pending_permission.is_none());
    }

    #[tokio::test]
    async fn child_session_attach_detach() {
        let store = RuntimeStateStore::new();
        store.mark_running("parent", None).await;

        store.child_attached("parent", "child_1").await;
        store.child_attached("parent", "child_2").await;
        let state = store.get("parent").await.unwrap();
        assert_eq!(state.child_sessions.len(), 2);

        // Duplicate attach is idempotent.
        store.child_attached("parent", "child_1").await;
        assert_eq!(store.get("parent").await.unwrap().child_sessions.len(), 2);

        store.child_detached("parent", "child_1").await;
        let state = store.get("parent").await.unwrap();
        assert_eq!(state.child_sessions.len(), 1);
        assert_eq!(state.child_sessions[0].child_id, "child_2");
    }

    #[tokio::test]
    async fn remove_cleans_up() {
        let store = RuntimeStateStore::new();
        store.mark_running("ses_1", None).await;
        assert!(store.get("ses_1").await.is_some());

        store.remove("ses_1").await;
        assert!(store.get("ses_1").await.is_none());
    }

    #[tokio::test]
    async fn concurrent_question_and_permission() {
        let store = RuntimeStateStore::new();
        store.mark_running("ses_1", None).await;

        // Both question and permission pending simultaneously.
        store
            .question_created("ses_1", "q_1", serde_json::json!("q"))
            .await;
        store
            .permission_requested("ses_1", "p_1", serde_json::json!("p"))
            .await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Pending);
        assert_eq!(
            state.pending_reason,
            Some(PendingReason::QuestionAndPermission)
        );

        // Resolving question alone should NOT revert to Running
        // because permission is still pending.
        store.question_resolved("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Pending);
        assert_eq!(state.pending_reason, Some(PendingReason::Permission));

        store.permission_resolved("ses_1").await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Running);
        assert!(state.pending_reason.is_none());
    }

    #[tokio::test]
    async fn mark_error_sets_error_status_and_clears_pending_state() {
        let store = RuntimeStateStore::new();
        store
            .mark_running("ses_1", Some("msg_001".to_string()))
            .await;
        store
            .question_created("ses_1", "q_1", serde_json::json!([{"question": "ok?"}]))
            .await;

        store
            .mark_error("ses_1", "provider timeout".to_string())
            .await;

        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Error);
        assert_eq!(state.error_message.as_deref(), Some("provider timeout"));
        assert!(state.pending_reason.is_none());
        assert!(state.pending_question.is_none());
        assert!(state.pending_permission.is_none());
        assert!(state.active_tools.is_empty());
    }

    #[test]
    fn pending_status_serializes_with_reason() {
        let state = SessionRuntimeState {
            session_id: "ses_1".to_string(),
            run_status: RunStatus::Pending,
            pending_reason: Some(PendingReason::Question),
            error_message: None,
            current_message_id: None,
            active_tools: Vec::new(),
            pending_question: None,
            pending_permission: None,
            child_sessions: Vec::new(),
        };

        let value = serde_json::to_value(state).expect("serialize pending runtime state");
        assert_eq!(value["run_status"], "pending");
        assert_eq!(value["pending_reason"], "question");
    }

    #[test]
    fn legacy_waiting_on_user_deserializes_as_pending() {
        let state: SessionRuntimeState = serde_json::from_value(serde_json::json!({
            "session_id": "ses_1",
            "run_status": "waiting_on_user",
            "pending_question": {"request_id": "q_1", "questions": []},
            "active_tools": [],
            "child_sessions": []
        }))
        .expect("deserialize legacy waiting_on_user");

        assert_eq!(state.run_status, RunStatus::Pending);
    }

    #[tokio::test]
    async fn mark_pending_maps_reason_and_sets_pending_status() {
        let store = RuntimeStateStore::new();

        store.mark_pending("ses_1", "question".to_string()).await;
        let state = store.get("ses_1").await.unwrap();
        assert_eq!(state.run_status, RunStatus::Pending);
        assert_eq!(state.pending_reason, Some(PendingReason::Question));
    }
}
