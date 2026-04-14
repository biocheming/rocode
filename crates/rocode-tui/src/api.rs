use reqwest::blocking::Client;
use rocode_command::stage_protocol::{StageEvent, StageSummary};
use rocode_config::Config as AppConfig;
pub use rocode_multimodal::{
    ModalityKind, ModalityPreflightResult, MultimodalCapabilitiesResponse,
    MultimodalPolicyResponse, MultimodalPreflightRequest, MultimodalPreflightResponse,
    PreflightCapabilityView, PreflightInputPart,
};
use rocode_runtime_context::ResolvedWorkspaceContext;
pub use rocode_session::{
    PermissionRulesetInfo, SessionInfo, SessionListContract, SessionListHints, SessionListItem,
    SessionListResponse, SessionListTime, SessionRevertInfo, SessionShareInfo, SessionSummaryInfo,
    SessionTimeInfo,
};
use rocode_session::SessionUsage;
use rocode_state::RecentModelEntry;
pub use rocode_types::{
    ManagedSkillRecord, MemoryConflictResponse, MemoryConsolidationRequest,
    MemoryConsolidationResponse, MemoryConsolidationRunListResponse, MemoryConsolidationRunQuery,
    MemoryDetailView, MemoryListQuery, MemoryListResponse, MemoryRetrievalPreviewResponse,
    MemoryRetrievalQuery, MemoryRuleHitListResponse, MemoryRuleHitQuery,
    MemoryRulePackListResponse, MemoryValidationReportResponse, SessionInsightsResponse,
    SessionMemoryTelemetrySummary, SessionStatusInfo,
    SkillArtifactCacheEntry, SkillAuditEvent, SkillDistributionRecord,
    SkillGovernanceTimelineEntry, SkillGovernanceTimelineStatus, SkillGovernanceWriteResult,
    SkillGuardReport, SkillGuardStatus, SkillHubArtifactCacheResponse, SkillHubAuditResponse,
    SkillHubDistributionResponse, SkillHubGuardRunRequest, SkillHubGuardRunResponse,
    SkillHubIndexRefreshRequest, SkillHubIndexRefreshResponse, SkillHubIndexResponse,
    SkillHubLifecycleResponse, SkillHubManagedDetachRequest, SkillHubManagedDetachResponse,
    SkillHubManagedRemoveRequest, SkillHubManagedRemoveResponse, SkillHubManagedResponse,
    SkillHubPolicy, SkillHubPolicyResponse, SkillHubRemoteInstallApplyRequest,
    SkillHubRemoteInstallPlanRequest, SkillHubRemoteUpdateApplyRequest,
    SkillHubRemoteUpdatePlanRequest, SkillHubSyncApplyRequest, SkillHubSyncPlanRequest,
    SkillHubSyncPlanResponse, SkillHubTimelineQuery, SkillHubTimelineResponse,
    SkillManagedLifecycleRecord, SkillRemoteInstallAction, SkillRemoteInstallEntry,
    SkillRemoteInstallPlan, SkillRemoteInstallResponse, SkillSourceIndexSnapshot,
    SkillSourceKind, SkillSourceRef, SkillSyncPlan,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type PromptPart = rocode_session::prompt::PartInput;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillCatalogEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    pub location: String,
    #[serde(default)]
    pub writable: bool,
    #[serde(default)]
    pub supporting_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillFileRef {
    pub relative_path: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDetailMeta {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    pub location: String,
    #[serde(default)]
    pub supporting_files: Vec<SkillFileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDetailSkill {
    pub meta: SkillDetailMeta,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDetailResponse {
    pub skill: SkillDetailSkill,
    pub source: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillManageAction {
    Create,
    Patch,
    Edit,
    WriteFile,
    RemoveFile,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManageRequest {
    pub session_id: String,
    pub action: SkillManageAction,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub methodology: Option<rocode_skill::SkillMethodologyTemplate>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub directory_name: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillManageResult {
    pub action: String,
    pub skill_name: String,
    pub location: String,
    #[serde(default)]
    pub supporting_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillManageResponse {
    #[serde(flatten)]
    pub result: SkillManageResult,
    #[serde(default)]
    pub guard_report: Option<SkillGuardReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillCatalogQuery {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub tool_policy: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillDetailQuery {
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub tool_policy: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RecentModelsPayload {
    #[serde(default)]
    recent_models: Vec<RecentModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptResponse {
    pub status: String,
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pending_question_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingCommandInvocation {
    pub command: String,
    #[serde(rename = "rawArguments", default)]
    pub raw_arguments: String,
    #[serde(rename = "missingFields", default)]
    pub missing_fields: Vec<String>,
    #[serde(rename = "schedulerProfile", default)]
    pub scheduler_profile: Option<String>,
    #[serde(rename = "questionId", default)]
    pub question_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    PromptRun,
    SchedulerRun,
    SchedulerStage,
    ToolCall,
    AgentTask,
    Question,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Waiting,
    Cancelling,
    Retry,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExecutionNode {
    pub id: String,
    pub kind: ExecutionKind,
    pub status: ExecutionStatus,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub waiting_on: Option<String>,
    #[serde(default)]
    pub recent_event: Option<String>,
    pub started_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub children: Vec<SessionExecutionNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExecutionTopology {
    #[serde(alias = "sessionID", alias = "sessionId")]
    pub session_id: String,
    pub active_count: usize,
    #[serde(default)]
    pub done_count: usize,
    pub running_count: usize,
    pub waiting_count: usize,
    pub cancelling_count: usize,
    pub retry_count: usize,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub roots: Vec<SessionExecutionNode>,
}

// ── Session Runtime State (from GET /session/{id}/runtime) ──────────────

/// Aggregated runtime snapshot for a single session.
///
/// This is the client-side mirror of `rocode_server::session_runtime::state::SessionRuntimeState`.
/// Deserialized from the `GET /session/{id}/runtime` endpoint response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntimeState {
    pub session_id: String,
    pub run_status: SessionRunStatusKind,
    #[serde(default)]
    pub current_message_id: Option<String>,
    #[serde(default)]
    pub usage: Option<SessionUsage>,
    #[serde(default)]
    pub active_stage_id: Option<String>,
    #[serde(default)]
    pub active_stage_count: u32,
    #[serde(default)]
    pub active_tools: Vec<ActiveToolSummary>,
    #[serde(default)]
    pub pending_question: Option<PendingQuestionSummary>,
    #[serde(default)]
    pub pending_permission: Option<PendingPermissionSummary>,
    #[serde(default)]
    pub child_sessions: Vec<ChildSessionSummary>,
}

/// Coarse run-status for the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRunStatusKind {
    Idle,
    Running,
    WaitingOnTool,
    WaitingOnUser,
    Cancelling,
}

/// Summary of a currently executing tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveToolSummary {
    pub tool_call_id: String,
    pub tool_name: String,
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

/// Aggregated runtime/activity snapshot for a single session.
///
/// This is the client-side mirror of `GET /session/{id}/telemetry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTelemetrySnapshot {
    pub runtime: SessionRuntimeState,
    #[serde(default)]
    pub stages: Vec<StageSummary>,
    pub topology: SessionExecutionTopology,
    pub usage: SessionUsage,
    #[serde(default)]
    pub memory: Option<SessionMemoryTelemetrySummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionEventsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryProtocolStatus {
    Running,
    AwaitingUser,
    Recoverable,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionKind {
    AbortRun,
    AbortStage,
    Retry,
    Resume,
    PartialReplay,
    RestartStage,
    RestartSubtask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCheckpointInfo {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub scheduler_profile: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub stage_index: Option<u32>,
    #[serde(default)]
    pub stage_total: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryActionInfo {
    pub kind: RecoveryActionKind,
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecoveryProtocol {
    #[serde(alias = "sessionID", alias = "sessionId")]
    pub session_id: String,
    pub status: RecoveryProtocolStatus,
    pub active_execution_count: usize,
    pub pending_question_count: usize,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub last_user_prompt: Option<String>,
    #[serde(default)]
    pub actions: Vec<RecoveryActionInfo>,
    #[serde(default)]
    pub checkpoints: Vec<RecoveryCheckpointInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRecoveryRequest {
    pub action: RecoveryActionKind,
    #[serde(default)]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOptionInfo {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItemInfo {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<QuestionOptionInfo>,
    #[serde(default)]
    pub multiple: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionInfo {
    pub id: String,
    #[serde(alias = "sessionID", alias = "sessionId")]
    pub session_id: String,
    pub questions: Vec<String>,
    #[serde(default)]
    pub options: Option<Vec<Vec<String>>>,
    /// Full-fidelity question items with descriptions, headers, multi-select.
    #[serde(default)]
    pub items: Vec<QuestionItemInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestInfo {
    pub id: String,
    #[serde(alias = "sessionID", alias = "sessionId")]
    pub session_id: String,
    pub tool: String,
    pub input: serde_json::Value,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    pub id: String,
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
    pub file: Option<FileInfo>,
    #[serde(alias = "toolCall")]
    pub tool_call: Option<ToolCall>,
    #[serde(alias = "toolResult")]
    pub tool_result: Option<ToolResult>,
    #[serde(default)]
    pub synthetic: Option<bool>,
    #[serde(default)]
    pub ignored: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub url: String,
    pub filename: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
    #[serde(default)]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(alias = "toolCallId")]
    pub tool_call_id: String,
    pub content: String,
    #[serde(alias = "isError")]
    pub is_error: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub attachments: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    #[serde(alias = "sessionId")]
    pub session_id: String,
    pub role: String,
    pub created_at: i64,
    #[serde(default, alias = "completedAt")]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub tokens: MessageTokensInfo,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub multimodal: Option<rocode_multimodal::PersistedMultimodalExplain>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageTokensInfo {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(default, alias = "cacheRead")]
    pub cache_read: u64,
    #[serde(default, alias = "cacheWrite")]
    pub cache_write: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<PromptPart>>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub command: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteShellRequest {
    pub command: String,
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub parent_id: Option<String>,
    pub scheduler_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderListResponse {
    pub providers: Vec<ProviderInfo>,
    #[serde(rename = "default")]
    pub default_model: HashMap<String, String>,
}

/// Response from `GET /provider/` — includes the full provider catalogue
/// together with a list of provider IDs that are currently connected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullProviderListResponse {
    pub all: Vec<ProviderInfo>,
    #[serde(rename = "default")]
    pub default_model: HashMap<String, String>,
    #[serde(default)]
    pub connected: Vec<String>,
}

/// A single entry from the `GET /provider/known` catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownProviderEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub model_count: usize,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub supports_api_key_connect: bool,
}

/// Response from `GET /provider/known`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownProvidersResponse {
    pub providers: Vec<KnownProviderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectProtocolOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnectSchemaResponse {
    pub providers: Vec<KnownProviderEntry>,
    #[serde(default)]
    pub protocols: Vec<ConnectProtocolOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectDraftMode {
    Known,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnectDraft {
    pub mode: ProviderConnectDraftMode,
    pub provider_id: String,
    #[serde(default)]
    pub known_provider_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub model_count: usize,
    #[serde(default)]
    pub supports_api_key_connect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveProviderConnectRequest {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveProviderConnectResponse {
    pub query: String,
    pub suggested_mode: ProviderConnectDraftMode,
    pub exact_match: bool,
    #[serde(default)]
    pub matches: Vec<KnownProviderEntry>,
    pub draft: ProviderConnectDraft,
    pub custom_draft: ProviderConnectDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogRefreshStatus {
    Updated,
    NotModified,
    FallbackCached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshProviderCatalogResponse {
    pub generation_before: u64,
    pub generation_after: u64,
    pub changed: bool,
    pub status: ProviderCatalogRefreshStatus,
    #[serde(default)]
    pub error_message: Option<String>,
}

impl RefreshProviderCatalogResponse {
    pub fn status_message(&self) -> String {
        match self.status {
            ProviderCatalogRefreshStatus::Updated => format!(
                "Model catalogue refreshed (generation {} -> {}).",
                self.generation_before, self.generation_after
            ),
            ProviderCatalogRefreshStatus::NotModified => format!(
                "Model catalogue checked; no changes (generation {}).",
                self.generation_after
            ),
            ProviderCatalogRefreshStatus::FallbackCached => format!(
                "Model catalogue refresh failed; using cached snapshot: {}",
                self.error_message
                    .as_deref()
                    .unwrap_or("Unknown refresh failure")
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectProviderRequest {
    pub provider_id: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ProviderModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(
        default,
        alias = "context_window",
        alias = "contextWindow",
        alias = "contextLength"
    )]
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionModeInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub orchestrator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStatusInfo {
    pub name: String,
    pub status: String,
    pub tools: usize,
    pub resources: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuthStartInfo {
    pub authorization_url: String,
    pub client_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LspStatusResponse {
    servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FormatterStatusResponse {
    formatters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResponse {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertRequest {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevertResponse {
    pub success: bool,
}

/// Server-side todo item returned by `/session/{id}/todo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Server-side diff entry returned by `/session/{id}/diff`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDiffEntry {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

pub struct ApiClient {
    client: Client,
    base_url: String,
    pub current_session: Arc<RwLock<Option<SessionInfo>>>,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            current_session: Arc::new(RwLock::new(None)),
        }
    }

    pub fn create_session(
        &self,
        parent_id: Option<String>,
        scheduler_profile: Option<String>,
    ) -> anyhow::Result<SessionInfo> {
        let url = format!("{}/session", self.base_url);
        let request = CreateSessionRequest {
            parent_id,
            scheduler_profile,
        };

        let response = self.client.post(&url).json(&request).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to create session: {} - {}", status, text);
        }

        let session: SessionInfo = response.json()?;
        Ok(session)
    }

    pub fn get_session(&self, session_id: &str) -> anyhow::Result<SessionInfo> {
        let url = format!("{}/session/{}", self.base_url, session_id);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session: {} - {}", status, text);
        }

        let session: SessionInfo = response.json()?;
        Ok(session)
    }

    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionListItem>> {
        self.list_sessions_filtered(None, None)
    }

    pub fn list_sessions_filtered(
        &self,
        search: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<SessionListItem>> {
        let url = format!("{}/session", self.base_url);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(search) = search.map(str::trim).filter(|s| !s.is_empty()) {
            params.push(("search", search.to_string()));
        }
        if let Some(limit) = limit.filter(|l| *l > 0) {
            params.push(("limit", limit.to_string()));
        }

        let request = if params.is_empty() {
            self.client.get(&url)
        } else {
            self.client.get(&url).query(&params)
        };
        let response = request.send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list sessions: {} - {}", status, text);
        }

        let sessions: SessionListResponse = response.json()?;
        Ok(sessions.items)
    }

    pub fn get_session_status(&self) -> anyhow::Result<HashMap<String, SessionStatusInfo>> {
        let url = format!("{}/session/status", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session status: {} - {}", status, text);
        }
        Ok(response.json::<HashMap<String, SessionStatusInfo>>()?)
    }

    pub fn get_session_executions(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionExecutionTopology> {
        let url = format!("{}/session/{}/executions", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session executions: {} - {}", status, text);
        }
        Ok(response.json::<SessionExecutionTopology>()?)
    }

    pub fn get_session_runtime(&self, session_id: &str) -> anyhow::Result<SessionRuntimeState> {
        let url = format!("{}/session/{}/runtime", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session runtime: {} - {}", status, text);
        }
        Ok(response.json::<SessionRuntimeState>()?)
    }

    pub fn get_session_telemetry(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionTelemetrySnapshot> {
        let url = format!("{}/session/{}/telemetry", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session telemetry: {} - {}", status, text);
        }
        Ok(response.json::<SessionTelemetrySnapshot>()?)
    }

    pub fn get_session_insights(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionInsightsResponse> {
        let url = format!("{}/session/{}/insights", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session insights: {} - {}", status, text);
        }
        Ok(response.json::<SessionInsightsResponse>()?)
    }

    pub fn get_session_events(
        &self,
        session_id: &str,
        query: &SessionEventsQuery,
    ) -> anyhow::Result<Vec<StageEvent>> {
        let url = format!("{}/session/{}/events", self.base_url, session_id);
        let response = self.client.get(&url).query(query).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session events: {} - {}", status, text);
        }
        Ok(response.json::<Vec<StageEvent>>()?)
    }

    pub fn get_session_todos(&self, session_id: &str) -> anyhow::Result<Vec<ApiTodoItem>> {
        let url = format!("{}/session/{}/todo", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            return Ok(Vec::new());
        }
        Ok(response.json::<Vec<ApiTodoItem>>()?)
    }

    pub fn get_session_diff(&self, session_id: &str) -> anyhow::Result<Vec<ApiDiffEntry>> {
        let url = format!("{}/session/{}/diff", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            return Ok(Vec::new());
        }
        Ok(response.json::<Vec<ApiDiffEntry>>()?)
    }

    pub fn get_session_recovery(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionRecoveryProtocol> {
        let url = format!("{}/session/{}/recovery", self.base_url, session_id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get session recovery: {} - {}", status, text);
        }
        Ok(response.json::<SessionRecoveryProtocol>()?)
    }

    pub fn execute_session_recovery(
        &self,
        session_id: &str,
        action: RecoveryActionKind,
        target_id: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/session/{}/recovery/execute", self.base_url, session_id);
        let request = ExecuteRecoveryRequest { action, target_id };
        let response = self.client.post(&url).json(&request).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to execute session recovery: {} - {}", status, text);
        }
        Ok(response.json::<serde_json::Value>()?)
    }

    pub fn list_questions(&self) -> anyhow::Result<Vec<QuestionInfo>> {
        let url = format!("{}/question", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list questions: {} - {}", status, text);
        }
        Ok(response.json::<Vec<QuestionInfo>>()?)
    }

    pub fn reply_question(
        &self,
        question_id: &str,
        answers: Vec<Vec<String>>,
    ) -> anyhow::Result<()> {
        let url = format!("{}/question/{}/reply", self.base_url, question_id);
        let body = serde_json::json!({ "answers": answers });
        let response = self.client.post(&url).json(&body).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to reply question `{}`: {} - {}",
                question_id,
                status,
                text
            );
        }
        Ok(())
    }

    pub fn reject_question(&self, question_id: &str) -> anyhow::Result<()> {
        let url = format!("{}/question/{}/reject", self.base_url, question_id);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to reject question `{}`: {} - {}",
                question_id,
                status,
                text
            );
        }
        Ok(())
    }

    pub fn list_permissions(&self) -> anyhow::Result<Vec<PermissionRequestInfo>> {
        let url = format!("{}/permission", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list permissions: {} - {}", status, text);
        }
        Ok(response.json::<Vec<PermissionRequestInfo>>()?)
    }

    pub fn reply_permission(
        &self,
        permission_id: &str,
        reply: &str,
        message: Option<String>,
    ) -> anyhow::Result<()> {
        let url = format!("{}/permission/{}/reply", self.base_url, permission_id);
        let body = serde_json::json!({
            "reply": reply,
            "message": message,
        });
        let response = self.client.post(&url).json(&body).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to reply permission `{}`: {} - {}",
                permission_id,
                status,
                text
            );
        }
        Ok(())
    }

    pub fn update_session_title(
        &self,
        session_id: &str,
        title: &str,
    ) -> anyhow::Result<SessionInfo> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let request = UpdateSessionRequest {
            title: Some(title.to_string()),
        };
        let response = self.client.patch(&url).json(&request).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to update session `{}` title: {} - {}",
                session_id,
                status,
                text
            );
        }
        let session: SessionInfo = response.json()?;
        Ok(session)
    }

    pub fn delete_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let response = self.client.delete(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to delete session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        let value = response.json::<serde_json::Value>()?;
        Ok(value
            .get("deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub fn send_prompt(
        &self,
        session_id: &str,
        content: String,
        parts: Option<Vec<PromptPart>>,
        agent: Option<String>,
        scheduler_profile: Option<String>,
        model: Option<String>,
        variant: Option<String>,
    ) -> anyhow::Result<PromptResponse> {
        let url = format!("{}/session/{}/prompt", self.base_url, session_id);
        let request = PromptRequest {
            message: (!content.trim().is_empty()).then_some(content),
            parts,
            agent,
            scheduler_profile,
            model,
            variant,
            command: None,
            arguments: None,
        };

        let response = self.client.post(&url).json(&request).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to send prompt to {}: {} - {}", url, status, text);
        }

        Ok(response.json::<PromptResponse>()?)
    }

    pub fn send_command_prompt(
        &self,
        session_id: &str,
        command: String,
        arguments: Option<String>,
        model: Option<String>,
        variant: Option<String>,
    ) -> anyhow::Result<PromptResponse> {
        let url = format!("{}/session/{}/prompt", self.base_url, session_id);
        let request = PromptRequest {
            message: None,
            parts: None,
            agent: None,
            scheduler_profile: None,
            model,
            variant,
            command: Some(command),
            arguments,
        };
        let response = self.client.post(&url).json(&request).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to send command prompt to {}: {} - {}",
                url,
                status,
                text
            );
        }

        Ok(response.json::<PromptResponse>()?)
    }

    pub fn execute_shell(
        &self,
        session_id: &str,
        command: String,
        workdir: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/session/{}/shell", self.base_url, session_id);
        let request = ExecuteShellRequest { command, workdir };
        let response = self.client.post(&url).json(&request).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to execute shell command: {} - {}", status, text);
        }

        Ok(response.json::<serde_json::Value>()?)
    }

    pub fn abort_session(&self, session_id: &str) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/session/{}/abort", self.base_url, session_id);
        let response = self.client.post(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to abort session: {} - {}", status, text);
        }

        Ok(response.json::<serde_json::Value>()?)
    }

    pub fn cancel_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!(
            "{}/session/{}/tool/{}/cancel",
            self.base_url, session_id, tool_call_id
        );
        let response = self.client.post(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to cancel tool call: {} - {}", status, text);
        }

        Ok(response.json::<serde_json::Value>()?)
    }

    pub fn get_config_providers(&self) -> anyhow::Result<ProviderListResponse> {
        let url = format!("{}/config/providers", self.base_url);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get providers: {} - {}", status, text);
        }

        let providers: ProviderListResponse = response.json()?;
        Ok(providers)
    }

    pub fn get_config(&self) -> anyhow::Result<AppConfig> {
        let url = format!("{}/config", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get config: {} - {}", status, text);
        }

        Ok(response.json()?)
    }

    pub fn get_workspace_context(&self) -> anyhow::Result<ResolvedWorkspaceContext> {
        let url = format!("{}/workspace/context", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get workspace context: {} - {}", status, text);
        }

        Ok(response.json()?)
    }

    pub fn get_multimodal_policy(&self) -> anyhow::Result<MultimodalPolicyResponse> {
        let url = format!("{}/multimodal/policy", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get multimodal policy: {} - {}", status, text);
        }

        Ok(response.json()?)
    }

    pub fn get_multimodal_capabilities(
        &self,
        model: Option<&str>,
    ) -> anyhow::Result<MultimodalCapabilitiesResponse> {
        let url = format!("{}/multimodal/capabilities", self.base_url);
        let request = if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
            self.client.get(&url).query(&[("model", model)])
        } else {
            self.client.get(&url)
        };
        let response = request.send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to get multimodal capabilities: {} - {}",
                status,
                text
            );
        }

        Ok(response.json()?)
    }

    pub fn preflight_multimodal(
        &self,
        request: &MultimodalPreflightRequest,
    ) -> anyhow::Result<MultimodalPreflightResponse> {
        let url = format!("{}/multimodal/preflight", self.base_url);
        let response = self.client.post(&url).json(request).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to run multimodal preflight: {} - {}", status, text);
        }

        Ok(response.json()?)
    }

    pub fn get_recent_models(&self) -> anyhow::Result<Vec<RecentModelEntry>> {
        let url = format!("{}/workspace/recent-models", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get recent models: {} - {}", status, text);
        }

        let payload: RecentModelsPayload = response.json()?;
        Ok(payload.recent_models)
    }

    pub fn put_recent_models(
        &self,
        recent_models: &[RecentModelEntry],
    ) -> anyhow::Result<Vec<RecentModelEntry>> {
        let url = format!("{}/workspace/recent-models", self.base_url);
        let response = self
            .client
            .put(&url)
            .json(&RecentModelsPayload {
                recent_models: recent_models.to_vec(),
            })
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to save recent models: {} - {}", status, text);
        }

        let payload: RecentModelsPayload = response.json()?;
        Ok(payload.recent_models)
    }

    pub fn patch_config(&self, patch: &serde_json::Value) -> anyhow::Result<AppConfig> {
        let url = format!("{}/config", self.base_url);
        let response = self.client.patch(&url).json(patch).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to patch config: {} - {}", status, text);
        }

        Ok(response.json()?)
    }

    /// Fetch the full provider catalogue from `GET /provider/`.
    /// Returns all known providers plus which ones are connected.
    pub fn get_all_providers(&self) -> anyhow::Result<FullProviderListResponse> {
        let url = format!("{}/provider", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get all providers: {} - {}", status, text);
        }
        Ok(response.json()?)
    }

    /// Fetch all known providers from `models.dev` via `GET /provider/known`.
    /// Returns every provider in the catalogue with connected status.
    pub fn get_known_providers(&self) -> anyhow::Result<KnownProvidersResponse> {
        let url = format!("{}/provider/known", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get known providers: {} - {}", status, text);
        }
        Ok(response.json()?)
    }

    pub fn get_provider_connect_schema(&self) -> anyhow::Result<ProviderConnectSchemaResponse> {
        let url = format!("{}/provider/connect/schema", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to get provider connect schema: {} - {}",
                status,
                text
            );
        }
        Ok(response.json()?)
    }

    pub fn resolve_provider_connect(
        &self,
        query: &str,
    ) -> anyhow::Result<ResolveProviderConnectResponse> {
        let url = format!("{}/provider/connect/resolve", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&ResolveProviderConnectRequest {
                query: query.to_string(),
            })
            .send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to resolve provider connect query: {} - {}",
                status,
                text
            );
        }
        Ok(response.json()?)
    }

    pub fn refresh_provider_catalog(&self) -> anyhow::Result<RefreshProviderCatalogResponse> {
        let url = format!("{}/provider/refresh", self.base_url);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to refresh provider catalogue: {} - {}",
                status,
                text
            );
        }
        Ok(response.json()?)
    }

    /// Set an API key for a provider via `PUT /auth/{id}`.
    pub fn set_auth(&self, provider_id: &str, api_key: &str) -> anyhow::Result<()> {
        self.connect_provider(&ConnectProviderRequest {
            provider_id: provider_id.to_string(),
            api_key: api_key.to_string(),
            base_url: None,
            protocol: None,
        })
    }

    /// Register a custom provider via `POST /provider/register`.
    pub fn register_custom_provider(
        &self,
        provider_id: &str,
        base_url: &str,
        protocol: &str,
        api_key: &str,
    ) -> anyhow::Result<()> {
        self.connect_provider(&ConnectProviderRequest {
            provider_id: provider_id.to_string(),
            api_key: api_key.to_string(),
            base_url: Some(base_url.to_string()),
            protocol: Some(protocol.to_string()),
        })
    }

    pub fn connect_provider(&self, request: &ConnectProviderRequest) -> anyhow::Result<()> {
        let url = format!("{}/provider/connect", self.base_url);
        let response = self.client.post(&url).json(request).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to connect provider `{}`: {} - {}",
                request.provider_id,
                status,
                text
            );
        }
        Ok(())
    }

    pub fn list_agents(&self) -> anyhow::Result<Vec<AgentInfo>> {
        let url = format!("{}/agent", self.base_url);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list agents: {} - {}", status, text);
        }

        let agents: Vec<AgentInfo> = response.json()?;
        Ok(agents)
    }

    pub fn list_execution_modes(&self) -> anyhow::Result<Vec<ExecutionModeInfo>> {
        let url = format!("{}/mode", self.base_url);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list execution modes: {} - {}", status, text);
        }

        let modes: Vec<ExecutionModeInfo> = response.json()?;
        Ok(modes)
    }

    pub fn list_skills(
        &self,
        query: Option<&SkillCatalogQuery>,
    ) -> anyhow::Result<Vec<SkillCatalogEntry>> {
        let url = format!("{}/skill/catalog", self.base_url);
        let response = match query {
            Some(query) => self.client.get(&url).query(query).send()?,
            None => self.client.get(&url).send()?,
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list skills: {} - {}", status, text);
        }

        Ok(response.json::<Vec<SkillCatalogEntry>>()?)
    }

    pub fn get_skill_detail(
        &self,
        query: &SkillDetailQuery,
    ) -> anyhow::Result<SkillDetailResponse> {
        let url = format!("{}/skill/detail", self.base_url);
        let response = self.client.get(&url).query(query).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill detail `{}`: {} - {}",
                query.name,
                status,
                text
            );
        }

        Ok(response.json::<SkillDetailResponse>()?)
    }

    pub fn manage_skill(&self, req: &SkillManageRequest) -> anyhow::Result<SkillManageResponse> {
        let url = format!("{}/skill/manage", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to manage skill: {} - {}", status, text);
        }

        Ok(response.json::<SkillManageResponse>()?)
    }

    pub fn list_memory(
        &self,
        query: Option<&MemoryListQuery>,
    ) -> anyhow::Result<MemoryListResponse> {
        let url = format!("{}/memory/list", self.base_url);
        let response = match query {
            Some(query) => self.client.get(&url).query(query).send()?,
            None => self.client.get(&url).send()?,
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to list memory: {} - {}", status, text);
        }
        Ok(response.json::<MemoryListResponse>()?)
    }

    pub fn search_memory(
        &self,
        query: Option<&MemoryListQuery>,
    ) -> anyhow::Result<MemoryListResponse> {
        let url = format!("{}/memory/search", self.base_url);
        let response = match query {
            Some(query) => self.client.get(&url).query(query).send()?,
            None => self.client.get(&url).send()?,
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to search memory: {} - {}", status, text);
        }
        Ok(response.json::<MemoryListResponse>()?)
    }

    pub fn get_memory_retrieval_preview(
        &self,
        query: &MemoryRetrievalQuery,
    ) -> anyhow::Result<MemoryRetrievalPreviewResponse> {
        let url = format!("{}/memory/retrieval-preview", self.base_url);
        let response = self.client.get(&url).query(query).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch memory retrieval preview: {} - {}",
                status,
                text
            );
        }
        Ok(response.json::<MemoryRetrievalPreviewResponse>()?)
    }

    pub fn get_memory_detail(&self, id: &str) -> anyhow::Result<MemoryDetailView> {
        let url = format!("{}/memory/{}", self.base_url, id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch memory detail `{}`: {} - {}",
                id,
                status,
                text
            );
        }
        Ok(response.json::<MemoryDetailView>()?)
    }

    pub fn get_memory_validation_report(
        &self,
        id: &str,
    ) -> anyhow::Result<MemoryValidationReportResponse> {
        let url = format!("{}/memory/{}/validation-report", self.base_url, id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch memory validation report `{}`: {} - {}",
                id,
                status,
                text
            );
        }
        Ok(response.json::<MemoryValidationReportResponse>()?)
    }

    pub fn get_memory_conflicts(&self, id: &str) -> anyhow::Result<MemoryConflictResponse> {
        let url = format!("{}/memory/{}/conflicts", self.base_url, id);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch memory conflicts `{}`: {} - {}",
                id,
                status,
                text
            );
        }
        Ok(response.json::<MemoryConflictResponse>()?)
    }

    pub fn list_memory_rule_packs(&self) -> anyhow::Result<MemoryRulePackListResponse> {
        let url = format!("{}/memory/rule-packs", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch memory rule packs: {} - {}", status, text);
        }
        Ok(response.json::<MemoryRulePackListResponse>()?)
    }

    pub fn list_memory_rule_hits(
        &self,
        query: Option<&MemoryRuleHitQuery>,
    ) -> anyhow::Result<MemoryRuleHitListResponse> {
        let url = format!("{}/memory/rule-hits", self.base_url);
        let response = match query {
            Some(query) => self.client.get(&url).query(query).send()?,
            None => self.client.get(&url).send()?,
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch memory rule hits: {} - {}", status, text);
        }
        Ok(response.json::<MemoryRuleHitListResponse>()?)
    }

    pub fn list_memory_consolidation_runs(
        &self,
        query: Option<&MemoryConsolidationRunQuery>,
    ) -> anyhow::Result<MemoryConsolidationRunListResponse> {
        let url = format!("{}/memory/consolidation/runs", self.base_url);
        let response = match query {
            Some(query) => self.client.get(&url).query(query).send()?,
            None => self.client.get(&url).send()?,
        };
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch memory consolidation runs: {} - {}",
                status,
                text
            );
        }
        Ok(response.json::<MemoryConsolidationRunListResponse>()?)
    }

    pub fn run_memory_consolidation(
        &self,
        request: &MemoryConsolidationRequest,
    ) -> anyhow::Result<MemoryConsolidationResponse> {
        let url = format!("{}/memory/consolidate", self.base_url);
        let response = self.client.post(&url).json(request).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to run memory consolidation: {} - {}", status, text);
        }
        Ok(response.json::<MemoryConsolidationResponse>()?)
    }

    pub fn list_skill_hub_managed(&self) -> anyhow::Result<SkillHubManagedResponse> {
        let url = format!("{}/skill/hub/managed", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill hub managed state: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubManagedResponse>()?)
    }

    pub fn list_skill_hub_index(&self) -> anyhow::Result<SkillHubIndexResponse> {
        let url = format!("{}/skill/hub/index", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill hub source index: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubIndexResponse>()?)
    }

    pub fn list_skill_hub_distributions(&self) -> anyhow::Result<SkillHubDistributionResponse> {
        let url = format!("{}/skill/hub/distributions", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill hub distributions: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubDistributionResponse>()?)
    }

    pub fn list_skill_hub_artifact_cache(&self) -> anyhow::Result<SkillHubArtifactCacheResponse> {
        let url = format!("{}/skill/hub/artifact-cache", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill hub artifact cache: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubArtifactCacheResponse>()?)
    }

    pub fn list_skill_hub_policy(&self) -> anyhow::Result<SkillHubPolicyResponse> {
        let url = format!("{}/skill/hub/policy", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch skill hub policy: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubPolicyResponse>()?)
    }

    pub fn list_skill_hub_lifecycle(&self) -> anyhow::Result<SkillHubLifecycleResponse> {
        let url = format!("{}/skill/hub/lifecycle", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch skill hub lifecycle: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubLifecycleResponse>()?)
    }

    pub fn refresh_skill_hub_index(
        &self,
        req: &SkillHubIndexRefreshRequest,
    ) -> anyhow::Result<SkillHubIndexRefreshResponse> {
        let url = format!("{}/skill/hub/index/refresh", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to refresh skill hub index: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubIndexRefreshResponse>()?)
    }

    pub fn list_skill_hub_audit(&self) -> anyhow::Result<SkillHubAuditResponse> {
        let url = format!("{}/skill/hub/audit", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch skill hub audit: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubAuditResponse>()?)
    }

    pub fn list_skill_hub_timeline(
        &self,
        query: &SkillHubTimelineQuery,
    ) -> anyhow::Result<SkillHubTimelineResponse> {
        let url = format!("{}/skill/hub/timeline", self.base_url);
        let response = self.client.get(&url).query(query).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fetch skill hub governance timeline: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubTimelineResponse>()?)
    }

    pub fn run_skill_hub_guard(
        &self,
        req: &SkillHubGuardRunRequest,
    ) -> anyhow::Result<SkillHubGuardRunResponse> {
        let url = format!("{}/skill/hub/guard/run", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to run skill hub guard: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubGuardRunResponse>()?)
    }

    pub fn plan_skill_hub_sync(
        &self,
        req: &SkillHubSyncPlanRequest,
    ) -> anyhow::Result<SkillHubSyncPlanResponse> {
        let url = format!("{}/skill/hub/sync/plan", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to plan skill hub sync: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubSyncPlanResponse>()?)
    }

    pub fn apply_skill_hub_sync(
        &self,
        req: &SkillHubSyncApplyRequest,
    ) -> anyhow::Result<SkillHubSyncPlanResponse> {
        let url = format!("{}/skill/hub/sync/apply", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to apply skill hub sync: {} - {}", status, text);
        }

        Ok(response.json::<SkillHubSyncPlanResponse>()?)
    }

    pub fn plan_skill_hub_remote_install(
        &self,
        req: &SkillHubRemoteInstallPlanRequest,
    ) -> anyhow::Result<SkillRemoteInstallPlan> {
        let url = format!("{}/skill/hub/install/plan", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to plan skill hub remote install: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillRemoteInstallPlan>()?)
    }

    pub fn apply_skill_hub_remote_install(
        &self,
        req: &SkillHubRemoteInstallApplyRequest,
    ) -> anyhow::Result<SkillRemoteInstallResponse> {
        let url = format!("{}/skill/hub/install/apply", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to apply skill hub remote install: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillRemoteInstallResponse>()?)
    }

    pub fn plan_skill_hub_remote_update(
        &self,
        req: &SkillHubRemoteUpdatePlanRequest,
    ) -> anyhow::Result<SkillRemoteInstallPlan> {
        let url = format!("{}/skill/hub/update/plan", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to plan skill hub remote update: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillRemoteInstallPlan>()?)
    }

    pub fn apply_skill_hub_remote_update(
        &self,
        req: &SkillHubRemoteUpdateApplyRequest,
    ) -> anyhow::Result<SkillRemoteInstallResponse> {
        let url = format!("{}/skill/hub/update/apply", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to apply skill hub remote update: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillRemoteInstallResponse>()?)
    }

    pub fn detach_skill_hub_managed(
        &self,
        req: &SkillHubManagedDetachRequest,
    ) -> anyhow::Result<SkillHubManagedDetachResponse> {
        let url = format!("{}/skill/hub/detach", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to detach skill hub managed skill: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubManagedDetachResponse>()?)
    }

    pub fn remove_skill_hub_managed(
        &self,
        req: &SkillHubManagedRemoveRequest,
    ) -> anyhow::Result<SkillHubManagedRemoveResponse> {
        let url = format!("{}/skill/hub/remove", self.base_url);
        let response = self.client.post(&url).json(req).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to remove skill hub managed skill: {} - {}",
                status,
                text
            );
        }

        Ok(response.json::<SkillHubManagedRemoveResponse>()?)
    }

    pub fn get_mcp_status(&self) -> anyhow::Result<Vec<McpStatusInfo>> {
        let url = format!("{}/mcp", self.base_url);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to fetch MCP status: {} - {}", status, text);
        }

        let mut servers: Vec<McpStatusInfo> = response
            .json::<HashMap<String, McpStatusInfo>>()?
            .into_values()
            .collect();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(servers)
    }

    pub fn start_mcp_auth(&self, name: &str) -> anyhow::Result<McpAuthStartInfo> {
        let url = format!("{}/mcp/{}/auth", self.base_url, name);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to start MCP auth `{}`: {} - {}", name, status, text);
        }
        Ok(response.json::<McpAuthStartInfo>()?)
    }

    pub fn authenticate_mcp(&self, name: &str) -> anyhow::Result<McpStatusInfo> {
        let url = format!("{}/mcp/{}/auth/authenticate", self.base_url, name);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to authenticate MCP `{}`: {} - {}",
                name,
                status,
                text
            );
        }
        Ok(response.json::<McpStatusInfo>()?)
    }

    pub fn remove_mcp_auth(&self, name: &str) -> anyhow::Result<bool> {
        let url = format!("{}/mcp/{}/auth", self.base_url, name);
        let response = self.client.delete(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to remove MCP auth `{}`: {} - {}",
                name,
                status,
                text
            );
        }
        let value = response.json::<serde_json::Value>()?;
        Ok(value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub fn connect_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let url = format!("{}/mcp/{}/connect", self.base_url, name);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to connect MCP `{}`: {} - {}", name, status, text);
        }
        Ok(response.json::<bool>().unwrap_or(true))
    }

    pub fn disconnect_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let url = format!("{}/mcp/{}/disconnect", self.base_url, name);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to disconnect MCP `{}`: {} - {}", name, status, text);
        }
        Ok(response.json::<bool>().unwrap_or(true))
    }

    pub fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<MessageInfo>> {
        self.get_messages_after(session_id, None, None)
    }

    pub fn get_messages_after(
        &self,
        session_id: &str,
        after: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<MessageInfo>> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(after) = after.map(str::trim).filter(|value| !value.is_empty()) {
            params.push(("after", after.to_string()));
        }
        if let Some(limit) = limit.filter(|value| *value > 0) {
            params.push(("limit", limit.to_string()));
        }
        let request = if params.is_empty() {
            self.client.get(&url)
        } else {
            self.client.get(&url).query(&params)
        };

        let response = request.send()?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get messages: {} - {}", status, text);
        }

        let messages: Vec<MessageInfo> = response.json()?;
        Ok(messages)
    }

    pub fn get_lsp_servers(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/lsp", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get LSP status: {} - {}", status, text);
        }
        let status = response.json::<LspStatusResponse>()?;
        Ok(status.servers)
    }

    pub fn get_formatters(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/formatter", self.base_url);
        let response = self.client.get(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!("Failed to get formatter status: {} - {}", status, text);
        }
        let status = response.json::<FormatterStatusResponse>()?;
        Ok(status.formatters)
    }

    pub fn share_session(&self, session_id: &str) -> anyhow::Result<ShareResponse> {
        let url = format!("{}/session/{}/share", self.base_url, session_id);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to share session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        Ok(response.json::<ShareResponse>()?)
    }

    pub fn unshare_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let url = format!("{}/session/{}/share", self.base_url, session_id);
        let response = self.client.delete(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to unshare session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        let value = response.json::<serde_json::Value>()?;
        Ok(value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    pub fn compact_session(&self, session_id: &str) -> anyhow::Result<CompactResponse> {
        let url = format!("{}/session/{}/compact", self.base_url, session_id);
        let response = self.client.post(&url).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to compact session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        Ok(response.json::<CompactResponse>()?)
    }

    pub fn revert_session(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> anyhow::Result<RevertResponse> {
        let url = format!("{}/session/{}/revert", self.base_url, session_id);
        let request = RevertRequest {
            message_id: message_id.to_string(),
        };
        let response = self.client.post(&url).json(&request).send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to revert session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        Ok(response.json::<RevertResponse>()?)
    }

    pub fn fork_session(
        &self,
        session_id: &str,
        message_id: Option<&str>,
    ) -> anyhow::Result<SessionInfo> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(msg_id) = message_id {
            params.push(("message_id", msg_id.to_string()));
        }
        let url = format!("{}/session/{}/fork", self.base_url, session_id);
        let request = if params.is_empty() {
            self.client.post(&url)
        } else {
            self.client.post(&url).query(&params)
        };
        let response = request.send()?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            anyhow::bail!(
                "Failed to fork session `{}`: {} - {}",
                session_id,
                status,
                text
            );
        }
        Ok(response.json::<SessionInfo>()?)
    }

    pub fn set_current_session(&self, session: SessionInfo) {
        let mut current = futures::executor::block_on(self.current_session.write());
        *current = Some(session);
    }

    pub fn get_current_session(&self) -> Option<SessionInfo> {
        let current = futures::executor::block_on(self.current_session.read());
        current.clone()
    }
}
