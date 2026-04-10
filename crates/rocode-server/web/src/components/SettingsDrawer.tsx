import { useCallback, useEffect, useMemo, useState } from "react";
import { cn } from "@/lib/utils";

type SettingsTabId = "general" | "providers" | "scheduler" | "mcp" | "plugins" | "lsp";

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
    case "mcp":
      return "MCP config saved here is global. An isolated workspace does not automatically inherit that global config into its current sandbox runtime.";
    case "plugins":
      return "Plugin config saved here is global. The current isolated sandbox will not inherit those global config changes unless they are mirrored into this workspace's .rocode authority.";
    default:
      return null;
  }
}

function formatError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error ?? "Unknown error");
}

function stringifyJson(value: unknown) {
  return JSON.stringify(value ?? {}, null, 2);
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

  const reloadSettingsData = useCallback(async () => {
    setRefreshing(true);
    setFeedback(null);
    try {
      const [config, managed, scheduler, mcp, plugins, lsp, formatter] = await Promise.all([
        apiJson<AppConfigSnapshot>("/config"),
        apiJson<{ providers: ManagedProviderInfo[] }>("/provider/managed"),
        apiJson<SchedulerConfigResponse>("/config/scheduler"),
        apiJson<Record<string, McpStatusInfo>>("/mcp"),
        apiJson<PluginAuthProviderInfo[]>("/plugin/auth").catch(() => []),
        apiJson<LspStatus>("/lsp"),
        apiJson<FormatterStatus>("/formatter"),
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
    } catch (error) {
      const message = `Failed to load settings data: ${formatError(error)}`;
      setFeedback(message);
      onBanner(message);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [apiJson, onBanner]);

  useEffect(() => {
    void reloadSettingsData();
  }, [reloadSettingsData]);

  const runMutation = useCallback(
    async (key: string, action: () => Promise<void>, success: string) => {
      setBusyKey(key);
      setFeedback(null);
      try {
        await action();
        await Promise.all([reloadSettingsData(), onReloadCoreData()]);
        setFeedback(success);
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

  const providerSummary = `${providers.length} connected / ${knownProviders.length} known`;
  const chooseKnownProvider = (provider: KnownProviderEntryLike) => {
    onConnectQueryChange(provider.id);
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/40 backdrop-blur-sm flex items-start justify-end" data-testid="settings-overlay" onClick={onClose}>
      <section
        className="h-full w-full max-w-3xl bg-card border-l border-border overflow-y-auto p-8 flex flex-col gap-6"
        data-testid="settings-drawer"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-4">
          <div>
            <p className="m-0 mb-1.5 text-xs tracking-widest uppercase text-amber-700 font-bold">Settings</p>
            <h2>General, providers, scheduler, MCP, plugins, LSP</h2>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent"
              type="button"
              data-testid="settings-refresh"
              onClick={() => void reloadSettingsData()}
            >
              {refreshing ? "Refreshing..." : "Refresh"}
            </button>
            <button className="min-h-[36px] rounded-full px-4 border border-border bg-card/70 text-foreground text-sm inline-flex items-center justify-center cursor-pointer transition-all duration-150 hover:-translate-y-px hover:bg-accent" type="button" data-testid="settings-close" onClick={onClose}>
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
              className={activeTab === tab.id ? "px-4 py-2 rounded-full border-0 cursor-pointer text-sm bg-foreground text-background font-semibold" : "px-4 py-2 rounded-full border border-border cursor-pointer text-sm bg-card/70 text-foreground hover:bg-accent"}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {feedback ? <div className="rounded-2xl border border-amber-300 bg-amber-50/80 px-5 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200">{feedback}</div> : null}

        <div className="flex flex-col gap-6 flex-1 min-h-0">
          {loading ? <div className="flex flex-col items-center justify-center gap-3 text-muted-foreground py-8">Loading settings...</div> : null}
          {!loading && isolatedNotice ? (
            <div className="rounded-2xl border border-amber-300 bg-amber-50/80 px-5 py-3 text-sm leading-relaxed text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200">
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
                      className={theme === item.id ? "px-4 py-2 rounded-full border-0 cursor-pointer text-sm bg-foreground text-background font-semibold" : "px-4 py-2 rounded-full border border-border cursor-pointer text-sm bg-card/70 text-foreground hover:bg-accent"}
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
                  <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
                    <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Workspace Mode</span>
                    <strong>{workspaceMode === "isolated" ? "isolated sandbox" : "shared workspace"}</strong>
                  </div>
                  <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
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
                    "rounded-2xl border px-4 py-3 text-sm leading-relaxed",
                    workspaceMode === "isolated"
                      ? "border-amber-300 bg-amber-50/80 text-amber-900 dark:border-amber-700 dark:bg-amber-950/60 dark:text-amber-200"
                      : "border-border bg-muted/20 text-muted-foreground",
                  )}
                >
                  {workspaceMode === "isolated"
                    ? "This workspace runs as an isolated sandbox. It will not inherit global config, managed home config, or shared workspace overrides outside this .rocode root."
                    : "This workspace runs in shared mode. Global config can still participate in the resolved runtime context alongside workspace-local settings."}
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Providers</span>
                  <strong>{providerSummary}</strong>
                </div>
                <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">Scheduler Path</span>
                  <strong>{schedulerConfig?.raw_path || configSnapshot?.schedulerPath || "--"}</strong>
                </div>
                <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
                  <span className="text-xs tracking-widest uppercase text-muted-foreground font-semibold">MCP Servers</span>
                  <strong>{Object.keys(mcpConfigs).length}</strong>
                </div>
                <div className="rounded-2xl border border-border bg-card/80 p-4 grid gap-2">
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
                  <div className="grid gap-2 rounded-xl border border-border bg-card/50 p-3">
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
                          className="rounded-xl border border-border bg-card/70 px-3 py-2 text-left text-sm transition-all duration-150 hover:-translate-y-px hover:bg-accent"
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
                  className="min-h-40 w-full resize-y rounded-2xl border border-border bg-card/80 p-3.5 text-foreground leading-relaxed font-mono text-sm"
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
                    className="min-h-40 w-full resize-y rounded-2xl border border-border bg-card/80 p-3.5 text-foreground leading-relaxed font-mono text-sm"
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
                  className="min-h-40 w-full resize-y rounded-2xl border border-border bg-card/80 p-3.5 text-foreground leading-relaxed font-mono text-sm"
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
                    className="min-h-40 w-full resize-y rounded-2xl border border-border bg-card/80 p-3.5 text-foreground leading-relaxed font-mono text-sm"
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
                  className="min-h-40 w-full resize-y rounded-2xl border border-border bg-card/80 p-3.5 text-foreground leading-relaxed font-mono text-sm"
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
