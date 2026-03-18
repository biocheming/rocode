use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rocode_core::bus::{Bus, BusEventDef};
use rocode_core::contracts::{events::BusEventName, wire};

// ============================================================================
// Bus event definitions (matches TS SessionStatus.Event)
// ============================================================================

/// Event published when a session's run status changes.
pub static SESSION_STATUS_EVENT: BusEventDef =
    BusEventDef::new(BusEventName::SessionStatus.as_str());

/// Deprecated event published when a session becomes idle.
pub static SESSION_IDLE_EVENT: BusEventDef = BusEventDef::new(BusEventName::SessionIdle.as_str());

// ============================================================================
// Status types (matches TS SessionStatus.Info union type)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type")]
pub enum SessionStatusInfo {
    #[serde(rename = "idle")]
    #[default]
    Idle,
    #[serde(rename = "retry")]
    Retry {
        attempt: u32,
        message: String,
        next: u64,
    },
    #[serde(rename = "busy")]
    Busy,
}

// ============================================================================
// Status manager (matches TS SessionStatus namespace)
// ============================================================================

pub struct SessionStatusManager {
    state: Arc<RwLock<HashMap<String, SessionStatusInfo>>>,
    bus: Option<Arc<Bus>>,
}

impl SessionStatusManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            bus: None,
        }
    }

    /// Create a status manager with bus event publishing.
    pub fn with_bus(bus: Arc<Bus>) -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            bus: Some(bus),
        }
    }

    /// Get the status for a session. Returns Idle if not tracked.
    /// Matches TS `SessionStatus.get(sessionID)`.
    pub async fn get(&self, session_id: &str) -> SessionStatusInfo {
        let state = self.state.read().await;
        state.get(session_id).cloned().unwrap_or_default()
    }

    /// List all tracked session statuses.
    /// Matches TS `SessionStatus.list()`.
    pub async fn list(&self) -> HashMap<String, SessionStatusInfo> {
        let state = self.state.read().await;
        state.clone()
    }

    /// Set the status for a session and publish bus events.
    /// Matches TS `SessionStatus.set(sessionID, status)`.
    pub async fn set(&self, session_id: &str, status: SessionStatusInfo) {
        // Publish status event
        if let Some(ref bus) = self.bus {
            let mut event_data = serde_json::Map::new();
            event_data.insert(
                wire::keys::SESSION_ID.to_string(),
                serde_json::Value::String(session_id.to_string()),
            );
            event_data.insert(
                "status".to_string(),
                serde_json::to_value(&status).unwrap_or(serde_json::Value::Null),
            );
            bus.publish(&SESSION_STATUS_EVENT, serde_json::Value::Object(event_data))
                .await;
        }

        let mut state = self.state.write().await;
        match &status {
            SessionStatusInfo::Idle => {
                // Publish deprecated idle event
                if let Some(ref bus) = self.bus {
                    let mut idle_data = serde_json::Map::new();
                    idle_data.insert(
                        wire::keys::SESSION_ID.to_string(),
                        serde_json::Value::String(session_id.to_string()),
                    );
                    bus.publish(&SESSION_IDLE_EVENT, serde_json::Value::Object(idle_data))
                        .await;
                }
                state.remove(session_id);
            }
            _ => {
                state.insert(session_id.to_string(), status);
            }
        }
    }

    /// Convenience: set status to idle.
    pub async fn set_idle(&self, session_id: &str) {
        self.set(session_id, SessionStatusInfo::Idle).await;
    }

    /// Convenience: set status to busy.
    pub async fn set_busy(&self, session_id: &str) {
        self.set(session_id, SessionStatusInfo::Busy).await;
    }

    /// Convenience: set status to retry with details.
    /// Matches TS retry status with message and next timestamp.
    pub async fn set_retry(&self, session_id: &str, attempt: u32, message: String, next: u64) {
        self.set(
            session_id,
            SessionStatusInfo::Retry {
                attempt,
                message,
                next,
            },
        )
        .await;
    }

    /// Check if a session is busy (busy or retrying).
    pub async fn is_busy(&self, session_id: &str) -> bool {
        let state = self.state.read().await;
        matches!(
            state.get(session_id),
            Some(SessionStatusInfo::Busy | SessionStatusInfo::Retry { .. })
        )
    }
}

impl Default for SessionStatusManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_is_idle() {
        let mgr = SessionStatusManager::new();
        let status = mgr.get("ses_123").await;
        assert!(matches!(status, SessionStatusInfo::Idle));
    }

    #[tokio::test]
    async fn test_set_busy() {
        let mgr = SessionStatusManager::new();
        mgr.set_busy("ses_123").await;
        assert!(mgr.is_busy("ses_123").await);
    }

    #[tokio::test]
    async fn test_set_idle_removes() {
        let mgr = SessionStatusManager::new();
        mgr.set_busy("ses_123").await;
        mgr.set_idle("ses_123").await;
        assert!(!mgr.is_busy("ses_123").await);
        let list = mgr.list().await;
        assert!(!list.contains_key("ses_123"));
    }

    #[tokio::test]
    async fn test_set_retry() {
        let mgr = SessionStatusManager::new();
        mgr.set_retry("ses_123", 2, "Rate limited".to_string(), 1700000000000)
            .await;
        let status = mgr.get("ses_123").await;
        match status {
            SessionStatusInfo::Retry {
                attempt,
                message,
                next,
            } => {
                assert_eq!(attempt, 2);
                assert_eq!(message, "Rate limited");
                assert_eq!(next, 1700000000000);
            }
            _ => panic!("Expected Retry status"),
        }
        assert!(mgr.is_busy("ses_123").await);
    }

    #[tokio::test]
    async fn test_list() {
        let mgr = SessionStatusManager::new();
        mgr.set_busy("ses_1").await;
        mgr.set_busy("ses_2").await;
        let list = mgr.list().await;
        assert_eq!(list.len(), 2);
        assert!(list.contains_key("ses_1"));
        assert!(list.contains_key("ses_2"));
    }

    #[tokio::test]
    async fn test_with_bus() {
        let bus = Arc::new(Bus::new());
        let mgr = SessionStatusManager::with_bus(bus);
        mgr.set_busy("ses_123").await;
        assert!(mgr.is_busy("ses_123").await);
    }
}
