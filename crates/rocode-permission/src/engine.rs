use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use rocode_core::contracts::permission::PermissionHookStatus;
use rocode_plugin::{HookContext, HookEvent};

use crate::matching::wildcard_match;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionInfo {
    pub id: String,
    pub permission_type: String,
    pub pattern: Option<Pattern>,
    pub session_id: String,
    pub message_id: String,
    pub call_id: Option<String>,
    pub message: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub time: TimeInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeInfo {
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pattern {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Once,
    Always,
    Reject,
}

#[derive(Debug, Clone)]
pub struct PendingPermission {
    pub info: PermissionInfo,
}

pub struct PermissionEngine {
    pending: HashMap<String, HashMap<String, PendingPermission>>,
    approved: HashMap<String, HashMap<String, bool>>,
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            approved: HashMap::new(),
        }
    }

    pub fn pending(&self) -> &HashMap<String, HashMap<String, PendingPermission>> {
        &self.pending
    }

    pub fn list(&self) -> Vec<&PermissionInfo> {
        let mut result: Vec<&PermissionInfo> = Vec::new();
        for items in self.pending.values() {
            for item in items.values() {
                result.push(&item.info);
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        result
    }

    fn to_keys(pattern: Option<&Pattern>, permission_type: &str) -> Vec<String> {
        match pattern {
            None => vec![permission_type.to_string()],
            Some(Pattern::Single(s)) => vec![s.clone()],
            Some(Pattern::Multiple(v)) => v.clone(),
        }
    }

    fn covered(keys: &[String], approved: &HashMap<String, bool>) -> bool {
        let patterns: Vec<&String> = approved.keys().collect();
        keys.iter()
            .all(|k| patterns.iter().any(|p| wildcard_match(k, p)))
    }

    pub fn is_approved(
        &self,
        session_id: &str,
        pattern: Option<&Pattern>,
        permission_type: &str,
    ) -> bool {
        let empty = HashMap::new();
        let approved_for_session = self.approved.get(session_id).unwrap_or(&empty);
        let keys = Self::to_keys(pattern, permission_type);
        Self::covered(&keys, approved_for_session)
    }

    pub async fn ask(&mut self, info: PermissionInfo) -> Result<(), PermissionError> {
        let session_id = info.session_id.clone();
        let permission_id = info.id.clone();

        if self.is_approved(&session_id, info.pattern.as_ref(), &info.permission_type) {
            return Ok(());
        }

        // Plugin hook: permission.ask — plugins may decide "ask" | "deny" | "allow".
        let mut hook_ctx = HookContext::new(HookEvent::PermissionAsk)
            .with_session(&session_id)
            .with_data("permission_type", serde_json::json!(&info.permission_type))
            .with_data("permission_id", serde_json::json!(&permission_id))
            .with_data("permission", serde_json::json!(&info))
            .with_data(
                "status",
                serde_json::json!(PermissionHookStatus::Ask.as_str()),
            );
        if let Some(call_id) = &info.call_id {
            hook_ctx = hook_ctx.with_data("call_id", serde_json::json!(call_id));
        }

        let mut status = PermissionHookStatus::Ask;
        let hook_outputs = rocode_plugin::trigger_collect(hook_ctx).await;
        for output in hook_outputs {
            let Some(payload) = output.payload.as_ref() else {
                continue;
            };
            if let Some(next_status) = extract_permission_status(payload) {
                status = next_status;
            }
        }

        match status {
            PermissionHookStatus::Allow => return Ok(()),
            PermissionHookStatus::Deny => {
                return Err(PermissionError::Rejected {
                    session_id: session_id.clone(),
                    permission_id: permission_id.clone(),
                    tool_call_id: info.call_id.clone(),
                });
            }
            PermissionHookStatus::Ask => {}
        }

        self.pending
            .entry(session_id.clone())
            .or_default()
            .insert(permission_id, PendingPermission { info });

        Ok(())
    }

    pub fn respond(
        &mut self,
        session_id: &str,
        permission_id: &str,
        response: Response,
    ) -> Result<(), PermissionError> {
        let session_pending = self.pending.get_mut(session_id).ok_or_else(|| {
            PermissionError::NotFound(session_id.to_string(), permission_id.to_string())
        })?;

        let match_item = session_pending.remove(permission_id).ok_or_else(|| {
            PermissionError::NotFound(session_id.to_string(), permission_id.to_string())
        })?;

        if response == Response::Reject {
            return Err(PermissionError::Rejected {
                session_id: session_id.to_string(),
                permission_id: permission_id.to_string(),
                tool_call_id: match_item.info.call_id.clone(),
            });
        }

        if response == Response::Always {
            let approved_session = self.approved.entry(session_id.to_string()).or_default();
            let approve_keys = Self::to_keys(
                match_item.info.pattern.as_ref(),
                &match_item.info.permission_type,
            );
            for k in approve_keys {
                approved_session.insert(k, true);
            }
        }

        Ok(())
    }

    pub fn clear_session(&mut self, session_id: &str) {
        self.pending.remove(session_id);
        self.approved.remove(session_id);
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn hook_payload_object(
    payload: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    payload
        .get("output")
        .and_then(|value| value.as_object())
        .or_else(|| payload.as_object())
        .or_else(|| payload.get("data").and_then(|value| value.as_object()))
}

fn extract_permission_status(payload: &serde_json::Value) -> Option<PermissionHookStatus> {
    hook_payload_object(payload)
        .and_then(|object| object.get("status"))
        .and_then(|value| value.as_str())
        .and_then(PermissionHookStatus::parse)
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("Permission not found: {0}/{1}")]
    NotFound(String, String),

    #[error("Permission rejected")]
    Rejected {
        session_id: String,
        permission_id: String,
        tool_call_id: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocode_core::contracts::tools::BuiltinToolName;

    #[tokio::test]
    async fn test_permission_engine() {
        let mut engine = PermissionEngine::new();

        let info = PermissionInfo {
            id: "per_test".to_string(),
            permission_type: BuiltinToolName::Bash.as_str().to_string(),
            pattern: Some(Pattern::Single("ls".to_string())),
            session_id: "ses_test".to_string(),
            message_id: "msg_test".to_string(),
            call_id: None,
            message: "Execute ls command".to_string(),
            metadata: HashMap::new(),
            time: TimeInfo { created: 0 },
        };

        engine.ask(info).await.unwrap();
        assert!(!engine.list().is_empty());

        engine
            .respond("ses_test", "per_test", Response::Once)
            .unwrap();
        assert!(engine.list().is_empty());
    }

    #[test]
    fn test_wildcard_match() {
        assert!(wildcard_match("foo", "*"));
        assert!(wildcard_match("foo/bar", "foo/*"));
        assert!(wildcard_match("foo/bar/baz", "*/baz"));
        assert!(wildcard_match("foo/bar/baz", "*bar*"));
        assert!(!wildcard_match("foo", "bar"));
    }
}
