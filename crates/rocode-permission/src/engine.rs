use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use rocode_plugin::{HookContext, HookEvent};

use crate::matching::wildcard_match;
use crate::{
    evaluate_permission_patterns, tool_to_permission, PermissionAction, PermissionRuleset,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskOutcome {
    Granted,
    Pending,
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

    pub fn find(&self, permission_id: &str) -> Option<&PermissionInfo> {
        self.pending
            .values()
            .find_map(|items| items.get(permission_id).map(|item| &item.info))
    }

    fn to_keys(pattern: Option<&Pattern>, permission_type: &str) -> Vec<String> {
        match pattern {
            None => vec![permission_type.to_string()],
            Some(Pattern::Single(s)) => vec![s.clone()],
            Some(Pattern::Multiple(v)) => v.clone(),
        }
    }

    fn patterns(pattern: Option<&Pattern>) -> Vec<String> {
        match pattern {
            None => Vec::new(),
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

    pub fn grant(&mut self, session_id: &str, permission_type: &str, pattern: Option<&Pattern>) {
        let approved_session = self.approved.entry(session_id.to_string()).or_default();
        for key in Self::to_keys(pattern, permission_type) {
            approved_session.insert(key, true);
        }
    }

    pub fn grant_patterns(&mut self, session_id: &str, permission_type: &str, patterns: &[String]) {
        let pattern = match patterns {
            [] => None,
            [single] => Some(Pattern::Single(single.clone())),
            _ => Some(Pattern::Multiple(patterns.to_vec())),
        };
        self.grant(session_id, permission_type, pattern.as_ref());
    }

    pub fn evaluate_tool(
        tool_name: &str,
        allowed_tools: &[String],
        rulesets: &[PermissionRuleset],
    ) -> PermissionAction {
        Self::evaluate_tool_with_patterns(tool_name, &[], allowed_tools, rulesets)
    }

    pub fn evaluate_tool_with_patterns(
        tool_name: &str,
        patterns: &[String],
        allowed_tools: &[String],
        rulesets: &[PermissionRuleset],
    ) -> PermissionAction {
        if !allowed_tools.is_empty() && !allowed_tools.iter().any(|tool| tool == tool_name) {
            return PermissionAction::Deny;
        }

        let permission = tool_to_permission(tool_name);
        evaluate_permission_patterns(permission, patterns, rulesets)
    }

    pub async fn ask(&mut self, info: PermissionInfo) -> Result<AskOutcome, PermissionError> {
        self.ask_with_rules(info, &[]).await
    }

    pub async fn ask_with_rules(
        &mut self,
        info: PermissionInfo,
        rulesets: &[PermissionRuleset],
    ) -> Result<AskOutcome, PermissionError> {
        let session_id = info.session_id.clone();
        let permission_id = info.id.clone();
        let patterns = Self::patterns(info.pattern.as_ref());

        if self.is_approved(&session_id, info.pattern.as_ref(), &info.permission_type) {
            return Ok(AskOutcome::Granted);
        }

        match evaluate_permission_patterns(&info.permission_type, &patterns, rulesets) {
            PermissionAction::Allow => return Ok(AskOutcome::Granted),
            PermissionAction::Deny => {
                return Err(PermissionError::Rejected {
                    session_id: session_id.clone(),
                    permission_id: permission_id.clone(),
                    tool_call_id: info.call_id.clone(),
                });
            }
            PermissionAction::Ask => {}
        }

        // Plugin hook: permission.ask — plugins may decide "ask" | "deny" | "allow".
        let mut hook_ctx = HookContext::new(HookEvent::PermissionAsk)
            .with_session(&session_id)
            .with_data("permission_type", serde_json::json!(&info.permission_type))
            .with_data("permission_id", serde_json::json!(&permission_id))
            .with_data("permission", serde_json::json!(&info))
            .with_data("status", serde_json::json!("ask"));
        if let Some(call_id) = &info.call_id {
            hook_ctx = hook_ctx.with_data("call_id", serde_json::json!(call_id));
        }

        let mut status = "ask".to_string();
        let hook_outputs = rocode_plugin::trigger_collect(hook_ctx).await;
        for output in hook_outputs {
            let Some(payload) = output.payload.as_ref() else {
                continue;
            };
            if let Some(next_status) = extract_permission_status(payload) {
                status = next_status;
            }
        }

        match status.as_str() {
            "allow" => return Ok(AskOutcome::Granted),
            "deny" => {
                return Err(PermissionError::Rejected {
                    session_id: session_id.clone(),
                    permission_id: permission_id.clone(),
                    tool_call_id: info.call_id.clone(),
                });
            }
            _ => {}
        }

        self.pending
            .entry(session_id.clone())
            .or_default()
            .insert(permission_id, PendingPermission { info });

        Ok(AskOutcome::Pending)
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
            self.grant(
                session_id,
                &match_item.info.permission_type,
                match_item.info.pattern.as_ref(),
            );
        }

        Ok(())
    }

    pub fn respond_by_id(
        &mut self,
        permission_id: &str,
        response: Response,
    ) -> Result<PermissionInfo, PermissionError> {
        let session_id = self
            .pending
            .iter()
            .find_map(|(session_id, items)| items.contains_key(permission_id).then_some(session_id))
            .cloned()
            .ok_or_else(|| PermissionError::NotFound("*".to_string(), permission_id.to_string()))?;

        let info = self.find(permission_id).cloned().ok_or_else(|| {
            PermissionError::NotFound(session_id.clone(), permission_id.to_string())
        })?;

        self.respond(&session_id, permission_id, response)?;
        Ok(info)
    }

    pub fn remove_pending(&mut self, permission_id: &str) -> Option<PermissionInfo> {
        let session_id = self
            .pending
            .iter()
            .find_map(|(session_id, items)| items.contains_key(permission_id).then_some(session_id))
            .cloned()?;

        let items = self.pending.get_mut(&session_id)?;
        let removed = items.remove(permission_id)?;
        if items.is_empty() {
            self.pending.remove(&session_id);
        }
        Some(removed.info)
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

fn extract_permission_status(payload: &serde_json::Value) -> Option<String> {
    rocode_plugin::hook_payload_object(payload)
        .and_then(|object| object.get("status"))
        .and_then(|value| value.as_str())
        .filter(|status| matches!(*status, "ask" | "deny" | "allow"))
        .map(ToString::to_string)
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

    #[tokio::test]
    async fn test_permission_engine() {
        let mut engine = PermissionEngine::new();

        let info = PermissionInfo {
            id: "per_test".to_string(),
            permission_type: "bash".to_string(),
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
