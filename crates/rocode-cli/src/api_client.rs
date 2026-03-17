//! Async HTTP client for the CLI to communicate with the ROCode server.
//!
//! Mirrors the TUI's `ApiClient` (which uses `reqwest::blocking`) but is
//! fully async so it integrates naturally with the CLI's `tokio::select!`
//! event loop.
//!
//! Data types are re-exported from `rocode_tui::api` where possible to
//! avoid duplication (Constitution §2 — unique configuration truth).

use std::time::Duration;

use crate::util::server_url;
use rocode_config::Config;

// Re-export shared types from TUI api module so callers don't need to
// depend on rocode_tui directly.
pub use rocode_tui::api::{
    AgentInfo, CompactResponse, CreateSessionRequest, ExecuteRecoveryRequest, ExecuteShellRequest,
    ExecutionModeInfo, FullProviderListResponse, KnownProvidersResponse, McpAuthStartInfo,
    McpStatusInfo, MessageInfo, MessageTokensInfo, PermissionRequestInfo, PromptRequest,
    ProviderListResponse, QuestionInfo, RecoveryActionKind, RevertRequest, RevertResponse,
    SessionExecutionTopology, SessionInfo, SessionRecoveryProtocol, SessionRuntimeState,
    SessionStatusInfo, ShareResponse, UpdateSessionRequest,
};

/// Async HTTP client for communicating with the ROCode server.
pub struct CliApiClient {
    client: reqwest::Client,
    base_url: String,
}

#[allow(dead_code)]
impl CliApiClient {
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, base_url }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ── Session ──────────────────────────────────────────────────────

    pub async fn create_session(
        &self,
        parent_id: Option<String>,
        scheduler_profile: Option<String>,
    ) -> anyhow::Result<SessionInfo> {
        let url = server_url(&self.base_url, "/session");
        let req = CreateSessionRequest {
            parent_id,
            scheduler_profile,
        };
        let resp = self.client.post(&url).json(&req).send().await?;
        Self::json_ok(resp, "create session").await
    }

    pub async fn get_session(&self, session_id: &str) -> anyhow::Result<SessionInfo> {
        let url = server_url(&self.base_url, &format!("/session/{}", session_id));
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get session").await
    }

    pub async fn list_sessions(
        &self,
        search: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<SessionInfo>> {
        let url = server_url(&self.base_url, "/session");
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(s) = search.map(str::trim).filter(|s| !s.is_empty()) {
            params.push(("search", s.to_string()));
        }
        if let Some(l) = limit.filter(|l| *l > 0) {
            params.push(("limit", l.to_string()));
        }
        let req = if params.is_empty() {
            self.client.get(&url)
        } else {
            self.client.get(&url).query(&params)
        };
        let resp = req.send().await?;
        Self::json_ok(resp, "list sessions").await
    }

    pub async fn get_session_status(
        &self,
    ) -> anyhow::Result<std::collections::HashMap<String, SessionStatusInfo>> {
        let url = server_url(&self.base_url, "/session/status");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get session status").await
    }

    pub async fn update_session_title(
        &self,
        session_id: &str,
        title: &str,
    ) -> anyhow::Result<SessionInfo> {
        let url = server_url(&self.base_url, &format!("/session/{}", session_id));
        let req = UpdateSessionRequest {
            title: Some(title.to_string()),
        };
        let resp = self.client.patch(&url).json(&req).send().await?;
        Self::json_ok(resp, "update session title").await
    }

    pub async fn delete_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let url = server_url(&self.base_url, &format!("/session/{}", session_id));
        let resp = self.client.delete(&url).send().await?;
        let value: serde_json::Value = Self::json_ok(resp, "delete session").await?;
        Ok(value
            .get("deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    // ── Prompt ───────────────────────────────────────────────────────

    pub async fn send_prompt(
        &self,
        session_id: &str,
        content: String,
        agent: Option<String>,
        scheduler_profile: Option<String>,
        model: Option<String>,
        variant: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = server_url(&self.base_url, &format!("/session/{}/prompt", session_id));
        let req = PromptRequest {
            message: content,
            agent,
            scheduler_profile,
            model,
            variant,
            command: None,
            arguments: None,
        };
        let resp = self.client.post(&url).json(&req).send().await?;
        Self::json_ok(resp, "send prompt").await
    }

    pub async fn abort_session(&self, session_id: &str) -> anyhow::Result<serde_json::Value> {
        let url = server_url(&self.base_url, &format!("/session/{}/abort", session_id));
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, "abort session").await
    }

    pub async fn execute_shell(
        &self,
        session_id: &str,
        command: String,
        workdir: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = server_url(&self.base_url, &format!("/session/{}/shell", session_id));
        let req = ExecuteShellRequest { command, workdir };
        let resp = self.client.post(&url).json(&req).send().await?;
        Self::json_ok(resp, "execute shell").await
    }

    // ── Messages ─────────────────────────────────────────────────────

    pub async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<MessageInfo>> {
        self.get_messages_after(session_id, None, None).await
    }

    pub async fn get_messages_after(
        &self,
        session_id: &str,
        after: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<MessageInfo>> {
        let url = server_url(&self.base_url, &format!("/session/{}/message", session_id));
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(a) = after.map(str::trim).filter(|v| !v.is_empty()) {
            params.push(("after", a.to_string()));
        }
        if let Some(l) = limit.filter(|v| *v > 0) {
            params.push(("limit", l.to_string()));
        }
        let req = if params.is_empty() {
            self.client.get(&url)
        } else {
            self.client.get(&url).query(&params)
        };
        let resp = req.send().await?;
        Self::json_ok(resp, "get messages").await
    }

    // ── Question ─────────────────────────────────────────────────────

    pub async fn list_questions(&self) -> anyhow::Result<Vec<QuestionInfo>> {
        let url = server_url(&self.base_url, "/question");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "list questions").await
    }

    pub async fn reply_question(
        &self,
        question_id: &str,
        answers: Vec<Vec<String>>,
    ) -> anyhow::Result<()> {
        let url = server_url(&self.base_url, &format!("/question/{}/reply", question_id));
        let body = serde_json::json!({ "answers": answers });
        let resp = self.client.post(&url).json(&body).send().await?;
        Self::expect_success(resp, &format!("reply question `{}`", question_id)).await?;
        Ok(())
    }

    pub async fn reject_question(&self, question_id: &str) -> anyhow::Result<()> {
        let url = server_url(&self.base_url, &format!("/question/{}/reject", question_id));
        let resp = self.client.post(&url).send().await?;
        Self::expect_success(resp, &format!("reject question `{}`", question_id)).await?;
        Ok(())
    }

    pub async fn list_permissions(&self) -> anyhow::Result<Vec<PermissionRequestInfo>> {
        let url = server_url(&self.base_url, "/permission");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "list permissions").await
    }

    pub async fn reply_permission(
        &self,
        permission_id: &str,
        reply: &str,
        message: Option<String>,
    ) -> anyhow::Result<()> {
        let url = server_url(
            &self.base_url,
            &format!("/permission/{}/reply", permission_id),
        );
        let body = serde_json::json!({
            "reply": reply,
            "message": message,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        Self::expect_success(resp, &format!("reply permission `{}`", permission_id)).await?;
        Ok(())
    }

    // ── Execution topology & recovery ────────────────────────────────

    pub async fn get_session_executions(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionExecutionTopology> {
        let url = server_url(
            &self.base_url,
            &format!("/session/{}/executions", session_id),
        );
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get session executions").await
    }

    /// Fetch the aggregated runtime state for a session.
    pub async fn get_session_runtime(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionRuntimeState> {
        let url = server_url(&self.base_url, &format!("/session/{}/runtime", session_id));
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get session runtime").await
    }

    pub async fn get_session_recovery(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionRecoveryProtocol> {
        let url = server_url(&self.base_url, &format!("/session/{}/recovery", session_id));
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get session recovery").await
    }

    pub async fn execute_session_recovery(
        &self,
        session_id: &str,
        action: RecoveryActionKind,
        target_id: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = server_url(
            &self.base_url,
            &format!("/session/{}/recovery/execute", session_id),
        );
        let req = ExecuteRecoveryRequest { action, target_id };
        let resp = self.client.post(&url).json(&req).send().await?;
        Self::json_ok(resp, "execute session recovery").await
    }

    pub async fn cancel_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = server_url(
            &self.base_url,
            &format!("/session/{}/tool/{}/cancel", session_id, tool_call_id),
        );
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, "cancel tool call").await
    }

    // ── Providers ────────────────────────────────────────────────────

    pub async fn get_config(&self) -> anyhow::Result<Config> {
        let url = server_url(&self.base_url, "/config");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get config").await
    }

    pub async fn get_config_providers(&self) -> anyhow::Result<ProviderListResponse> {
        let url = server_url(&self.base_url, "/config/providers");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get config providers").await
    }

    pub async fn get_all_providers(&self) -> anyhow::Result<FullProviderListResponse> {
        let url = server_url(&self.base_url, "/provider");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get all providers").await
    }

    pub async fn get_known_providers(&self) -> anyhow::Result<KnownProvidersResponse> {
        let url = server_url(&self.base_url, "/provider/known");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "get known providers").await
    }

    pub async fn set_auth(&self, provider_id: &str, api_key: &str) -> anyhow::Result<()> {
        let url = server_url(&self.base_url, &format!("/auth/{}", provider_id));
        let body = serde_json::json!({ "key": api_key });
        let resp = self.client.put(&url).json(&body).send().await?;
        Self::expect_success(resp, &format!("set auth for `{}`", provider_id)).await?;
        Ok(())
    }

    // ── Agents & modes ───────────────────────────────────────────────

    pub async fn list_agents(&self) -> anyhow::Result<Vec<AgentInfo>> {
        let url = server_url(&self.base_url, "/agent");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "list agents").await
    }

    pub async fn list_execution_modes(&self) -> anyhow::Result<Vec<ExecutionModeInfo>> {
        let url = server_url(&self.base_url, "/mode");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "list execution modes").await
    }

    pub async fn list_skills(&self) -> anyhow::Result<Vec<String>> {
        let url = server_url(&self.base_url, "/skill");
        let resp = self.client.get(&url).send().await?;
        Self::json_ok(resp, "list skills").await
    }

    // ── MCP ──────────────────────────────────────────────────────────

    pub async fn get_mcp_status(&self) -> anyhow::Result<Vec<McpStatusInfo>> {
        let url = server_url(&self.base_url, "/mcp");
        let resp = self.client.get(&url).send().await?;
        let map: std::collections::HashMap<String, McpStatusInfo> =
            Self::json_ok(resp, "get MCP status").await?;
        let mut servers: Vec<McpStatusInfo> = map.into_values().collect();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(servers)
    }

    pub async fn start_mcp_auth(&self, name: &str) -> anyhow::Result<McpAuthStartInfo> {
        let url = server_url(&self.base_url, &format!("/mcp/{}/auth", name));
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, &format!("start MCP auth `{}`", name)).await
    }

    pub async fn authenticate_mcp(&self, name: &str) -> anyhow::Result<McpStatusInfo> {
        let url = server_url(&self.base_url, &format!("/mcp/{}/auth/authenticate", name));
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, &format!("authenticate MCP `{}`", name)).await
    }

    pub async fn remove_mcp_auth(&self, name: &str) -> anyhow::Result<bool> {
        let url = server_url(&self.base_url, &format!("/mcp/{}/auth", name));
        let resp = self.client.delete(&url).send().await?;
        let value: serde_json::Value =
            Self::json_ok(resp, &format!("remove MCP auth `{}`", name)).await?;
        Ok(value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub async fn connect_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let url = server_url(&self.base_url, &format!("/mcp/{}/connect", name));
        let resp = self.client.post(&url).send().await?;
        let bytes = Self::expect_success(resp, &format!("connect MCP `{}`", name)).await?;
        Ok(serde_json::from_slice::<bool>(&bytes).unwrap_or(true))
    }

    pub async fn disconnect_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let url = server_url(&self.base_url, &format!("/mcp/{}/disconnect", name));
        let resp = self.client.post(&url).send().await?;
        let bytes = Self::expect_success(resp, &format!("disconnect MCP `{}`", name)).await?;
        Ok(serde_json::from_slice::<bool>(&bytes).unwrap_or(true))
    }

    // ── LSP & formatters ─────────────────────────────────────────────

    pub async fn get_lsp_servers(&self) -> anyhow::Result<Vec<String>> {
        let url = server_url(&self.base_url, "/lsp");
        let resp = self.client.get(&url).send().await?;
        let v: serde_json::Value = Self::json_ok(resp, "get LSP status").await?;
        Ok(v.get("servers")
            .and_then(|s| serde_json::from_value::<Vec<String>>(s.clone()).ok())
            .unwrap_or_default())
    }

    pub async fn get_formatters(&self) -> anyhow::Result<Vec<String>> {
        let url = server_url(&self.base_url, "/formatter");
        let resp = self.client.get(&url).send().await?;
        let v: serde_json::Value = Self::json_ok(resp, "get formatters").await?;
        Ok(v.get("formatters")
            .and_then(|s| serde_json::from_value::<Vec<String>>(s.clone()).ok())
            .unwrap_or_default())
    }

    // ── Session sharing / compact / revert / fork ────────────────────

    pub async fn share_session(&self, session_id: &str) -> anyhow::Result<ShareResponse> {
        let url = server_url(&self.base_url, &format!("/session/{}/share", session_id));
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, &format!("share session `{}`", session_id)).await
    }

    pub async fn unshare_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let url = server_url(&self.base_url, &format!("/session/{}/share", session_id));
        let resp = self.client.delete(&url).send().await?;
        let value: serde_json::Value =
            Self::json_ok(resp, &format!("unshare session `{}`", session_id)).await?;
        Ok(value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub async fn compact_session(&self, session_id: &str) -> anyhow::Result<CompactResponse> {
        let url = server_url(&self.base_url, &format!("/session/{}/compact", session_id));
        let resp = self.client.post(&url).send().await?;
        Self::json_ok(resp, &format!("compact session `{}`", session_id)).await
    }

    pub async fn revert_session(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> anyhow::Result<RevertResponse> {
        let url = server_url(&self.base_url, &format!("/session/{}/revert", session_id));
        let req = RevertRequest {
            message_id: message_id.to_string(),
        };
        let resp = self.client.post(&url).json(&req).send().await?;
        Self::json_ok(resp, &format!("revert session `{}`", session_id)).await
    }

    pub async fn fork_session(
        &self,
        session_id: &str,
        message_id: Option<&str>,
    ) -> anyhow::Result<SessionInfo> {
        let url = server_url(&self.base_url, &format!("/session/{}/fork", session_id));
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(msg_id) = message_id {
            params.push(("message_id", msg_id.to_string()));
        }
        let req = if params.is_empty() {
            self.client.post(&url)
        } else {
            self.client.post(&url).query(&params)
        };
        let resp = req.send().await?;
        Self::json_ok(resp, &format!("fork session `{}`", session_id)).await
    }

    // ── internal ─────────────────────────────────────────────────────

    /// Consume the response, returning the body bytes on success or an
    /// error with the response text on failure.
    async fn expect_success(resp: reqwest::Response, action: &str) -> anyhow::Result<Vec<u8>> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp.bytes().await?.to_vec())
        } else {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to {}: {} - {}", action, status, text);
        }
    }

    /// Convenience: check status, consume body, deserialize JSON.
    async fn json_ok<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
        action: &str,
    ) -> anyhow::Result<T> {
        let bytes = Self::expect_success(resp, action).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
