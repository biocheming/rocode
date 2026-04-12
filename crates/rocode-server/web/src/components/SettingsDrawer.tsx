import { useCallback, useEffect, useMemo, useState } from "react";
import { cn } from "@/lib/utils";
import { SkillGovernanceTimeline } from "./SkillGovernanceTimeline";

type SettingsTabId =
  | "general"
  | "providers"
  | "scheduler"
  | "skills"
  | "mcp"
  | "plugins"
  | "lsp";

interface ThemeOption {
  id: string;
  label: string;
}

interface ModeOption {
  key: string;
  label: string;
}

interface ModelOption {
  key: string;
  label: string;
}

interface ProviderModelLike {
  id: string;
  name?: string;
}

interface ProviderRecordLike {
  id: string;
  name: string;
  models?: ProviderModelLike[];
}

interface KnownProviderEntryLike {
  id: string;
  name: string;
  env?: string[];
  connected?: boolean;
  model_count?: number;
  base_url?: string | null;
  protocol?: string | null;
}

interface ConnectProtocolOptionLike {
  id: string;
  name: string;
}

type ProviderConnectDraftModeLike = "known" | "custom";

interface ProviderConnectDraftLike {
  mode: ProviderConnectDraftModeLike;
  provider_id: string;
  known_provider_id?: string | null;
  name?: string | null;
  base_url?: string | null;
  protocol?: string | null;
  env?: string[];
  connected?: boolean;
  model_count?: number;
  supports_api_key_connect?: boolean;
}

interface ResolveProviderConnectResponseLike {
  query: string;
  suggested_mode: ProviderConnectDraftModeLike;
  exact_match: boolean;
  matches: KnownProviderEntryLike[];
  draft: ProviderConnectDraftLike;
  custom_draft: ProviderConnectDraftLike;
}

interface ManagedProviderInfo {
  id: string;
  name: string;
  status: string;
  connected: boolean;
  configured: boolean;
  known: boolean;
  has_auth: boolean;
  auth_type?: string | null;
  env?: string[];
  base_url?: string | null;
  protocol?: string | null;
  model_overrides?: Array<{ key: string }>;
  models?: ProviderModelLike[];
}

interface SchedulerProfileSummary {
  key: string;
  orchestrator?: string | null;
  description?: string | null;
  stages: string[];
}

interface SchedulerConfigResponse {
  raw_path?: string | null;
  resolved_path?: string | null;
  exists: boolean;
  content: string;
  default_profile?: string | null;
  profiles: SchedulerProfileSummary[];
  parse_error?: string | null;
}

interface McpStatusInfo {
  name: string;
  status: string;
  tools: number;
  resources: number;
  error?: string | null;
}

interface PluginAuthProviderInfo {
  provider: string;
  methods: Array<{ type?: string; label?: string }>;
}

interface LspStatus {
  servers: string[];
}

interface FormatterStatus {
  formatters: string[];
}

interface SkillCatalogEntry {
  name: string;
  description: string;
  category?: string | null;
  location: string;
  writable: boolean;
  supporting_files: string[];
}

interface SkillFileRefLike {
  relative_path: string;
  location: string;
}

interface LoadedSkillMetaLike {
  name: string;
  description: string;
  category?: string | null;
  location: string;
  supporting_files: SkillFileRefLike[];
}

interface LoadedSkillLike {
  meta: LoadedSkillMetaLike;
  content: string;
}

interface SkillDetailResponse {
  skill: LoadedSkillLike;
  source: string;
  writable: boolean;
}

interface SkillManageResponseLike {
  result: {
    action: string;
    skill_name: string;
    location: string;
    supporting_file?: string | null;
  };
  guard_report?: SkillGuardReportLike | null;
}

interface SkillGuardViolationLike {
  rule_id: string;
  severity: "info" | "warn" | "error";
  message: string;
  file_path?: string | null;
}

interface SkillGuardReportLike {
  skill_name: string;
  status: "passed" | "warn" | "blocked";
  violations: SkillGuardViolationLike[];
  scanned_at: number;
}

interface SkillSourceRefLike {
  source_id: string;
  source_kind: "bundled" | "local_path" | "git" | "archive" | "registry";
  locator: string;
  revision?: string | null;
}

interface ManagedSkillRecordLike {
  skill_name: string;
  source?: SkillSourceRefLike | null;
  installed_revision?: string | null;
  local_hash?: string | null;
  last_synced_at?: number | null;
  locally_modified: boolean;
  deleted_locally: boolean;
}

interface SkillSourceIndexEntryLike {
  skill_name: string;
  description?: string | null;
  category?: string | null;
  revision?: string | null;
}

interface SkillSourceIndexSnapshotLike {
  source: SkillSourceRefLike;
  updated_at: number;
  entries: SkillSourceIndexEntryLike[];
}

interface SkillAuditEventLike {
  event_id: string;
  kind: string;
  skill_name?: string | null;
  source_id?: string | null;
  actor: string;
  created_at: number;
  payload: unknown;
}

interface SkillHubManagedResponseLike {
  managed_skills: ManagedSkillRecordLike[];
}

interface SkillHubIndexResponseLike {
  source_indices: SkillSourceIndexSnapshotLike[];
}

interface SkillHubIndexRefreshResponseLike {
  snapshot: SkillSourceIndexSnapshotLike;
}

interface SkillArtifactRefLike {
  artifact_id: string;
  kind: string;
  locator: string;
  checksum?: string | null;
  size_bytes?: number | null;
}

interface SkillDistributionReleaseLike {
  version?: string | null;
  revision?: string | null;
  checksum?: string | null;
  manifest_path?: string | null;
  published_at?: number | null;
}

interface SkillDistributionResolutionLike {
  resolved_at: number;
  resolver_kind: string;
  artifact: SkillArtifactRefLike;
}

interface SkillInstalledDistributionLike {
  installed_at: number;
  workspace_skill_path: string;
  installed_revision?: string | null;
  local_hash?: string | null;
}

interface SkillDistributionRecordLike {
  distribution_id: string;
  source: SkillSourceRefLike;
  skill_name: string;
  release: SkillDistributionReleaseLike;
  resolution: SkillDistributionResolutionLike;
  installed?: SkillInstalledDistributionLike | null;
  lifecycle: string;
}

interface SkillManagedLifecycleRecordLike {
  distribution_id: string;
  source_id: string;
  skill_name: string;
  state: string;
  updated_at: number;
  error?: string | null;
}

interface SkillHubDistributionResponseLike {
  distributions: SkillDistributionRecordLike[];
}

interface SkillHubArtifactCacheResponseLike {
  artifact_cache: SkillArtifactCacheEntryLike[];
}

interface SkillHubPolicyLike {
  artifact_cache_retention_seconds: number;
  fetch_timeout_ms: number;
  max_download_bytes: number;
  max_extract_bytes: number;
}

interface SkillHubPolicyResponseLike {
  policy: SkillHubPolicyLike;
}

interface SkillHubLifecycleResponseLike {
  lifecycle: SkillManagedLifecycleRecordLike[];
}

interface SkillHubAuditResponseLike {
  audit_events: SkillAuditEventLike[];
}

interface SkillGovernanceTimelineEntryLike {
  entry_id: string;
  kind: string;
  created_at: number;
  skill_name?: string | null;
  source_id?: string | null;
  actor?: string | null;
  title: string;
  summary: string;
  status: "info" | "success" | "warn" | "error";
  managed_record?: ManagedSkillRecordLike | null;
  guard_report?: SkillGuardReportLike | null;
}

interface SkillHubTimelineResponseLike {
  entries: SkillGovernanceTimelineEntryLike[];
}

interface SkillSyncEntryLike {
  skill_name: string;
  action: string;
  reason: string;
}

interface SkillSyncPlanLike {
  source_id: string;
  entries: SkillSyncEntryLike[];
}

interface SkillHubSyncPlanResponseLike {
  plan: SkillSyncPlanLike;
  guard_reports?: SkillGuardReportLike[];
}

interface SkillHubGuardRunRequestLike {
  skill_name?: string;
  source?: SkillSourceRefLike;
}

interface SkillHubGuardRunResponseLike {
  reports: SkillGuardReportLike[];
}

interface SkillRemoteInstallEntryLike {
  distribution_id: string;
  source_id: string;
  skill_name: string;
  action: "install" | "update";
  reason: string;
}

interface SkillRemoteInstallPlanLike {
  source_id: string;
  distribution: SkillDistributionRecordLike;
  entry: SkillRemoteInstallEntryLike;
}

interface SkillArtifactCacheEntryLike {
  artifact: SkillArtifactRefLike;
  cached_at: number;
  local_path: string;
  extracted_path?: string | null;
  status: string;
  error?: string | null;
}

interface SkillGovernanceWriteResultLike {
  action: string;
  skill_name: string;
  location: string;
  supporting_file?: string | null;
}

interface SkillRemoteInstallResponseLike {
  plan: SkillRemoteInstallPlanLike;
  artifact_cache: SkillArtifactCacheEntryLike;
  guard_report?: SkillGuardReportLike | null;
  result: SkillGovernanceWriteResultLike;
}

interface SkillHubManagedDetachResponseLike {
  lifecycle: SkillManagedLifecycleRecordLike;
}

interface SkillHubManagedRemoveResponseLike {
  lifecycle: SkillManagedLifecycleRecordLike;
  deleted_from_workspace: boolean;
  result?: SkillGovernanceWriteResultLike | null;
}

interface RefreshProviderCatalogueResponse {
  changed: boolean;
  generation_before: number;
  generation_after: number;
  status: "updated" | "not_modified" | "fallback_cached";
  error_message?: string | null;
}

interface AppConfigSnapshot extends Record<string, unknown> {
  provider?: Record<string, unknown>;
  plugin?: Record<string, unknown>;
  mcp?: Record<string, unknown>;
  schedulerPath?: string | null;
}

interface SettingsDrawerProps {
  onClose: () => void;
  theme: string;
  themes: ThemeOption[];
  onThemeChange: (themeId: string) => void;
  workspaceMode: "shared" | "isolated" | null;
  workspaceRootPath: string;
  workspaceConfigDir?: string | null;
  selectedSessionId: string | null;
  modeOptions: ModeOption[];
  selectedMode: string;
  onModeChange: (mode: string) => void;
  modelOptions: ModelOption[];
  selectedModel: string;
  onModelChange: (model: string) => void;
  showThinking: boolean;
  onShowThinkingChange: (value: boolean) => void;
  providers: ProviderRecordLike[];
  knownProviders: KnownProviderEntryLike[];
  connectProtocols: ConnectProtocolOptionLike[];
  connectQuery: string;
  onConnectQueryChange: (value: string) => void;
  connectResolution: ResolveProviderConnectResponseLike | null;
  connectResolveBusy: boolean;
  connectResolveError: string | null;
  connectProviderId: string;
  onConnectProviderIdChange: (value: string) => void;
  connectProtocol: string;
  onConnectProtocolChange: (value: string) => void;
  connectApiKey: string;
  onConnectApiKeyChange: (value: string) => void;
  connectBaseUrl: string;
  onConnectBaseUrlChange: (value: string) => void;
  connectBusy: boolean;
  onConnectProvider: () => Promise<void>;
  api: (path: string, options?: RequestInit) => Promise<Response>;
  apiJson: <T>(path: string, options?: RequestInit) => Promise<T>;
  onBanner: (message: string) => void;
  onReloadCoreData: () => Promise<void>;
}

const SETTINGS_TABS: Array<{ id: SettingsTabId; label: string }> = [
  { id: "general", label: "General" },
  { id: "providers", label: "Providers" },
  { id: "scheduler", label: "Scheduler" },
  { id: "skills", label: "Skills" },
  { id: "mcp", label: "MCP" },
  { id: "plugins", label: "Plugins" },
  { id: "lsp", label: "LSP" },
];

function isolatedWorkspaceNotice(tab: SettingsTabId): string | null {
  switch (tab) {
    case "general":
      return "These settings are still persisted to global config. In isolated mode, that persisted global copy does not become the current sandbox runtime unless you switch back to a shared workspace.";
    case "providers":
      return "Provider and model changes made here target global config or shared provider state. The current isolated sandbox will not inherit those global config changes unless the same intent is expressed inside this workspace's .rocode.";
    case "scheduler":
      return "Scheduler edits here write global config. The active isolated sandbox will continue resolving scheduler behavior from its local workspace authority until you switch to shared mode or add matching workspace-local config.";
    case "skills":
      return "Skill mutations here always target this workspace's .rocode/skills. In isolated mode, that means the current sandbox stays local and does not inherit or modify global skill config.";
    case "mcp":
      return "MCP config saved here is global. An isolated workspace does not automatically inherit that global config into its current sandbox runtime.";
    case "plugins":
      return "Plugin config saved here is global. The current isolated sandbox will not inherit those global config changes unless they are mirrored into this workspace's .rocode authority.";
    default:
      return null;
  }
}

function managedSkillStateLabel(record: ManagedSkillRecordLike): string {
  if (record.deleted_locally) return "deleted locally";
  if (record.locally_modified) return "locally modified";
  return "managed clean";
}

function latestGuardStatusLabel(report: SkillGuardReportLike): string {
  switch (report.status) {
    case "blocked":
      return "guard blocked";
    case "warn":
      return "guard warn";
    default:
      return "guard passed";
  }
}

function lifecycleStatusClass(state: string): string {
  const normalized = state.trim().toLowerCase();
  if (normalized.includes("failed") || normalized === "diverged") {
    return "border-red-300 bg-red-50 text-red-800 dark:border-red-700 dark:bg-red-950/60 dark:text-red-300";
  }
  if (
    normalized === "updateavailable" ||
    normalized === "update_available" ||
    normalized === "plannedinstall" ||
    normalized === "planned_install" ||
    normalized === "removepending" ||
    normalized === "remove_pending"
  ) {
    return "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200";
  }
  if (normalized === "installed" || normalized === "fetched" || normalized === "resolved") {
    return "border-green-300 bg-green-50 text-green-800 dark:border-green-700 dark:bg-green-950 dark:text-green-300";
  }
  return "border-border bg-card/80 text-muted-foreground";
}

function unixTimeLabel(value?: number | null): string {
  if (!value) return "--";
  try {
    return new Date(value * 1000).toLocaleString();
  } catch {
    return String(value);
  }
}

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error ?? "Unknown error");
}

function stringifyJson(value: unknown) {
  return JSON.stringify(value ?? {}, null, 2);
}

function formatHubDurationSeconds(value?: number | null): string {
  if (!value) return "--";
  if (value % 86400 === 0) return `${value / 86400}d`;
  if (value % 3600 === 0) return `${value / 3600}h`;
  if (value % 60 === 0) return `${value / 60}m`;
  return `${value}s`;
}

function formatHubDurationMs(value?: number | null): string {
  if (!value) return "--";
  if (value % 1000 === 0) return `${value / 1000}s`;
  return `${value}ms`;
}

function formatHubBytes(value?: number | null): string {
  if (!value) return "--";
  if (value >= 1024 * 1024 && value % (1024 * 1024) === 0) {
    return `${value / (1024 * 1024)} MiB`;
  }
  if (value >= 1024 && value % 1024 === 0) {
    return `${value / 1024} KiB`;
  }
  return `${value} bytes`;
}

function objectRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value as Record<string, unknown>;
}

function parseObjectJson(label: string, raw: string) {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new Error(`${label} JSON cannot be empty`);
  }
  const parsed = JSON.parse(trimmed) as unknown;
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${label} JSON must be an object`);
  }
  return parsed as Record<string, unknown>;
}

function statusTone(status: string | null | undefined) {
  switch ((status || "").toLowerCase()) {
    case "connected":
    case "done":
      return "ok";
    case "needs-auth":
    case "warning":
      return "warn";
    case "error":
    case "failed":
      return "danger";
    default:
      return "muted";
  }
}

export function SettingsDrawer({
  onClose,
  theme,
  themes,
  onThemeChange,
  workspaceMode,
  workspaceRootPath,
  workspaceConfigDir,
  selectedSessionId,
  modeOptions,
  selectedMode,
  onModeChange,
  modelOptions,
  selectedModel,
  onModelChange,
  showThinking,
  onShowThinkingChange,
  providers,
  knownProviders,
  connectProtocols,
  connectQuery,
  onConnectQueryChange,
  connectResolution,
  connectResolveBusy,
  connectResolveError,
  connectProviderId,
  onConnectProviderIdChange,
  connectProtocol,
  onConnectProtocolChange,
  connectApiKey,
  onConnectApiKeyChange,
  connectBaseUrl,
  onConnectBaseUrlChange,
  connectBusy,
  onConnectProvider,
  api,
  apiJson,
  onBanner,
  onReloadCoreData,
}: SettingsDrawerProps) {
  const [activeTab, setActiveTab] = useState<SettingsTabId>("general");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [configSnapshot, setConfigSnapshot] = useState<AppConfigSnapshot | null>(null);
  const [managedProviders, setManagedProviders] = useState<ManagedProviderInfo[]>([]);
  const [schedulerConfig, setSchedulerConfig] = useState<SchedulerConfigResponse | null>(null);
  const [schedulerPathDraft, setSchedulerPathDraft] = useState("");
  const [schedulerContentDraft, setSchedulerContentDraft] = useState("");
  const [mcpStatus, setMcpStatus] = useState<Record<string, McpStatusInfo>>({});
  const [mcpDrafts, setMcpDrafts] = useState<Record<string, string>>({});
  const [newMcpKey, setNewMcpKey] = useState("");
  const [newMcpDraft, setNewMcpDraft] = useState("{\n  \"type\": \"local\",\n  \"command\": \"\"\n}");
  const [pluginAuthProviders, setPluginAuthProviders] = useState<PluginAuthProviderInfo[]>([]);
  const [pluginDrafts, setPluginDrafts] = useState<Record<string, string>>({});
  const [newPluginKey, setNewPluginKey] = useState("");
  const [newPluginDraft, setNewPluginDraft] = useState("{\n  \"command\": \"\",\n  \"args\": []\n}");
  const [lspStatus, setLspStatus] = useState<LspStatus | null>(null);
  const [formatterStatus, setFormatterStatus] = useState<FormatterStatus | null>(null);
  const [skillCatalog, setSkillCatalog] = useState<SkillCatalogEntry[]>([]);
  const [managedSkills, setManagedSkills] = useState<ManagedSkillRecordLike[]>([]);
  const [skillSourceIndices, setSkillSourceIndices] = useState<SkillSourceIndexSnapshotLike[]>([]);
  const [skillDistributions, setSkillDistributions] = useState<SkillDistributionRecordLike[]>([]);
  const [skillArtifactCache, setSkillArtifactCache] = useState<SkillArtifactCacheEntryLike[]>([]);
  const [skillHubPolicy, setSkillHubPolicy] = useState<SkillHubPolicyLike | null>(null);
  const [skillLifecycle, setSkillLifecycle] = useState<SkillManagedLifecycleRecordLike[]>([]);
  const [skillAuditEvents, setSkillAuditEvents] = useState<SkillAuditEventLike[]>([]);
  const [skillGovernanceTimeline, setSkillGovernanceTimeline] = useState<SkillGovernanceTimelineEntryLike[]>([]);
  const [skillSyncSourceId, setSkillSyncSourceId] = useState("");
  const [skillSyncSourceKind, setSkillSyncSourceKind] = useState<SkillSourceRefLike["source_kind"]>("local_path");
  const [skillSyncLocator, setSkillSyncLocator] = useState("");
  const [skillSyncRevision, setSkillSyncRevision] = useState("");
  const [skillSyncPlan, setSkillSyncPlan] = useState<SkillSyncPlanLike | null>(null);
  const [remoteInstallSkillName, setRemoteInstallSkillName] = useState("");
  const [remoteInstallPlan, setRemoteInstallPlan] = useState<SkillRemoteInstallPlanLike | null>(null);
  const [skillGuardReports, setSkillGuardReports] = useState<SkillGuardReportLike[]>([]);
  const [skillGuardTarget, setSkillGuardTarget] = useState<string | null>(null);
  const [selectedSkillName, setSelectedSkillName] = useState<string | null>(null);
  const [skillDetail, setSkillDetail] = useState<SkillDetailResponse | null>(null);
  const [skillDetailLoading, setSkillDetailLoading] = useState(false);
  const [skillEditorContent, setSkillEditorContent] = useState("");
  const [newSkillName, setNewSkillName] = useState("");
  const [newSkillDescription, setNewSkillDescription] = useState("");
  const [newSkillCategory, setNewSkillCategory] = useState("");
  const [newSkillBody, setNewSkillBody] = useState("");

  const mcpConfigs = useMemo(
    () => objectRecord(configSnapshot?.mcp),
    [configSnapshot?.mcp],
  );
  const pluginConfigs = useMemo(
    () => objectRecord(configSnapshot?.plugin),
    [configSnapshot?.plugin],
  );
  const isolatedNotice = workspaceMode === "isolated" ? isolatedWorkspaceNotice(activeTab) : null;
  const connectMatches = connectResolution?.matches ?? [];
  const exactKnownProvider = connectResolution?.exact_match ? connectResolution.draft : null;
  const selectedSkillEntry = useMemo(
    () =>
      skillCatalog.find(
        (skill) =>
          skill.name.trim().toLowerCase() === (selectedSkillName ?? "").trim().toLowerCase(),
      ) ?? null,
    [selectedSkillName, skillCatalog],
  );
  const managedRecordBySkill = useMemo(
    () =>
      new Map(
        managedSkills.map((record) => [record.skill_name.trim().toLowerCase(), record] as const),
      ),
    [managedSkills],
  );
  const selectedHubSourceSnapshot = useMemo(
    () =>
      skillSourceIndices.find(
        (snapshot) =>
          snapshot.source.source_id.trim().toLowerCase() ===
          skillSyncSourceId.trim().toLowerCase(),
      ) ?? null,
    [skillSourceIndices, skillSyncSourceId],
  );
  const selectedRemoteSourceEntries = useMemo(
    () => selectedHubSourceSnapshot?.entries ?? [],
    [selectedHubSourceSnapshot],
  );
  const selectedRemoteSourceEntry = useMemo(
    () =>
      selectedRemoteSourceEntries.find(
        (entry) =>
          entry.skill_name.trim().toLowerCase() ===
          remoteInstallSkillName.trim().toLowerCase(),
      ) ?? null,
    [remoteInstallSkillName, selectedRemoteSourceEntries],
  );
  const selectedRemoteDistribution = useMemo(() => {
    const matches = skillDistributions
      .filter(
        (record) =>
          record.source.source_id.trim().toLowerCase() ===
            skillSyncSourceId.trim().toLowerCase() &&
          record.skill_name.trim().toLowerCase() ===
            remoteInstallSkillName.trim().toLowerCase(),
      )
      .sort(
        (left, right) =>
          (right.resolution?.resolved_at ?? 0) - (left.resolution?.resolved_at ?? 0),
      );
    return matches[0] ?? null;
  }, [remoteInstallSkillName, skillDistributions, skillSyncSourceId]);
  const selectedRemoteLifecycle = useMemo(() => {
    if (selectedRemoteDistribution) {
      return (
        skillLifecycle.find(
          (record) => record.distribution_id === selectedRemoteDistribution.distribution_id,
        ) ?? null
      );
    }
    const matches = skillLifecycle
      .filter(
        (record) =>
          record.source_id.trim().toLowerCase() === skillSyncSourceId.trim().toLowerCase() &&
          record.skill_name.trim().toLowerCase() ===
            remoteInstallSkillName.trim().toLowerCase(),
      )
      .sort((left, right) => right.updated_at - left.updated_at);
    return matches[0] ?? null;
  }, [remoteInstallSkillName, selectedRemoteDistribution, skillLifecycle, skillSyncSourceId]);
  const selectedRemoteArtifactCache = useMemo(() => {
    if (!selectedRemoteDistribution) {
      return null;
    }
    return (
      skillArtifactCache.find(
        (entry) =>
          entry.artifact.artifact_id ===
          selectedRemoteDistribution.resolution.artifact.artifact_id,
      ) ?? null
    );
  }, [selectedRemoteDistribution, skillArtifactCache]);
  const latestGuardBySkill = useMemo(() => {
    const result = new Map<string, SkillGuardReportLike>();
    for (const entry of skillGovernanceTimeline) {
      const key = entry.skill_name?.trim().toLowerCase();
      if (!key || result.has(key) || !entry.guard_report) {
        continue;
      }
      result.set(key, entry.guard_report);
    }
    return result;
  }, [skillGovernanceTimeline]);
  const skillWorkspaceRoot = useMemo(() => {
    const trimmed = workspaceRootPath.trim();
    if (!trimmed) return ".rocode/skills";
    return `${trimmed.replace(/\/+$/, "")}/.rocode/skills`;
  }, [workspaceRootPath]);
  const skillsMutationsEnabled = Boolean(selectedSessionId);

  const reloadSettingsData = useCallback(async () => {
    setRefreshing(true);
    setFeedback(null);
    try {
      const skillCatalogPath = selectedSessionId
        ? `/skill/catalog?session_id=${encodeURIComponent(selectedSessionId)}`
        : "/skill/catalog";
      const [config, managed, scheduler, mcp, plugins, lsp, formatter, skills, skillHubManaged, skillHubIndex, skillHubDistributions, skillHubArtifactCache, skillHubPolicyResponse, skillHubLifecycle, skillHubAudit, skillHubTimeline] =
        await Promise.all([
          apiJson<AppConfigSnapshot>("/config"),
          apiJson<{ providers: ManagedProviderInfo[] }>("/provider/managed"),
          apiJson<SchedulerConfigResponse>("/config/scheduler"),
          apiJson<Record<string, McpStatusInfo>>("/mcp"),
          apiJson<PluginAuthProviderInfo[]>("/plugin/auth").catch(() => []),
          apiJson<LspStatus>("/lsp"),
          apiJson<FormatterStatus>("/formatter"),
          apiJson<SkillCatalogEntry[]>(skillCatalogPath),
          apiJson<SkillHubManagedResponseLike>("/skill/hub/managed"),
          apiJson<SkillHubIndexResponseLike>("/skill/hub/index"),
          apiJson<SkillHubDistributionResponseLike>("/skill/hub/distributions"),
          apiJson<SkillHubArtifactCacheResponseLike>("/skill/hub/artifact-cache"),
          apiJson<SkillHubPolicyResponseLike>("/skill/hub/policy"),
          apiJson<SkillHubLifecycleResponseLike>("/skill/hub/lifecycle"),
          apiJson<SkillHubAuditResponseLike>("/skill/hub/audit"),
          apiJson<SkillHubTimelineResponseLike>("/skill/hub/timeline?limit=120"),
        ]);
      setConfigSnapshot(config);
      setManagedProviders(managed.providers ?? []);
      setSchedulerConfig(scheduler);
      setSchedulerPathDraft(scheduler.raw_path ?? "");
      setSchedulerContentDraft(scheduler.content ?? "");
      setMcpStatus(mcp ?? {});
      setMcpDrafts(
        Object.fromEntries(
          Object.entries(objectRecord(config.mcp)).map(([key, value]) => [key, stringifyJson(value)]),
        ),
      );
      setPluginAuthProviders(plugins ?? []);
      setPluginDrafts(
        Object.fromEntries(
          Object.entries(objectRecord(config.plugin)).map(([key, value]) => [key, stringifyJson(value)]),
        ),
      );
      setLspStatus(lsp);
      setFormatterStatus(formatter);
      setSkillCatalog(skills ?? []);
      setManagedSkills(skillHubManaged.managed_skills ?? []);
      setSkillSourceIndices(skillHubIndex.source_indices ?? []);
      setSkillDistributions(skillHubDistributions.distributions ?? []);
      setSkillArtifactCache(skillHubArtifactCache.artifact_cache ?? []);
      setSkillHubPolicy(skillHubPolicyResponse.policy ?? null);
      setSkillLifecycle(skillHubLifecycle.lifecycle ?? []);
      setSkillAuditEvents(skillHubAudit.audit_events ?? []);
      setSkillGovernanceTimeline(skillHubTimeline.entries ?? []);
    } catch (error) {
      const message = `Failed to load settings data: ${formatError(error)}`;
      setFeedback(message);
      onBanner(message);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [apiJson, onBanner, selectedSessionId]);

  useEffect(() => {
    void reloadSettingsData();
  }, [reloadSettingsData]);

  useEffect(() => {
    if (skillCatalog.length === 0) {
      setSelectedSkillName(null);
      setSkillDetail(null);
      setSkillDetailLoading(false);
      setSkillEditorContent("");
      return;
    }

    const current = (selectedSkillName ?? "").trim().toLowerCase();
    const matched = current
      ? skillCatalog.find((skill) => skill.name.trim().toLowerCase() === current)
      : null;

    if (matched) {
      return;
    }

    setSelectedSkillName(skillCatalog[0].name);
  }, [selectedSkillName, skillCatalog]);

  useEffect(() => {
    if (skillSyncSourceId.trim() || skillSourceIndices.length === 0) {
      return;
    }
    const firstSource = skillSourceIndices[0]?.source;
    if (!firstSource) {
      return;
    }
    setSkillSyncSourceId(firstSource.source_id);
    setSkillSyncSourceKind(firstSource.source_kind);
    setSkillSyncLocator(firstSource.locator);
    setSkillSyncRevision(firstSource.revision ?? "");
  }, [skillSourceIndices, skillSyncLocator, skillSyncRevision, skillSyncSourceId]);

  useEffect(() => {
    if (!selectedHubSourceSnapshot) {
      return;
    }
    const current = remoteInstallSkillName.trim().toLowerCase();
    const exactMatch = selectedHubSourceSnapshot.entries.some(
      (entry) => entry.skill_name.trim().toLowerCase() === current,
    );
    if (!current || !exactMatch) {
      setRemoteInstallSkillName(selectedHubSourceSnapshot.entries[0]?.skill_name ?? "");
    }
  }, [remoteInstallSkillName, selectedHubSourceSnapshot]);

  useEffect(() => {
    if (!selectedSkillName) {
      setSkillDetail(null);
      setSkillDetailLoading(false);
      setSkillEditorContent("");
      return;
    }

    let cancelled = false;
    setSkillDetailLoading(true);

    void (async () => {
      try {
        const detailPath = selectedSessionId
          ? `/skill/detail?name=${encodeURIComponent(selectedSkillName)}&session_id=${encodeURIComponent(selectedSessionId)}`
          : `/skill/detail?name=${encodeURIComponent(selectedSkillName)}`;
        const detail = await apiJson<SkillDetailResponse>(
          detailPath,
        );
        if (cancelled) return;
        setSkillDetail(detail);
        setSkillEditorContent(detail.source ?? "");
      } catch (error) {
        if (cancelled) return;
        const message = `Failed to load skill ${selectedSkillName}: ${formatError(error)}`;
        setSkillDetail(null);
        setSkillEditorContent("");
        setFeedback(message);
        onBanner(message);
      } finally {
        if (!cancelled) {
          setSkillDetailLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [apiJson, onBanner, selectedSessionId, selectedSkillName]);

  const runMutation = useCallback(
    async (key: string, action: () => Promise<string | void>, success: string) => {
      setBusyKey(key);
      setFeedback(null);
      try {
        const actionSuccess = await action();
        await Promise.all([reloadSettingsData(), onReloadCoreData()]);
        setFeedback(actionSuccess ?? success);
      } catch (error) {
        const message = formatError(error);
        setFeedback(message);
        onBanner(message);
      } finally {
        setBusyKey(null);
      }
    },
    [onBanner, onReloadCoreData, reloadSettingsData],
  );

  const saveScheduler = async () => {
    await runMutation(
      "scheduler:save",
      async () => {
        const response = await apiJson<SchedulerConfigResponse>("/config/scheduler", {
          method: "PUT",
          body: JSON.stringify({
            path: schedulerPathDraft.trim() || undefined,
            content: schedulerContentDraft,
          }),
        });
        setSchedulerConfig(response);
        setSchedulerPathDraft(response.raw_path ?? "");
        setSchedulerContentDraft(response.content ?? "");
      },
      "Scheduler config saved.",
    );
  };

  const saveMcpConfig = async (key: string, raw: string) => {
    await runMutation(
      `mcp:save:${key}`,
      async () => {
        await api(`/config/mcp/${encodeURIComponent(key)}`, {
          method: "PUT",
          body: JSON.stringify(parseObjectJson(`MCP ${key}`, raw)),
        });
      },
      `Saved MCP config ${key}.`,
    );
  };

  const deleteMcpConfig = async (key: string) => {
    await runMutation(
      `mcp:delete:${key}`,
      async () => {
        await api(`/config/mcp/${encodeURIComponent(key)}`, { method: "DELETE" });
      },
      `Removed MCP config ${key}.`,
    );
  };

  const savePluginConfig = async (key: string, raw: string) => {
    await runMutation(
      `plugin:save:${key}`,
      async () => {
        await api(`/config/plugin/${encodeURIComponent(key)}`, {
          method: "PUT",
          body: JSON.stringify(parseObjectJson(`Plugin ${key}`, raw)),
        });
      },
      `Saved plugin config ${key}.`,
    );
  };

  const deletePluginConfig = async (key: string) => {
    await runMutation(
      `plugin:delete:${key}`,
      async () => {
        await api(`/config/plugin/${encodeURIComponent(key)}`, { method: "DELETE" });
      },
      `Removed plugin config ${key}.`,
    );
  };

  const removeProvider = async (providerId: string) => {
    await runMutation(
      `provider:delete:${providerId}`,
      async () => {
        await api(`/provider/${encodeURIComponent(providerId)}`, { method: "DELETE" });
      },
      `Removed provider ${providerId}.`,
    );
  };

  const refreshProviderCatalogue = async () => {
    setBusyKey("provider:refresh");
    setFeedback(null);
    try {
      const response = await apiJson<RefreshProviderCatalogueResponse>("/provider/refresh", {
        method: "POST",
      });
      await Promise.all([reloadSettingsData(), onReloadCoreData()]);
      const message =
        response.status === "updated"
          ? `Provider catalogue refreshed (generation ${response.generation_before} -> ${response.generation_after}).`
          : response.status === "not_modified"
            ? `Provider catalogue checked; no changes (generation ${response.generation_after}).`
            : `Provider catalogue refresh failed; using cached snapshot: ${response.error_message ?? "Unknown refresh failure"}`;
      setFeedback(message);
      if (response.status === "fallback_cached") {
        onBanner(message);
      }
    } catch (error) {
      const message = formatError(error);
      setFeedback(message);
      onBanner(message);
    } finally {
      setBusyKey(null);
    }
  };

  const runMcpAction = async (name: string, action: "connect" | "disconnect" | "restart") => {
    await runMutation(
      `mcp:${action}:${name}`,
      async () => {
        await api(`/mcp/${encodeURIComponent(name)}/${action}`, { method: "POST" });
      },
      `MCP ${name} ${action} complete.`,
    );
  };

  const buildSkillSyncSource = useCallback((): SkillSourceRefLike => {
    if (!skillSyncSourceId.trim()) {
      throw new Error("Skill hub source_id is required.");
    }
    if (!skillSyncLocator.trim()) {
      throw new Error("Skill hub locator is required.");
    }
    return {
      source_id: skillSyncSourceId.trim(),
      source_kind: skillSyncSourceKind,
      locator: skillSyncLocator.trim(),
      revision: skillSyncRevision.trim() || undefined,
    };
  }, [skillSyncLocator, skillSyncRevision, skillSyncSourceId, skillSyncSourceKind]);

  const planSkillSync = async () => {
    const source = buildSkillSyncSource();
    setBusyKey(`skill:sync:plan:${source.source_id}`);
    setFeedback(null);
    try {
      const response = await apiJson<SkillHubSyncPlanResponseLike>("/skill/hub/sync/plan", {
        method: "POST",
        body: JSON.stringify({ source }),
      });
      setSkillSyncPlan(response.plan);
      await reloadSettingsData();
      setFeedback(`Built skill sync plan for ${source.source_id}.`);
    } catch (error) {
      const message = formatError(error);
      setFeedback(message);
      onBanner(message);
    } finally {
      setBusyKey(null);
    }
  };

  const refreshSkillSourceIndex = async () => {
    const source = buildSkillSyncSource();
    setBusyKey(`skill:index:refresh:${source.source_id}`);
    setFeedback(null);
    try {
      const response = await apiJson<SkillHubIndexRefreshResponseLike>("/skill/hub/index/refresh", {
        method: "POST",
        body: JSON.stringify({ source }),
      });
      await reloadSettingsData();
      setFeedback(
        `Refreshed source index for ${response.snapshot.source.source_id} (${response.snapshot.entries.length} entries).`,
      );
    } catch (error) {
      const message = formatError(error);
      setFeedback(message);
      onBanner(message);
    } finally {
      setBusyKey(null);
    }
  };

  const runGuard = async (request: SkillHubGuardRunRequestLike, targetLabel: string) => {
    await runMutation(
      `skill:guard:${targetLabel}`,
      async () => {
        const response = await apiJson<SkillHubGuardRunResponseLike>("/skill/hub/guard/run", {
          method: "POST",
          body: JSON.stringify(request),
        });
        setSkillGuardTarget(targetLabel);
        setSkillGuardReports(response.reports);
        const violationCount = response.reports.reduce(
          (total, report) => total + report.violations.length,
          0,
        );
        return `Guard scanned ${targetLabel} (${response.reports.length} report${response.reports.length === 1 ? "" : "s"}, ${violationCount} total violations).`;
      },
      `Guard scanned ${targetLabel}.`,
    );
  };

  const applySkillSync = async () => {
    if (!selectedSessionId) return;
    const source = buildSkillSyncSource();
    await runMutation(
      `skill:sync:apply:${source.source_id}`,
      async () => {
        const response = await apiJson<SkillHubSyncPlanResponseLike>("/skill/hub/sync/apply", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            source,
          }),
        });
        setSkillSyncPlan(response.plan);
        if ((response.guard_reports?.length ?? 0) > 0) {
          return `Applied skill sync for ${source.source_id} with ${response.guard_reports?.length ?? 0} guard warnings.`;
        }
      },
      `Applied skill sync for ${source.source_id}.`,
    );
  };

  const planRemoteInstall = async () => {
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    setBusyKey(`skill:install:plan:${source.source_id}:${skillName}`);
    setFeedback(null);
    try {
      const response = await apiJson<SkillRemoteInstallPlanLike>("/skill/hub/install/plan", {
        method: "POST",
        body: JSON.stringify({
          source,
          skill_name: skillName,
        }),
      });
      setRemoteInstallPlan(response);
      await reloadSettingsData();
      setFeedback(
        `Built remote install plan for ${response.entry.skill_name} from ${source.source_id} (${response.entry.action}).`,
      );
    } catch (error) {
      const message = formatError(error);
      setFeedback(message);
      onBanner(message);
    } finally {
      setBusyKey(null);
    }
  };

  const applyRemoteInstall = async () => {
    if (!selectedSessionId) return;
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    await runMutation(
      `skill:install:apply:${source.source_id}:${skillName}`,
      async () => {
        const response = await apiJson<SkillRemoteInstallResponseLike>("/skill/hub/install/apply", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            source,
            skill_name: skillName,
          }),
        });
        setRemoteInstallPlan(response.plan);
        const violationCount = response.guard_report?.violations.length ?? 0;
        if (violationCount > 0) {
          return `Applied remote ${response.plan.entry.action} for ${response.result.skill_name} with ${violationCount} guard warnings.`;
        }
        return `Applied remote ${response.plan.entry.action} for ${response.result.skill_name}.`;
      },
      `Applied remote install for ${skillName}.`,
    );
  };

  const planRemoteUpdate = async () => {
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    setBusyKey(`skill:update:plan:${source.source_id}:${skillName}`);
    setFeedback(null);
    try {
      const response = await apiJson<SkillRemoteInstallPlanLike>("/skill/hub/update/plan", {
        method: "POST",
        body: JSON.stringify({
          source,
          skill_name: skillName,
        }),
      });
      setRemoteInstallPlan(response);
      await reloadSettingsData();
      setFeedback(
        `Built remote update plan for ${response.entry.skill_name} from ${source.source_id} (${response.entry.action}).`,
      );
    } catch (error) {
      const message = formatError(error);
      setFeedback(message);
      onBanner(message);
    } finally {
      setBusyKey(null);
    }
  };

  const applyRemoteUpdate = async () => {
    if (!selectedSessionId) return;
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    await runMutation(
      `skill:update:apply:${source.source_id}:${skillName}`,
      async () => {
        const response = await apiJson<SkillRemoteInstallResponseLike>("/skill/hub/update/apply", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            source,
            skill_name: skillName,
          }),
        });
        setRemoteInstallPlan(response.plan);
        const violationCount = response.guard_report?.violations.length ?? 0;
        if (violationCount > 0) {
          return `Applied remote ${response.plan.entry.action} for ${response.result.skill_name} with ${violationCount} guard warnings.`;
        }
        return `Applied remote ${response.plan.entry.action} for ${response.result.skill_name}.`;
      },
      `Applied remote update for ${skillName}.`,
    );
  };

  const detachManagedSkill = async () => {
    if (!selectedSessionId) return;
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    await runMutation(
      `skill:detach:${source.source_id}:${skillName}`,
      async () => {
        const response = await apiJson<SkillHubManagedDetachResponseLike>("/skill/hub/detach", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            source,
            skill_name: skillName,
          }),
        });
        return `Detached managed skill ${response.lifecycle.skill_name}; workspace content was preserved.`;
      },
      `Detached managed skill ${skillName}.`,
    );
  };

  const removeManagedSkill = async () => {
    if (!selectedSessionId) return;
    const source = buildSkillSyncSource();
    const skillName = remoteInstallSkillName.trim();
    if (!skillName) {
      throw new Error("Select or type a remote skill name first.");
    }
    await runMutation(
      `skill:remove:${source.source_id}:${skillName}`,
      async () => {
        const response = await apiJson<SkillHubManagedRemoveResponseLike>("/skill/hub/remove", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            source,
            skill_name: skillName,
          }),
        });
        if (response.deleted_from_workspace) {
          return `Removed managed skill ${response.lifecycle.skill_name} and deleted the clean workspace copy.`;
        }
        return `Removed managed skill ${response.lifecycle.skill_name} without deleting the workspace copy.`;
      },
      `Removed managed skill ${skillName}.`,
    );
  };

  const createSkill = async () => {
    if (!selectedSessionId) return;
    await runMutation(
      `skill:create:${newSkillName.trim() || "new"}`,
      async () => {
        const response = await apiJson<SkillManageResponseLike>("/skill/manage", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            action: "create",
            name: newSkillName,
            description: newSkillDescription,
            category: newSkillCategory.trim() || undefined,
            body: newSkillBody,
          }),
        });
        setSelectedSkillName(response.result.skill_name);
        setNewSkillName("");
        setNewSkillDescription("");
        setNewSkillCategory("");
        setNewSkillBody("");
        if (response.guard_report) {
          return `Created skill ${response.result.skill_name} with ${response.guard_report.violations.length} guard warnings.`;
        }
      },
      `Created skill ${newSkillName.trim()}.`,
    );
  };

  const saveSelectedSkill = async () => {
    if (!selectedSessionId || !selectedSkillName) return;
    await runMutation(
      `skill:edit:${selectedSkillName}`,
      async () => {
        const response = await apiJson<SkillManageResponseLike>("/skill/manage", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            action: "edit",
            name: selectedSkillName,
            content: skillEditorContent,
          }),
        });
        if (response.guard_report) {
          return `Saved skill ${selectedSkillName} with ${response.guard_report.violations.length} guard warnings.`;
        }
      },
      `Saved skill ${selectedSkillName}.`,
    );
  };

  const deleteSelectedSkill = async () => {
    if (!selectedSessionId || !selectedSkillName) return;
    const deletedSkillName = selectedSkillName;
    await runMutation(
      `skill:delete:${deletedSkillName}`,
      async () => {
        await apiJson<SkillManageResponseLike>("/skill/manage", {
          method: "POST",
          body: JSON.stringify({
            session_id: selectedSessionId,
            action: "delete",
            name: deletedSkillName,
          }),
        });
        setSelectedSkillName(null);
      },
      `Deleted skill ${deletedSkillName}.`,
    );
  };

  const runSelectedSkillGuard = async () => {
    if (!selectedSkillName) return;
    await runGuard({ skill_name: selectedSkillName }, `skill ${selectedSkillName}`);
  };

  const runSelectedSourceGuard = async () => {
    const source = buildSkillSyncSource();
    await runGuard({ source }, `source ${source.source_id}`);
  };

  const providerSummary = `${providers.length} connected / ${knownProviders.length} known`;
  const chooseKnownProvider = (provider: KnownProviderEntryLike) => {
    onConnectQueryChange(provider.id);
  };
  const secondaryButtonClass =
    "roc-action min-h-[36px] px-4 text-foreground text-sm cursor-pointer transition-colors";
  const primaryButtonClass =
    "min-h-[36px] rounded-lg px-5 border border-foreground bg-foreground text-background text-sm font-semibold inline-flex items-center justify-center cursor-pointer transition-colors disabled:cursor-not-allowed disabled:opacity-60";
  const summaryCardClass = "rounded-xl border border-border/40 bg-card/72 p-4 grid gap-2";
  const sectionCardClass = "grid gap-4 rounded-xl border border-border/40 bg-card/65 p-5";
  const mutedCardClass = "rounded-xl border border-border/35 bg-muted/10 px-4 py-3 text-sm leading-relaxed text-muted-foreground";
  const editorTextareaClass =
    "min-h-40 w-full resize-y rounded-xl border border-border/45 bg-background/78 p-3.5 text-foreground leading-relaxed font-mono text-sm";

  return (
    <div className="fixed inset-0 z-50 bg-black/40 backdrop-blur-sm flex items-start justify-end" data-testid="settings-overlay" onClick={onClose}>
      <section
        className="h-full w-full max-w-3xl bg-card border-l border-border/50 overflow-y-auto p-8 flex flex-col gap-6"
        data-testid="settings-drawer"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-4">
          <div>
            <p className="m-0 mb-1.5 text-xs tracking-widest uppercase text-amber-700 font-bold">Settings</p>
            <h2>General, providers, scheduler, skills, MCP, plugins, LSP</h2>
          </div>
          <div className="flex items-center gap-2">
            <button
              className={secondaryButtonClass}
              type="button"
              data-testid="settings-refresh"
              onClick={() => void reloadSettingsData()}
            >
              {refreshing ? "Refreshing..." : "Refresh"}
            </button>
            <button className={secondaryButtonClass} type="button" data-testid="settings-close" onClick={onClose}>
              Close
            </button>
          </div>
        </header>

        <div className="flex flex-wrap gap-2 border-b border-border pb-4">
          {SETTINGS_TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              data-testid={`settings-tab-${tab.id}`}
              className={
                activeTab === tab.id
                  ? "px-4 py-2 rounded-lg border border-foreground/10 cursor-pointer text-sm bg-foreground text-background font-semibold"
                  : "px-4 py-2 rounded-lg border border-transparent cursor-pointer text-sm bg-transparent text-foreground hover:bg-accent"
              }
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {feedback ? <div className="rounded-xl border border-amber-300 bg-amber-50/80 px-5 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200">{feedback}</div> : null}

        <div className="flex flex-col gap-6 flex-1 min-h-0">
          {loading ? <div className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">Loading settings...</div> : null}
          {!loading && isolatedNotice ? (
            <div className="rounded-xl border border-amber-300 bg-amber-50/80 px-5 py-3 text-sm leading-relaxed text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200">
              {isolatedNotice}
            </div>
          ) : null}

          {!loading && activeTab === "general" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <label>Theme</label>
                <div className="flex flex-wrap gap-2">
                  {themes.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={
                        theme === item.id
                          ? "px-4 py-2 rounded-lg border border-foreground/10 cursor-pointer text-sm bg-foreground text-background font-semibold"
                          : "px-4 py-2 rounded-lg border border-transparent cursor-pointer text-sm bg-transparent text-foreground hover:bg-accent"
                      }
                      onClick={() => onThemeChange(item.id)}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="grid gap-3">
                <label htmlFor="settings-mode-select">Execution Mode</label>
                <select
                  id="settings-mode-select"
                  value={selectedMode}
                  onChange={(event) => onModeChange(event.target.value)}
                >
                  <option value="">auto</option>
                  {modeOptions.map((mode) => (
                    <option key={mode.key} value={mode.key}>
                      {mode.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className="grid gap-3">
                <label htmlFor="settings-model-select">Model</label>
                <select
                  id="settings-model-select"
                  value={selectedModel}
                  onChange={(event) => onModelChange(event.target.value)}
                >
                  <option value="">auto</option>
                  {modelOptions.map((model) => (
                    <option key={model.key} value={model.key}>
                      {model.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className="grid gap-3">
                <label className="flex items-center gap-3 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={showThinking}
                    onChange={(event) => onShowThinkingChange(event.target.checked)}
                  />
                  Show reasoning blocks
                </label>
              </div>

              <div className="grid gap-3">
                <label>Workspace Authority</label>
                <div className="grid gap-3 sm:grid-cols-2">
                  <div className={summaryCardClass}>
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace Mode</span>
                    <strong>{workspaceMode === "isolated" ? "isolated sandbox" : "shared workspace"}</strong>
                  </div>
                  <div className={summaryCardClass}>
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace Root</span>
                    <strong className="break-all text-sm">{workspaceRootPath || "--"}</strong>
                  </div>
                </div>
                {workspaceConfigDir ? (
                  <p className="m-0 text-xs leading-relaxed text-muted-foreground">
                    Isolated config dir: <code>{workspaceConfigDir}</code>
                  </p>
                ) : null}
                <div
                  className={cn(
                    "rounded-xl border px-4 py-3 text-sm leading-relaxed",
                    workspaceMode === "isolated"
                      ? "border-amber-300 bg-amber-50/80 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                      : "border-border/35 bg-muted/10 text-muted-foreground",
                  )}
                >
                  {workspaceMode === "isolated"
                    ? "This workspace runs as an isolated sandbox. It will not inherit global config, managed home config, or shared workspace overrides outside this .rocode root."
                    : "This workspace runs in shared mode. Global config can still participate in the resolved runtime context alongside workspace-local settings."}
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <div className={summaryCardClass}>
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Providers</span>
                  <strong>{providerSummary}</strong>
                </div>
                <div className={summaryCardClass}>
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Scheduler Path</span>
                  <strong>{schedulerConfig?.raw_path || configSnapshot?.schedulerPath || "--"}</strong>
                </div>
                <div className={summaryCardClass}>
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">MCP Servers</span>
                  <strong>{Object.keys(mcpConfigs).length}</strong>
                </div>
                <div className={summaryCardClass}>
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Plugins</span>
                  <strong>{Object.keys(pluginConfigs).length}</strong>
                </div>
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "providers" ? (
            <div className="grid gap-6">
              <form
                className="grid gap-3"
                onSubmit={(event) => {
                  event.preventDefault();
                  void (async () => {
                    await onConnectProvider();
                    await reloadSettingsData();
                  })();
                }}
              >
                <label htmlFor="settings-provider-connect-query">Connect Provider</label>
                <input
                  id="settings-provider-connect-query"
                  type="text"
                  placeholder="Search provider or enter custom id"
                  value={connectQuery}
                  onChange={(event) => onConnectQueryChange(event.target.value)}
                />
                {connectQuery.trim() ? (
                  <div className="grid gap-2 rounded-lg border border-border/35 bg-muted/10 p-3">
                    {connectResolveBusy ? (
                      <p className="m-0 text-sm text-muted-foreground">
                        Resolving provider defaults...
                      </p>
                    ) : connectResolveError ? (
                      <p className="m-0 text-sm text-red-600 dark:text-red-300">
                        Failed to resolve provider defaults: {connectResolveError}
                      </p>
                    ) : connectMatches.length > 0 ? (
                      connectMatches.map((provider) => (
                        <button
                          key={provider.id}
                          type="button"
                          className="rounded-lg border border-transparent bg-transparent px-3 py-2 text-left text-sm transition-colors hover:bg-accent"
                          onClick={() => chooseKnownProvider(provider)}
                        >
                          <strong>{provider.name}</strong>
                          <span className="block text-muted-foreground">
                            {provider.id}
                            {provider.protocol ? ` · ${provider.protocol}` : ""}
                            {provider.base_url ? ` · ${provider.base_url}` : ""}
                          </span>
                        </button>
                      ))
                    ) : (
                      <p className="m-0 text-sm text-muted-foreground">
                        No known provider match. This query will fall back to custom provider id{" "}
                        <code>{connectResolution?.custom_draft.provider_id || connectQuery.trim()}</code>.
                      </p>
                    )}
                  </div>
                ) : null}
                <input
                  type="text"
                  placeholder="Provider id"
                  value={connectProviderId}
                  onChange={(event) => onConnectProviderIdChange(event.target.value)}
                />
                {exactKnownProvider ? (
                  <p className="m-0 text-xs text-muted-foreground">
                    Known provider match.
                    {exactKnownProvider.env?.length
                      ? ` Expected env: ${exactKnownProvider.env.join(", ")}.`
                      : ""}
                    {exactKnownProvider.model_count
                      ? ` ${exactKnownProvider.model_count} models in catalogue.`
                      : ""}
                  </p>
                ) : connectResolution?.draft.mode === "custom" ? (
                  <p className="m-0 text-xs text-muted-foreground">
                    No known provider matched. You can keep the suggested custom draft or edit the
                    provider id, base URL and protocol below.
                  </p>
                ) : null}
                <input
                  type="password"
                  placeholder="API key"
                  value={connectApiKey}
                  onChange={(event) => onConnectApiKeyChange(event.target.value)}
                />
                <input
                  type="url"
                  placeholder="Custom base URL (optional)"
                  value={connectBaseUrl}
                  onChange={(event) => onConnectBaseUrlChange(event.target.value)}
                />
                <select
                  value={connectProtocol}
                  onChange={(event) => onConnectProtocolChange(event.target.value)}
                >
                  {connectProtocols.map((protocol) => (
                    <option key={protocol.id} value={protocol.id}>
                      {protocol.name}
                    </option>
                  ))}
                </select>
                <button className="min-h-[36px] rounded-full px-5 bg-foreground border-foreground text-background text-sm font-semibold inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px" type="submit" disabled={connectBusy}>
                  {connectBusy ? "Connecting..." : "Connect"}
                </button>
              </form>

              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Configured Providers</p>
                  <div className="flex items-center gap-2">
                    <span>{providerSummary}</span>
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === "provider:refresh"}
                      onClick={() => void refreshProviderCatalogue()}
                    >
                      {busyKey === "provider:refresh" ? "Refreshing..." : "Refresh Catalogue"}
                    </button>
                  </div>
                </div>
                {providers.map((provider) => (
                  <div key={provider.id} className="rounded-xl border border-border bg-card/70 p-4 flex items-start justify-between gap-4">
                    <div>
                      <strong>{provider.name}</strong>
                      <p className="text-sm text-muted-foreground leading-relaxed">
                        {provider.id} · {(provider.models ?? []).length} models
                      </p>
                    </div>
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === `provider:delete:${provider.id}`}
                      onClick={() => void removeProvider(provider.id)}
                    >
                      Remove
                    </button>
                  </div>
                ))}
              </div>

              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Managed Providers</p>
                  <span>{managedProviders.length} items</span>
                </div>
                {managedProviders.map((provider) => (
                  <div key={provider.id} className="rounded-xl border border-border bg-card/70 p-4 flex items-start justify-between gap-4">
                    <div>
                      <strong>{provider.name}</strong>
                      <p className="text-sm text-muted-foreground leading-relaxed">
                        {provider.id}
                        {provider.base_url ? ` · ${provider.base_url}` : ""}
                        {provider.protocol ? ` · ${provider.protocol}` : ""}
                      </p>
                      <p className="text-sm text-muted-foreground leading-relaxed">
                        status {provider.status}
                        {provider.auth_type ? ` · auth ${provider.auth_type}` : ""}
                        {provider.env?.length ? ` · env ${provider.env.join(", ")}` : ""}
                      </p>
                    </div>
                    <span className={cn("rounded-full border px-3 py-1.5 text-xs font-semibold", statusTone(provider.status) === "ok" ? "border-green-300 bg-green-50 text-green-800 dark:border-green-700 dark:bg-green-950 dark:text-green-300" : statusTone(provider.status) === "warn" ? "border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-300" : statusTone(provider.status) === "danger" ? "border-red-300 bg-red-50 text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-300" : "border-border bg-muted text-muted-foreground")}>
                      {provider.status}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "scheduler" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <label htmlFor="settings-scheduler-path">Scheduler Config Path</label>
                <input
                  id="settings-scheduler-path"
                  type="text"
                  value={schedulerPathDraft}
                  onChange={(event) => setSchedulerPathDraft(event.target.value)}
                />
                <div className="text-sm text-muted-foreground leading-relaxed">
                  resolved {schedulerConfig?.resolved_path || "--"} · {schedulerConfig?.exists ? "exists" : "new file"}
                </div>
              </div>

              <div className="grid gap-3 col-span-full">
                <label htmlFor="settings-scheduler-content">Scheduler Config</label>
                <textarea
                  id="settings-scheduler-content"
                  className={editorTextareaClass}
                  value={schedulerContentDraft}
                  onChange={(event) => setSchedulerContentDraft(event.target.value)}
                  spellCheck={false}
                />
                <div className="flex items-center gap-2">
                  <button
                    className="min-h-[36px] rounded-full px-5 bg-foreground border-foreground text-background text-sm font-semibold inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px"
                    type="button"
                    disabled={busyKey === "scheduler:save"}
                    onClick={() => void saveScheduler()}
                  >
                    {busyKey === "scheduler:save" ? "Saving..." : "Save Scheduler Config"}
                  </button>
                </div>
                {schedulerConfig?.parse_error ? (
                  <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-3 text-sm text-destructive">{schedulerConfig.parse_error}</div>
                ) : null}
              </div>

              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Profiles</p>
                  <span>default {schedulerConfig?.default_profile || "--"}</span>
                </div>
                {schedulerConfig?.profiles.length ? (
                  schedulerConfig.profiles.map((profile) => (
                    <div key={profile.key} className="rounded-xl border border-border bg-card/70 p-4 flex items-start justify-between gap-4">
                      <div>
                        <strong>{profile.key}</strong>
                        <p className="text-sm text-muted-foreground leading-relaxed">
                          {profile.orchestrator || "no orchestrator"}
                          {profile.description ? ` · ${profile.description}` : ""}
                        </p>
                        <p className="text-sm text-muted-foreground leading-relaxed">
                          {profile.stages.length ? profile.stages.join(" -> ") : "no stages"}
                        </p>
                      </div>
                    </div>
                  ))
                ) : (
                  <p className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">No scheduler profiles parsed yet.</p>
                )}
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "skills" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <label>Workspace Skill Authority</label>
                <div className="grid gap-3 sm:grid-cols-3">
                  <div className={summaryCardClass}>
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace Root</span>
                    <strong className="break-all text-sm">{workspaceRootPath || "--"}</strong>
                  </div>
                  <div className={summaryCardClass}>
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Writable Skill Root</span>
                    <strong className="break-all text-sm">{skillWorkspaceRoot}</strong>
                  </div>
                  <div className={summaryCardClass}>
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Discovered Skills</span>
                    <strong>{skillCatalog.length}</strong>
                  </div>
                </div>
                <div className={mutedCardClass}>
                  Writes from this panel go through <code>/skill/manage</code> and land only in the
                  current workspace authority at <code>{skillWorkspaceRoot}</code>. Global config and
                  external skill roots stay read-only here.
                </div>
                {selectedSessionId ? (
                  <div className={mutedCardClass}>
                    Catalog reads now go through <code>/skill/catalog?session_id=...</code>, so the
                    visible skill set follows the active session's scheduler stage when a stage is
                    currently constraining tools. Detail preview now uses the same session-aware
                    scope through <code>/skill/detail?session_id=...</code>.
                  </div>
                ) : null}
                {!skillsMutationsEnabled ? (
                  <div className="rounded-xl border border-amber-300 bg-amber-50/80 px-4 py-3 text-sm leading-relaxed text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200">
                    Select or create a session before managing skills so permission prompts can be
                    routed to the active session.
                  </div>
                ) : null}
              </div>

              <div className={sectionCardClass}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <p className="m-0 text-xs tracking-widest uppercase text-muted-foreground font-semibold">Skill Hub / Sync</p>
                    <h3 className="m-0 mt-1">Managed provenance and authority sync</h3>
                  </div>
                  <div className="text-sm text-muted-foreground">
                    managed {managedSkills.length} · sources {skillSourceIndices.length} · distributions {skillDistributions.length} · artifacts {skillArtifactCache.length} · lifecycle {skillLifecycle.length}
                  </div>
                </div>

                <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                  <input
                    type="text"
                    placeholder="source_id"
                    value={skillSyncSourceId}
                    onChange={(event) => setSkillSyncSourceId(event.target.value)}
                  />
                  <select
                    value={skillSyncSourceKind}
                    onChange={(event) => setSkillSyncSourceKind(event.target.value as SkillSourceRefLike["source_kind"])}
                  >
                    <option value="local_path">local_path</option>
                    <option value="bundled">bundled</option>
                    <option value="git">git</option>
                    <option value="archive">archive</option>
                    <option value="registry">registry</option>
                  </select>
                  <input
                    type="text"
                    placeholder="locator"
                    value={skillSyncLocator}
                    onChange={(event) => setSkillSyncLocator(event.target.value)}
                  />
                  <input
                    type="text"
                    placeholder="revision (optional)"
                    value={skillSyncRevision}
                    onChange={(event) => setSkillSyncRevision(event.target.value)}
                  />
                </div>

                <div className="flex flex-wrap items-center gap-2">
                  <button
                    className={primaryButtonClass}
                    type="button"
                    disabled={!skillSyncSourceId.trim() || !skillSyncLocator.trim() || busyKey === `skill:sync:plan:${skillSyncSourceId.trim()}`}
                    onClick={() => void planSkillSync()}
                  >
                    {busyKey === `skill:sync:plan:${skillSyncSourceId.trim()}` ? "Planning..." : "Preview Sync Plan"}
                  </button>
                  <button
                    className={secondaryButtonClass}
                    type="button"
                    disabled={
                      !skillsMutationsEnabled ||
                      !skillSyncSourceId.trim() ||
                      !skillSyncLocator.trim() ||
                      busyKey === `skill:sync:apply:${skillSyncSourceId.trim()}`
                    }
                    onClick={() => void applySkillSync()}
                  >
                    {busyKey === `skill:sync:apply:${skillSyncSourceId.trim()}` ? "Applying..." : "Apply Sync"}
                  </button>
                  <button
                    className={secondaryButtonClass}
                    type="button"
                    disabled={!skillSyncSourceId.trim() || !skillSyncLocator.trim() || busyKey === `skill:index:refresh:${skillSyncSourceId.trim()}`}
                    onClick={() => void refreshSkillSourceIndex()}
                  >
                    {busyKey === `skill:index:refresh:${skillSyncSourceId.trim()}` ? "Refreshing Index..." : "Refresh Source Index"}
                  </button>
                  <button
                    className={secondaryButtonClass}
                    type="button"
                    disabled={!skillSyncSourceId.trim() || !skillSyncLocator.trim() || busyKey === `skill:guard:source ${skillSyncSourceId.trim()}`}
                    onClick={() => void runSelectedSourceGuard()}
                  >
                    {busyKey === `skill:guard:source ${skillSyncSourceId.trim()}` ? "Scanning..." : "Run Source Guard"}
                  </button>
                </div>

                <div className="grid gap-4 xl:grid-cols-2">
                  <div className="grid gap-3">
                    <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Managed Skills</div>
                    {managedSkills.length ? managedSkills.slice(0, 8).map((record) => (
                      <div key={record.skill_name} className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                        <div className="flex items-start justify-between gap-3">
                          <strong>{record.skill_name}</strong>
                          <span className="text-muted-foreground">{record.installed_revision || "--"}</span>
                        </div>
                        <div className="mt-2 text-muted-foreground">
                          {(record.source?.source_id ?? "unmanaged")} · {record.locally_modified ? "locally modified" : record.deleted_locally ? "deleted locally" : "clean"}
                        </div>
                      </div>
                    )) : (
                      <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">No managed records yet.</div>
                    )}
                  </div>

                  <div className="grid gap-3">
                    <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Indexed Sources</div>
                    {skillSourceIndices.length ? skillSourceIndices.slice(0, 6).map((snapshot) => (
                      <button
                        key={snapshot.source.source_id}
                        type="button"
                        className="rounded-lg border border-transparent bg-transparent p-3 text-left transition-colors hover:bg-accent"
                        onClick={() => {
                          setSkillSyncSourceId(snapshot.source.source_id);
                          setSkillSyncSourceKind(snapshot.source.source_kind);
                          setSkillSyncLocator(snapshot.source.locator);
                          setSkillSyncRevision(snapshot.source.revision ?? "");
                          setRemoteInstallSkillName(snapshot.entries[0]?.skill_name ?? "");
                        }}
                      >
                        <strong>{snapshot.source.source_id}</strong>
                        <div className="mt-2 text-sm text-muted-foreground">
                          {snapshot.source.source_kind} · {snapshot.entries.length} skills
                        </div>
                        <div className="mt-1 break-all text-xs text-muted-foreground">{snapshot.source.locator}</div>
                      </button>
                    )) : (
                      <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">No source index cached yet.</div>
                    )}
                  </div>
                </div>

                <div className="grid gap-4 rounded-xl border border-border/35 bg-muted/8 p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div>
                      <p className="m-0 text-xs tracking-widest uppercase text-muted-foreground font-semibold">Remote Install</p>
                      <h4 className="m-0 mt-1">Remote distribution plan and apply</h4>
                    </div>
                    <div className="text-sm text-muted-foreground">
                      source {selectedHubSourceSnapshot?.source.source_id ?? "--"}
                    </div>
                  </div>

                  <div className="grid gap-3 sm:grid-cols-[minmax(0,1.2fr)_repeat(2,minmax(0,1fr))] xl:grid-cols-[minmax(0,1.4fr)_repeat(4,minmax(0,0.8fr))]">
                    <input
                      type="text"
                      placeholder="remote skill name"
                      value={remoteInstallSkillName}
                      onChange={(event) => setRemoteInstallSkillName(event.target.value)}
                    />
                    <button
                      className={primaryButtonClass}
                      type="button"
                      disabled={
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:install:plan:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void planRemoteInstall()}
                    >
                      {busyKey === `skill:install:plan:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Planning..."
                        : "Preview Install"}
                    </button>
                    <button
                      className={secondaryButtonClass}
                      type="button"
                      disabled={
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:update:plan:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void planRemoteUpdate()}
                    >
                      {busyKey === `skill:update:plan:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Planning..."
                        : "Preview Update"}
                    </button>
                    <button
                      className={secondaryButtonClass}
                      type="button"
                      disabled={
                        !skillsMutationsEnabled ||
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:install:apply:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void applyRemoteInstall()}
                    >
                      {busyKey === `skill:install:apply:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Installing..."
                        : "Install To Workspace"}
                    </button>
                    <button
                      className={secondaryButtonClass}
                      type="button"
                      disabled={
                        !skillsMutationsEnabled ||
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:update:apply:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void applyRemoteUpdate()}
                    >
                      {busyKey === `skill:update:apply:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Updating..."
                        : "Update Workspace"}
                    </button>
                    <button
                      className="min-h-[36px] rounded-lg px-4 border border-amber-300 bg-amber-50/80 text-amber-950 text-sm inline-flex items-center justify-center cursor-pointer transition-colors disabled:cursor-not-allowed disabled:opacity-60 dark:border-amber-700 dark:bg-amber-950/40 dark:text-amber-200 dark:hover:bg-amber-950/60"
                      type="button"
                      disabled={
                        !skillsMutationsEnabled ||
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:detach:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void detachManagedSkill()}
                    >
                      {busyKey === `skill:detach:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Detaching..."
                        : "Detach Managed"}
                    </button>
                    <button
                      className="min-h-[36px] rounded-lg px-4 border border-red-300 bg-red-50/80 text-red-900 text-sm inline-flex items-center justify-center cursor-pointer transition-colors disabled:cursor-not-allowed disabled:opacity-60 dark:border-red-700 dark:bg-red-950/40 dark:text-red-200 dark:hover:bg-red-950/60"
                      type="button"
                      disabled={
                        !skillsMutationsEnabled ||
                        !skillSyncSourceId.trim() ||
                        !skillSyncLocator.trim() ||
                        !remoteInstallSkillName.trim() ||
                        busyKey === `skill:remove:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                      }
                      onClick={() => void removeManagedSkill()}
                    >
                      {busyKey === `skill:remove:${skillSyncSourceId.trim()}:${remoteInstallSkillName.trim()}`
                        ? "Removing..."
                        : "Remove Managed"}
                    </button>
                  </div>

                  <div className="text-xs text-muted-foreground">
                    Update re-applies the selected managed source through the unified lifecycle pipeline. Detach drops managed ownership but keeps workspace files. Remove clears managed state and only deletes the workspace copy when the local skill is still clean.
                  </div>

                  {selectedRemoteSourceEntry ? (
                    <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                      <div className="flex items-start justify-between gap-3">
                        <strong>{selectedRemoteSourceEntry.skill_name}</strong>
                        <span className="text-muted-foreground">
                          {selectedRemoteSourceEntry.revision || "--"}
                        </span>
                      </div>
                      <div className="mt-2 text-muted-foreground">
                        {selectedRemoteSourceEntry.category
                          ? `${selectedRemoteSourceEntry.category} · `
                          : ""}
                        {selectedRemoteSourceEntry.description || "No remote description provided."}
                      </div>
                    </div>
                  ) : (
                    <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">
                      Type a skill name from the selected source index to preview or apply a remote install.
                    </div>
                  )}

                  {selectedHubSourceSnapshot?.entries.length ? (
                    <div className="grid gap-3">
                      <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">
                        Indexed Entries for Selected Source
                      </div>
                      <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
                        {selectedHubSourceSnapshot.entries.slice(0, 12).map((entry) => {
                          const selected =
                            entry.skill_name.trim().toLowerCase() ===
                            remoteInstallSkillName.trim().toLowerCase();
                          return (
                            <button
                              key={entry.skill_name}
                              type="button"
                              className={cn(
                                "rounded-lg border p-3 text-left transition-colors",
                                selected
                                  ? "border-border/70 bg-accent"
                                  : "border-transparent bg-transparent hover:bg-accent",
                              )}
                              onClick={() => setRemoteInstallSkillName(entry.skill_name)}
                            >
                              <strong>{entry.skill_name}</strong>
                              <div className="mt-1 text-xs text-muted-foreground">
                                {entry.category ? `${entry.category} · ` : ""}
                                {entry.revision || "unversioned"}
                              </div>
                              <div className="mt-2 text-sm text-muted-foreground">
                                {entry.description || "No description"}
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  ) : null}

                  <div className="grid gap-4 xl:grid-cols-4">
                    <div className="grid gap-3">
                      <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">
                        Hub Policy
                      </div>
                      <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                        {skillHubPolicy ? (
                          <div className="grid gap-2 text-muted-foreground">
                            <div>
                              retention {formatHubDurationSeconds(skillHubPolicy.artifact_cache_retention_seconds)}
                            </div>
                            <div>
                              timeout {formatHubDurationMs(skillHubPolicy.fetch_timeout_ms)}
                            </div>
                            <div>
                              max download {formatHubBytes(skillHubPolicy.max_download_bytes)}
                            </div>
                            <div>
                              max extract {formatHubBytes(skillHubPolicy.max_extract_bytes)}
                            </div>
                          </div>
                        ) : (
                        <div className="text-muted-foreground">
                          No hub policy payload loaded yet.
                        </div>
                        )}
                      </div>
                    </div>

                    <div className="grid gap-3">
                      <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">
                        Distribution Snapshot
                      </div>
                      {selectedRemoteDistribution ? (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                          <div className="flex items-center justify-between gap-3">
                            <strong>{selectedRemoteDistribution.skill_name}</strong>
                            <span
                              className={cn(
                                "rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide",
                                lifecycleStatusClass(selectedRemoteDistribution.lifecycle),
                              )}
                            >
                              {selectedRemoteDistribution.lifecycle}
                            </span>
                          </div>
                          <div className="mt-2 text-muted-foreground">
                            release {selectedRemoteDistribution.release.version || "--"} · revision{" "}
                            {selectedRemoteDistribution.release.revision || "--"}
                          </div>
                          <div className="mt-1 break-all text-xs text-muted-foreground">
                            artifact {selectedRemoteDistribution.resolution.artifact.artifact_id} ·{" "}
                            {selectedRemoteDistribution.resolution.artifact.locator}
                          </div>
                          {selectedRemoteDistribution.installed ? (
                            <div className="mt-2 text-xs text-muted-foreground">
                              installed at {unixTimeLabel(selectedRemoteDistribution.installed.installed_at)} ·{" "}
                              {selectedRemoteDistribution.installed.workspace_skill_path}
                            </div>
                          ) : null}
                        </div>
                      ) : (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">
                          No resolved distribution recorded for the current remote skill yet.
                        </div>
                      )}
                    </div>

                    <div className="grid gap-3">
                      <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">
                        Artifact Cache
                      </div>
                      {selectedRemoteArtifactCache ? (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                          <div className="flex items-center justify-between gap-3">
                            <strong>{selectedRemoteArtifactCache.artifact.artifact_id}</strong>
                            <span
                              className={cn(
                                "rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide",
                                lifecycleStatusClass(selectedRemoteArtifactCache.status),
                              )}
                            >
                              {selectedRemoteArtifactCache.status}
                            </span>
                          </div>
                          <div className="mt-2 text-muted-foreground">
                            cached {unixTimeLabel(selectedRemoteArtifactCache.cached_at)}
                          </div>
                          <div className="mt-1 break-all text-xs text-muted-foreground">
                            local {selectedRemoteArtifactCache.local_path}
                          </div>
                          {selectedRemoteArtifactCache.extracted_path ? (
                            <div className="mt-1 break-all text-xs text-muted-foreground">
                              extracted {selectedRemoteArtifactCache.extracted_path}
                            </div>
                          ) : null}
                          {selectedRemoteArtifactCache.error ? (
                            <div className="mt-2 rounded-xl border border-red-300 bg-red-50/80 px-3 py-2 text-xs leading-relaxed text-red-800 dark:border-red-700 dark:bg-red-950/60 dark:text-red-300">
                              {selectedRemoteArtifactCache.error}
                            </div>
                          ) : (
                            <div className="mt-2 text-xs text-muted-foreground">
                              No artifact fetch error recorded for this distribution.
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">
                          No artifact cache entry captured yet for the current remote skill.
                        </div>
                      )}
                    </div>

                    <div className="grid gap-3">
                      <div className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">
                        Lifecycle State
                      </div>
                      {selectedRemoteLifecycle ? (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                          <div className="flex items-center justify-between gap-3">
                            <strong>{selectedRemoteLifecycle.skill_name}</strong>
                            <span
                              className={cn(
                                "rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide",
                                lifecycleStatusClass(selectedRemoteLifecycle.state),
                              )}
                            >
                              {selectedRemoteLifecycle.state}
                            </span>
                          </div>
                          <div className="mt-2 text-muted-foreground">
                            updated {unixTimeLabel(selectedRemoteLifecycle.updated_at)}
                          </div>
                          {selectedRemoteLifecycle.error ? (
                            <div className="mt-2 rounded-xl border border-red-300 bg-red-50/80 px-3 py-2 text-xs leading-relaxed text-red-800 dark:border-red-700 dark:bg-red-950/60 dark:text-red-300">
                              {selectedRemoteLifecycle.error}
                            </div>
                          ) : (
                            <div className="mt-2 text-xs text-muted-foreground">
                              No lifecycle error recorded for this distribution.
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">
                          No lifecycle record captured yet for the current remote skill.
                        </div>
                      )}
                    </div>
                  </div>
                </div>

                {skillSyncPlan ? (
                  <div className="grid gap-3 rounded-xl border border-border/35 bg-muted/8 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <strong>Sync Plan · {skillSyncPlan.source_id}</strong>
                      <span className="text-sm text-muted-foreground">{skillSyncPlan.entries.length} entries</span>
                    </div>
                    {skillSyncPlan.entries.length ? skillSyncPlan.entries.map((entry) => (
                      <div key={`${entry.skill_name}:${entry.action}`} className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm">
                        <div className="flex items-start justify-between gap-3">
                          <strong>{entry.skill_name}</strong>
                          <span className="text-xs uppercase tracking-wide text-muted-foreground">{entry.action}</span>
                        </div>
                        <div className="mt-2 text-muted-foreground">{entry.reason}</div>
                      </div>
                    )) : (
                      <div className="rounded-lg border border-border/35 bg-background/65 p-3 text-sm text-muted-foreground">This source currently produces an empty plan.</div>
                    )}
                  </div>
                ) : null}

                {remoteInstallPlan ? (
                  <div className="grid gap-3 rounded-xl border border-border/35 bg-muted/8 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <strong>
                        Remote Install Plan · {remoteInstallPlan.entry.skill_name}
                      </strong>
                      <span
                        className={cn(
                          "rounded-full border px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide",
                          lifecycleStatusClass(remoteInstallPlan.entry.action),
                        )}
                      >
                        {remoteInstallPlan.entry.action}
                      </span>
                    </div>
                    <div className="text-sm text-muted-foreground">
                      {remoteInstallPlan.entry.reason}
                    </div>
                    <div className="grid gap-2 text-sm text-muted-foreground">
                      <div>
                        source <code>{remoteInstallPlan.source_id}</code>
                      </div>
                      <div>
                        distribution <code>{remoteInstallPlan.distribution.distribution_id}</code>
                      </div>
                      <div>
                        artifact <code>{remoteInstallPlan.distribution.resolution.artifact.artifact_id}</code>
                      </div>
                      <div>
                        locator <code>{remoteInstallPlan.distribution.resolution.artifact.locator}</code>
                      </div>
                    </div>
                  </div>
                ) : null}

                {skillGuardTarget ? (
                  <div className={mutedCardClass}>
                    Latest guard run targeted <code>{skillGuardTarget}</code> and returned{" "}
                    {skillGuardReports.length} report{skillGuardReports.length === 1 ? "" : "s"}.
                    The full result is now folded into the governance timeline below.
                  </div>
                ) : null}

                <SkillGovernanceTimeline
                  entries={skillGovernanceTimeline}
                  selectedSkillName={selectedSkillName}
                  selectedSourceId={skillSyncSourceId.trim() || null}
                />
              </div>

              <div className="grid gap-6">
                <div className="grid gap-3 content-start">
                  <div className="flex items-center justify-between gap-3">
                    <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Catalog</p>
                    <span>{skillCatalog.length} skills</span>
                  </div>
                  {skillCatalog.length ? (
                    <div className="max-h-[28rem] overflow-y-auto pr-1">
                      <div className="grid gap-2">
                        {skillCatalog.map((skill) => {
                          const selected = selectedSkillEntry?.name === skill.name;
                          const managedRecord =
                            managedRecordBySkill.get(skill.name.trim().toLowerCase()) ?? null;
                          const latestGuard =
                            latestGuardBySkill.get(skill.name.trim().toLowerCase()) ?? null;
                          return (
                            <button
                              key={skill.name}
                              type="button"
                              className={cn(
                                "rounded-lg border px-3 py-2.5 text-left transition-colors",
                                selected
                                  ? "border-border/70 bg-accent"
                                  : "border-transparent bg-transparent hover:bg-accent",
                              )}
                              onClick={() => setSelectedSkillName(skill.name)}
                            >
                              <div className="flex items-start justify-between gap-3">
                                <div className="min-w-0">
                                  <strong className="block truncate">{skill.name}</strong>
                                  <p className="m-0 mt-1 truncate text-xs text-muted-foreground">
                                    {skill.description || "No description"}
                                  </p>
                                </div>
                                <span
                                  className={cn(
                                    "shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
                                    skill.writable
                                      ? "border-green-300 bg-green-50 text-green-800 dark:border-green-700 dark:bg-green-950 dark:text-green-300"
                                      : "border-border bg-muted text-muted-foreground",
                                  )}
                                >
                                  {skill.writable ? "workspace" : "read only"}
                                </span>
                              </div>
                              <div className="mt-2 flex flex-wrap gap-1.5 text-[10px]">
                                <span className="rounded-full border border-border bg-card/80 px-2 py-0.5 text-muted-foreground">
                                  {skill.supporting_files.length} files
                                </span>
                                {skill.category ? (
                                  <span className="rounded-full border border-border bg-card/80 px-2 py-0.5 text-muted-foreground">
                                    {skill.category}
                                  </span>
                                ) : null}
                                {managedRecord ? (
                                  <span
                                    className={cn(
                                      "rounded-full border px-2 py-0.5",
                                      managedRecord.locally_modified || managedRecord.deleted_locally
                                        ? "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                                        : "border-border bg-card/80 text-muted-foreground",
                                    )}
                                  >
                                    {managedSkillStateLabel(managedRecord)}
                                  </span>
                                ) : null}
                                {latestGuard ? (
                                  <span
                                    className={cn(
                                      "rounded-full border px-2 py-0.5",
                                      latestGuard.status === "blocked"
                                        ? "border-red-300 bg-red-50 text-red-800 dark:border-red-700 dark:bg-red-950/60 dark:text-red-300"
                                        : latestGuard.status === "warn"
                                          ? "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                                          : "border-border bg-card/80 text-muted-foreground",
                                    )}
                                  >
                                    {latestGuardStatusLabel(latestGuard)}
                                  </span>
                                ) : null}
                              </div>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  ) : (
                    <p className="rounded-xl border border-border/35 bg-background/60 px-4 py-6 text-sm text-muted-foreground">
                      No skills discovered yet.
                    </p>
                  )}
                </div>

                <div className={sectionCardClass}>
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="m-0 text-xs tracking-widest uppercase text-muted-foreground font-semibold">Create Skill</p>
                      <h3 className="m-0 mt-1">New workspace skill</h3>
                    </div>
                  </div>
                  <input
                    type="text"
                    placeholder="skill name"
                    value={newSkillName}
                    onChange={(event) => setNewSkillName(event.target.value)}
                  />
                  <input
                    type="text"
                    placeholder="description"
                    value={newSkillDescription}
                    onChange={(event) => setNewSkillDescription(event.target.value)}
                  />
                  <input
                    type="text"
                    placeholder="category (optional)"
                    value={newSkillCategory}
                    onChange={(event) => setNewSkillCategory(event.target.value)}
                  />
                  <textarea
                    className={editorTextareaClass}
                    placeholder="Skill body"
                    value={newSkillBody}
                    onChange={(event) => setNewSkillBody(event.target.value)}
                    spellCheck={false}
                  />
                  <div className="flex items-center gap-2">
                    <button
                      className={primaryButtonClass}
                      type="button"
                      disabled={
                        !skillsMutationsEnabled ||
                        !newSkillName.trim() ||
                        !newSkillDescription.trim() ||
                        !newSkillBody.trim() ||
                        busyKey === `skill:create:${newSkillName.trim() || "new"}`
                      }
                      onClick={() => void createSkill()}
                    >
                      {busyKey === `skill:create:${newSkillName.trim() || "new"}`
                        ? "Creating..."
                        : "Create Skill"}
                    </button>
                  </div>
                </div>

                <div className={sectionCardClass}>
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="m-0 text-xs tracking-widest uppercase text-muted-foreground font-semibold">Edit Skill</p>
                        <h3 className="m-0 mt-1">{selectedSkillEntry?.name || "Select a skill"}</h3>
                      </div>
                      {selectedSkillEntry ? (
                        <span
                          className={cn(
                            "rounded-full border px-3 py-1.5 text-xs font-semibold",
                            selectedSkillEntry.writable
                              ? "border-green-300 bg-green-50 text-green-800 dark:border-green-700 dark:bg-green-950 dark:text-green-300"
                              : "border-border bg-muted text-muted-foreground",
                          )}
                        >
                          {selectedSkillEntry.writable ? "Workspace writable" : "Read only"}
                        </span>
                      ) : null}
                    </div>

                    {selectedSkillEntry ? (
                      <>
                        {(() => {
                          const managedRecord =
                            managedRecordBySkill.get(
                              selectedSkillEntry.name.trim().toLowerCase(),
                            ) ?? null;
                          const latestGuard =
                            latestGuardBySkill.get(
                              selectedSkillEntry.name.trim().toLowerCase(),
                            ) ?? null;
                          return (
                            <div className="flex flex-wrap gap-2 text-xs">
                              {managedRecord ? (
                                <>
                                  <span className="rounded-full border border-border bg-card/80 px-2.5 py-1 text-muted-foreground">
                                    source {managedRecord.source?.source_id || "workspace-local"}
                                  </span>
                                  <span
                                    className={cn(
                                      "rounded-full border px-2.5 py-1",
                                      managedRecord.locally_modified ||
                                        managedRecord.deleted_locally
                                        ? "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                                        : "border-border bg-card/80 text-muted-foreground",
                                    )}
                                  >
                                    {managedSkillStateLabel(managedRecord)}
                                  </span>
                                </>
                              ) : null}
                              {latestGuard ? (
                                <span
                                  className={cn(
                                    "rounded-full border px-2.5 py-1",
                                    latestGuard.status === "blocked"
                                      ? "border-red-300 bg-red-50 text-red-800 dark:border-red-700 dark:bg-red-950/60 dark:text-red-300"
                                      : latestGuard.status === "warn"
                                        ? "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                                        : "border-border bg-card/80 text-muted-foreground",
                                  )}
                                >
                                  {latestGuardStatusLabel(latestGuard)} · {latestGuard.violations.length} violations
                                </span>
                              ) : null}
                            </div>
                          );
                        })()}
                        <div className="grid gap-2 rounded-lg border border-border/35 bg-muted/8 p-3 text-sm text-muted-foreground">
                          <div className="break-all">
                            <strong>Location:</strong> {selectedSkillEntry.location}
                          </div>
                          <div>
                            <strong>Category:</strong> {selectedSkillEntry.category || "--"}
                          </div>
                          <div>
                            <strong>Supporting files:</strong>{" "}
                            {selectedSkillEntry.supporting_files.length
                              ? selectedSkillEntry.supporting_files.join(", ")
                              : "none"}
                          </div>
                          {!selectedSkillEntry.writable ? (
                            <div className="text-amber-700 dark:text-amber-300">
                              This skill was discovered outside the workspace skill root. You can
                              inspect it here, but edits and deletes stay disabled because the
                              governed write path only targets <code>{skillWorkspaceRoot}</code>.
                            </div>
                          ) : null}
                        </div>

                        {skillDetailLoading ? (
                          <p className="m-0 text-sm text-muted-foreground">Loading skill source...</p>
                        ) : (
                          <textarea
                            className="min-h-[26rem] w-full resize-y rounded-xl border border-border/45 bg-background/78 p-3.5 text-foreground leading-relaxed font-mono text-sm"
                            value={skillEditorContent}
                            onChange={(event) => setSkillEditorContent(event.target.value)}
                            spellCheck={false}
                            readOnly={!selectedSkillEntry.writable}
                          />
                        )}

                        <div className="flex items-center gap-2">
                          <button
                            className={secondaryButtonClass}
                            type="button"
                            disabled={busyKey === `skill:guard:skill ${selectedSkillEntry.name}`}
                            onClick={() => void runSelectedSkillGuard()}
                          >
                            {busyKey === `skill:guard:skill ${selectedSkillEntry.name}` ? "Scanning..." : "Run Guard Check"}
                          </button>
                          <button
                            className={primaryButtonClass}
                            type="button"
                            disabled={
                              !skillsMutationsEnabled ||
                              !selectedSkillEntry.writable ||
                              skillDetailLoading ||
                              busyKey === `skill:edit:${selectedSkillEntry.name}`
                            }
                            onClick={() => void saveSelectedSkill()}
                          >
                            {busyKey === `skill:edit:${selectedSkillEntry.name}` ? "Saving..." : "Save Skill"}
                          </button>
                          <button
                            className={secondaryButtonClass}
                            type="button"
                            disabled={
                              !skillsMutationsEnabled ||
                              !selectedSkillEntry.writable ||
                              busyKey === `skill:delete:${selectedSkillEntry.name}`
                            }
                            onClick={() => void deleteSelectedSkill()}
                          >
                            {busyKey === `skill:delete:${selectedSkillEntry.name}` ? "Deleting..." : "Delete Skill"}
                          </button>
                        </div>
                      </>
                    ) : (
                      <p className="rounded-xl border border-border/35 bg-muted/8 px-4 py-6 text-sm text-muted-foreground">
                        Select a skill from the catalog to inspect or edit its raw <code>SKILL.md</code>.
                      </p>
                    )}
                </div>
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "mcp" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Runtime Status</p>
                  <span>{Object.keys(mcpStatus).length} servers</span>
                </div>
                {Object.values(mcpStatus).length ? (
                  Object.values(mcpStatus).map((server) => (
                    <div key={server.name} className="rounded-xl border border-border bg-card/70 p-4 flex items-start justify-between gap-4">
                      <div>
                        <strong>{server.name}</strong>
                        <p className="text-sm text-muted-foreground leading-relaxed">
                          status {server.status} · tools {server.tools} · resources {server.resources}
                        </p>
                        {server.error ? <p className="text-sm text-muted-foreground leading-relaxed">{server.error}</p> : null}
                      </div>
                      <div className="flex items-center gap-2">
                        <button
                          className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                          type="button"
                          disabled={busyKey === `mcp:connect:${server.name}`}
                          onClick={() => void runMcpAction(server.name, "connect")}
                        >
                          Connect
                        </button>
                        <button
                          className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                          type="button"
                          disabled={busyKey === `mcp:disconnect:${server.name}`}
                          onClick={() => void runMcpAction(server.name, "disconnect")}
                        >
                          Disconnect
                        </button>
                        <button
                          className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                          type="button"
                          disabled={busyKey === `mcp:restart:${server.name}`}
                          onClick={() => void runMcpAction(server.name, "restart")}
                        >
                          Restart
                        </button>
                      </div>
                    </div>
                  ))
                ) : (
                  <p className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">No MCP runtime entries reported yet.</p>
                )}
              </div>

              {Object.entries(mcpConfigs).map(([key]) => (
                <div key={key} className="grid gap-3 col-span-full">
                  <label>{key}</label>
                  <textarea
                    className={editorTextareaClass}
                    value={mcpDrafts[key] ?? ""}
                    onChange={(event) => setMcpDrafts((current) => ({ ...current, [key]: event.target.value }))}
                    spellCheck={false}
                  />
                  <div className="flex items-center gap-2">
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === `mcp:save:${key}`}
                      onClick={() => void saveMcpConfig(key, mcpDrafts[key] ?? "")}
                    >
                      Save
                    </button>
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === `mcp:delete:${key}`}
                      onClick={() => void deleteMcpConfig(key)}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              ))}

              <div className="grid gap-3 col-span-full">
                <label htmlFor="settings-new-mcp-key">New MCP Config</label>
                <input
                  id="settings-new-mcp-key"
                  type="text"
                  placeholder="server name"
                  value={newMcpKey}
                  onChange={(event) => setNewMcpKey(event.target.value)}
                />
                <textarea
                  className={editorTextareaClass}
                  value={newMcpDraft}
                  onChange={(event) => setNewMcpDraft(event.target.value)}
                  spellCheck={false}
                />
                <button
                  className="min-h-[36px] rounded-full px-5 bg-foreground border-foreground text-background text-sm font-semibold inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px"
                  type="button"
                  disabled={!newMcpKey.trim() || busyKey === `mcp:save:${newMcpKey.trim()}`}
                  onClick={() => void saveMcpConfig(newMcpKey.trim(), newMcpDraft)}
                >
                  Add MCP Config
                </button>
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "plugins" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Auth Bridges</p>
                  <span>{pluginAuthProviders.length} providers</span>
                </div>
                {pluginAuthProviders.length ? (
                  pluginAuthProviders.map((provider) => (
                    <div key={provider.provider} className="rounded-xl border border-border bg-card/70 p-4 flex items-start justify-between gap-4">
                      <div>
                        <strong>{provider.provider}</strong>
                        <p className="text-sm text-muted-foreground leading-relaxed">
                          {provider.methods.length
                            ? provider.methods
                                .map((method) => method.label || method.type || "method")
                                .join(", ")
                            : "no auth methods"}
                        </p>
                      </div>
                    </div>
                  ))
                ) : (
                  <p className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">No plugin auth bridges reported.</p>
                )}
              </div>

              {Object.entries(pluginConfigs).map(([key]) => (
                <div key={key} className="grid gap-3 col-span-full">
                  <label>{key}</label>
                  <textarea
                    className={editorTextareaClass}
                    value={pluginDrafts[key] ?? ""}
                    onChange={(event) => setPluginDrafts((current) => ({ ...current, [key]: event.target.value }))}
                    spellCheck={false}
                  />
                  <div className="flex items-center gap-2">
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === `plugin:save:${key}`}
                      onClick={() => void savePluginConfig(key, pluginDrafts[key] ?? "")}
                    >
                      Save
                    </button>
                    <button
                      className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
                      type="button"
                      disabled={busyKey === `plugin:delete:${key}`}
                      onClick={() => void deletePluginConfig(key)}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              ))}

              <div className="grid gap-3 col-span-full">
                <label htmlFor="settings-new-plugin-key">New Plugin Config</label>
                <input
                  id="settings-new-plugin-key"
                  type="text"
                  placeholder="plugin name"
                  value={newPluginKey}
                  onChange={(event) => setNewPluginKey(event.target.value)}
                />
                <textarea
                  className={editorTextareaClass}
                  value={newPluginDraft}
                  onChange={(event) => setNewPluginDraft(event.target.value)}
                  spellCheck={false}
                />
                <button
                  className="min-h-[36px] rounded-full px-5 bg-foreground border-foreground text-background text-sm font-semibold inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px"
                  type="button"
                  disabled={!newPluginKey.trim() || busyKey === `plugin:save:${newPluginKey.trim()}`}
                  onClick={() => void savePluginConfig(newPluginKey.trim(), newPluginDraft)}
                >
                  Add Plugin Config
                </button>
              </div>
            </div>
          ) : null}

          {!loading && activeTab === "lsp" ? (
            <div className="grid gap-6">
              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">LSP Servers</p>
                  <span>{lspStatus?.servers.length ?? 0}</span>
                </div>
                {lspStatus?.servers.length ? (
                  lspStatus.servers.map((server) => (
                    <div key={server} className="rounded-xl border border-border bg-card/70 p-4 flex items-center justify-between gap-4">
                      <strong>{server}</strong>
                    </div>
                  ))
                ) : (
                  <p className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">No LSP servers reported by `/lsp`.</p>
                )}
              </div>

              <div className="grid gap-3">
                <div className="flex items-center justify-between gap-3">
                  <p className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Formatters</p>
                  <span>{formatterStatus?.formatters.length ?? 0}</span>
                </div>
                {formatterStatus?.formatters.length ? (
                  formatterStatus.formatters.map((formatter) => (
                    <div key={formatter} className="rounded-xl border border-border bg-card/70 p-4 flex items-center justify-between gap-4">
                      <strong>{formatter}</strong>
                    </div>
                  ))
                ) : (
                  <p className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">No formatter status reported by `/formatter`.</p>
                )}
              </div>
            </div>
          ) : null}
        </div>
      </section>
    </div>
  );
}
