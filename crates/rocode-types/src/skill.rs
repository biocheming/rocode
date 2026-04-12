use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    Bundled,
    LocalPath,
    Git,
    Archive,
    Registry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSourceRef {
    pub source_id: String,
    pub source_kind: SkillSourceKind,
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSourceIndexEntry {
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSourceIndexSnapshot {
    pub source: SkillSourceRef,
    pub updated_at: i64,
    #[serde(default)]
    pub entries: Vec<SkillSourceIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillDistributionResolverKind {
    Bundled,
    LocalPath,
    RegistryIndex,
    RegistryManifest,
    ArchiveManifest,
    GitCheckout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillArtifactKind {
    RegistryPackage,
    GitCheckout,
    Archive,
    LocalSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillArtifactRef {
    pub artifact_id: String,
    pub kind: SkillArtifactKind,
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDistributionRelease {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDistributionResolution {
    pub resolved_at: i64,
    pub resolver_kind: SkillDistributionResolverKind,
    pub artifact: SkillArtifactRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillInstalledDistribution {
    pub installed_at: i64,
    pub workspace_skill_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillManagedLifecycleState {
    Indexed,
    Resolved,
    Fetched,
    PlannedInstall,
    Installed,
    UpdateAvailable,
    Diverged,
    Detached,
    RemovePending,
    Removed,
    ResolutionFailed,
    FetchFailed,
    ApplyFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDistributionRecord {
    pub distribution_id: String,
    pub source: SkillSourceRef,
    pub skill_name: String,
    pub release: SkillDistributionRelease,
    pub resolution: SkillDistributionResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<SkillInstalledDistribution>,
    pub lifecycle: SkillManagedLifecycleState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillArtifactCacheStatus {
    Fetched,
    Extracted,
    Failed,
    Evicted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillArtifactCacheEntry {
    pub artifact: SkillArtifactRef,
    pub cached_at: i64,
    pub local_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracted_path: Option<String>,
    pub status: SkillArtifactCacheStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillManagedLifecycleRecord {
    pub distribution_id: String,
    pub source_id: String,
    pub skill_name: String,
    pub state: SkillManagedLifecycleState,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundledSkillManifestEntry {
    pub skill_name: String,
    pub relative_path: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BundledSkillManifest {
    pub bundle_id: String,
    #[serde(default)]
    pub entries: Vec<BundledSkillManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedSkillRecord {
    pub skill_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SkillSourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<i64>,
    #[serde(default)]
    pub locally_modified: bool,
    #[serde(default)]
    pub deleted_locally: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSyncAction {
    Install,
    Update,
    SkipLocalModification,
    SkipDeletedLocally,
    RemoveManaged,
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSyncEntry {
    pub skill_name: String,
    pub action: SkillSyncAction,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSyncPlan {
    pub source_id: String,
    #[serde(default)]
    pub entries: Vec<SkillSyncEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubManagedResponse {
    #[serde(default)]
    pub managed_skills: Vec<ManagedSkillRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubDistributionResponse {
    #[serde(default)]
    pub distributions: Vec<SkillDistributionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubArtifactCacheResponse {
    #[serde(default)]
    pub artifact_cache: Vec<SkillArtifactCacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubPolicy {
    pub artifact_cache_retention_seconds: u64,
    pub fetch_timeout_ms: u64,
    pub max_download_bytes: u64,
    pub max_extract_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubPolicyResponse {
    pub policy: SkillHubPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubLifecycleResponse {
    #[serde(default)]
    pub lifecycle: Vec<SkillManagedLifecycleRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillRemoteInstallAction {
    Install,
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRemoteInstallEntry {
    pub distribution_id: String,
    pub source_id: String,
    pub skill_name: String,
    pub action: SkillRemoteInstallAction,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRemoteInstallPlan {
    pub source_id: String,
    pub distribution: SkillDistributionRecord,
    pub entry: SkillRemoteInstallEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRemoteInstallResponse {
    pub plan: SkillRemoteInstallPlan,
    pub artifact_cache: SkillArtifactCacheEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_report: Option<SkillGuardReport>,
    pub result: SkillGovernanceWriteResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubRemoteInstallPlanRequest {
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubRemoteInstallApplyRequest {
    pub session_id: String,
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubRemoteUpdatePlanRequest {
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubRemoteUpdateApplyRequest {
    pub session_id: String,
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillGovernanceWriteResult {
    pub action: String,
    pub skill_name: String,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supporting_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubIndexResponse {
    #[serde(default)]
    pub source_indices: Vec<SkillSourceIndexSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubIndexRefreshRequest {
    pub source: SkillSourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubIndexRefreshResponse {
    pub snapshot: SkillSourceIndexSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubManagedDetachRequest {
    pub session_id: String,
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubManagedDetachResponse {
    pub lifecycle: SkillManagedLifecycleRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubManagedRemoveRequest {
    pub session_id: String,
    pub source: SkillSourceRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubManagedRemoveResponse {
    pub lifecycle: SkillManagedLifecycleRecord,
    #[serde(default)]
    pub deleted_from_workspace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<SkillGovernanceWriteResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubAuditResponse {
    #[serde(default)]
    pub audit_events: Vec<SkillAuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SkillHubTimelineQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubTimelineResponse {
    #[serde(default)]
    pub entries: Vec<SkillGovernanceTimelineEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubSyncPlanRequest {
    pub source: SkillSourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubSyncApplyRequest {
    pub session_id: String,
    pub source: SkillSourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubSyncPlanResponse {
    pub plan: SkillSyncPlan,
    #[serde(default)]
    pub guard_reports: Vec<SkillGuardReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubGuardRunRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SkillSourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillHubGuardRunResponse {
    #[serde(default)]
    pub reports: Vec<SkillGuardReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillGovernanceTimelineKind {
    ManagedSnapshot,
    SourceIndexRefreshed,
    SourceResolved,
    ArtifactFetched,
    ArtifactEvicted,
    ArtifactFetchFailed,
    RemoteInstallPlanned,
    RemoteUpdatePlanned,
    LifecycleTransitioned,
    Create,
    Patch,
    Edit,
    Delete,
    WriteFile,
    RemoveFile,
    HubInstall,
    HubUpdate,
    HubDetach,
    HubRemove,
    SyncPlanCreated,
    SyncApplyCompleted,
    GuardBlocked,
    GuardWarned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillGovernanceTimelineStatus {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillGovernanceTimelineEntry {
    pub entry_id: String,
    pub kind: SkillGovernanceTimelineKind,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    pub title: String,
    pub summary: String,
    pub status: SkillGovernanceTimelineStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_record: Option<ManagedSkillRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard_report: Option<SkillGuardReport>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillGuardSeverity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillGuardStatus {
    Passed,
    Warn,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillGuardViolation {
    pub rule_id: String,
    pub severity: SkillGuardSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillGuardReport {
    pub skill_name: String,
    pub status: SkillGuardStatus,
    #[serde(default)]
    pub violations: Vec<SkillGuardViolation>,
    pub scanned_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillAuditKind {
    SourceIndexRefreshed,
    SourceResolved,
    ArtifactFetched,
    ArtifactEvicted,
    ArtifactFetchFailed,
    RemoteInstallPlanned,
    RemoteUpdatePlanned,
    LifecycleTransitioned,
    Create,
    Patch,
    Edit,
    Delete,
    WriteFile,
    RemoveFile,
    HubInstall,
    HubUpdate,
    HubDetach,
    HubRemove,
    SyncPlanCreated,
    SyncApplyCompleted,
    GuardBlocked,
    GuardWarned,
}

impl From<SkillAuditKind> for SkillGovernanceTimelineKind {
    fn from(value: SkillAuditKind) -> Self {
        match value {
            SkillAuditKind::SourceIndexRefreshed => Self::SourceIndexRefreshed,
            SkillAuditKind::SourceResolved => Self::SourceResolved,
            SkillAuditKind::ArtifactFetched => Self::ArtifactFetched,
            SkillAuditKind::ArtifactEvicted => Self::ArtifactEvicted,
            SkillAuditKind::ArtifactFetchFailed => Self::ArtifactFetchFailed,
            SkillAuditKind::RemoteInstallPlanned => Self::RemoteInstallPlanned,
            SkillAuditKind::RemoteUpdatePlanned => Self::RemoteUpdatePlanned,
            SkillAuditKind::LifecycleTransitioned => Self::LifecycleTransitioned,
            SkillAuditKind::Create => Self::Create,
            SkillAuditKind::Patch => Self::Patch,
            SkillAuditKind::Edit => Self::Edit,
            SkillAuditKind::Delete => Self::Delete,
            SkillAuditKind::WriteFile => Self::WriteFile,
            SkillAuditKind::RemoveFile => Self::RemoveFile,
            SkillAuditKind::HubInstall => Self::HubInstall,
            SkillAuditKind::HubUpdate => Self::HubUpdate,
            SkillAuditKind::HubDetach => Self::HubDetach,
            SkillAuditKind::HubRemove => Self::HubRemove,
            SkillAuditKind::SyncPlanCreated => Self::SyncPlanCreated,
            SkillAuditKind::SyncApplyCompleted => Self::SyncApplyCompleted,
            SkillAuditKind::GuardBlocked => Self::GuardBlocked,
            SkillAuditKind::GuardWarned => Self::GuardWarned,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillAuditEvent {
    pub event_id: String,
    pub kind: SkillAuditKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub actor: String,
    pub created_at: i64,
    #[serde(default)]
    pub payload: serde_json::Value,
}
