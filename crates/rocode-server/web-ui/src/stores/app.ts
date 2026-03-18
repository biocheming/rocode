// ── App Store ──────────────────────────────────────────────────────────────
// Central reactive state for the entire application.
// Replaces the global `state` object from constants.js.

import { createSignal, createMemo, batch } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { api, apiJson } from "~/api/client";
import { parseSSE } from "~/api/sse";
import { baseName } from "~/utils/format";
import type {
  Session,
  Provider,
  ExecutionMode,
  UiCommand,
  OutputBlock,
  OutputBlockEvent,
  UsageEvent,
  QuestionInteraction,
  PermissionInteraction,
  ExecutionTopology,
  RecoveryProtocol,
} from "~/api/types";

// ── Theme constants ────────────────────────────────────────────────────────

export const THEMES = [
  { id: "midnight", label: "Midnight" },
  { id: "graphite", label: "Graphite" },
  { id: "sunset", label: "Sunset" },
  { id: "daylight", label: "Daylight" },
] as const;

export type ThemeId = (typeof THEMES)[number]["id"];

// ── Normalized session helpers ─────────────────────────────────────────────

export interface NormalizedSession {
  readonly id: string;
  readonly title: string;
  readonly project_id: string;
  readonly directory: string;
  readonly updated: number;
  readonly share_url: string | null;
  readonly metadata: Record<string, unknown> | null;
}

export interface Project {
  readonly key: string;
  readonly label: string;
  readonly sessions: readonly NormalizedSession[];
}

function projectKey(session: { project_id?: string; directory?: string }): string {
  if (session.project_id && session.project_id !== "default") return session.project_id;
  return session.directory || "default";
}

function projectLabel(session: { project_id?: string; directory?: string }): string {
  if (session.project_id && session.project_id !== "default") return baseName(session.project_id);
  return baseName(session.directory);
}

export function normalizeSession(session: Session & Record<string, unknown>): NormalizedSession {
  const time = session.time as { updated?: number } | undefined;
  const share = session.share as { url?: string } | undefined;
  return {
    id: session.id,
    title: session.title || "(untitled)",
    project_id: (session.project_id as string) || "default",
    directory: (session.directory as string) || "",
    updated: time?.updated ?? Date.now(),
    share_url: share?.url ?? null,
    metadata: (session.metadata as Record<string, unknown>) ?? null,
  };
}

function sortSessions(items: NormalizedSession[]): NormalizedSession[] {
  return [...items].sort((a, b) => Number(b.updated) - Number(a.updated));
}

function normalizeSessions(items: (Session & Record<string, unknown>)[]): NormalizedSession[] {
  return sortSessions(
    (items || []).filter((s) => !s.parent_id).map(normalizeSession),
  );
}

// ── Store shape ────────────────────────────────────────────────────────────

export interface AppState {
  // Sessions
  sessions: NormalizedSession[];
  projects: Project[];
  selectedProject: string | null;
  selectedSession: string | null;
  parentSessionId: string | null;

  // Providers & modes
  providers: Provider[];
  modes: ExecutionMode[];
  uiCommands: UiCommand[];
  selectedModel: string | null;
  selectedModeKey: string | null;

  // Theme
  selectedTheme: ThemeId;
  showThinking: boolean;

  // Runtime
  streaming: boolean;
  abortRequested: boolean;
  busyAction: string | null;

  // Execution
  executionTopology: ExecutionTopology | null;
  recoveryProtocol: RecoveryProtocol | null;

  // Tokens
  promptTokens: number;
  completionTokens: number;

  // Stream state
  focusedChildSessionId: string | null;

  // Interactions
  activeQuestionInteraction: QuestionInteraction | null;
  questionSubmitting: boolean;
  activePermissionInteraction: PermissionInteraction | null;
  permissionSubmitting: boolean;

  // Settings
  settingsOpen: boolean;
  settingsActiveTab: string;
  configSnapshot: Record<string, unknown> | null;

  // Domain tab
  activeDomain: "chat" | "scheduler" | "terminal" | "workspace";
}

// ── Create store ───────────────────────────────────────────────────────────

const [state, setState] = createStore<AppState>({
  sessions: [],
  projects: [],
  selectedProject: null,
  selectedSession: null,
  parentSessionId: null,
  providers: [],
  modes: [],
  uiCommands: [],
  selectedModel: null,
  selectedModeKey: null,
  selectedTheme: "daylight",
  showThinking: true,
  streaming: false,
  abortRequested: false,
  busyAction: null,
  executionTopology: null,
  recoveryProtocol: null,
  promptTokens: 0,
  completionTokens: 0,
  focusedChildSessionId: null,
  activeQuestionInteraction: null,
  questionSubmitting: false,
  activePermissionInteraction: null,
  permissionSubmitting: false,
  settingsOpen: false,
  settingsActiveTab: "general",
  configSnapshot: null,
  activeDomain: "chat",
});

export { state };

// ── Derived signals ────────────────────────────────────────────────────────

export const currentSession = createMemo(() =>
  state.sessions.find((s) => s.id === state.selectedSession) ?? null,
);

export const selectedMode = createMemo(() =>
  state.modes.find((m) => `${m.kind}:${m.id}` === state.selectedModeKey) ?? null,
);

export const selectedModeLabel = createMemo(() => {
  const mode = selectedMode();
  if (!mode) return "auto";
  return mode.kind === "agent" ? mode.name : `${mode.kind}:${mode.name}`;
});

export const interactionLocked = createMemo(() =>
  state.streaming || Boolean(state.busyAction),
);

export const runtimeStatusLabel = createMemo(() => {
  if (state.abortRequested) return "cancelling";
  if (state.streaming) return "running";
  if (state.busyAction) return state.busyAction;
  return "ready";
});

export const runtimeStatusTone = createMemo(() => {
  if (state.abortRequested) return "warn";
  if (state.streaming || state.busyAction) return "warn";
  return "ok";
});

// ── Actions ────────────────────────────────────────────────────────────────

export function buildProjects(searchQuery = ""): Project[] {
  const map = new Map<string, { key: string; label: string; sessions: NormalizedSession[] }>();
  for (const session of state.sessions) {
    const key = projectKey(session);
    if (!map.has(key)) {
      map.set(key, { key, label: projectLabel(session), sessions: [] });
    }
    map.get(key)!.sessions.push(session);
  }

  const query = searchQuery.trim().toLowerCase();
  const projects = Array.from(map.values())
    .map((project) => {
      if (!query) return project;
      if (project.label.toLowerCase().includes(query)) return project;
      const sessions = project.sessions.filter((s) => s.title.toLowerCase().includes(query));
      return { ...project, sessions };
    })
    .filter((project) => project.sessions.length > 0)
    .sort((a, b) => Number(b.sessions[0].updated) - Number(a.sessions[0].updated));

  setState("projects", projects);
  return projects;
}

export function setTheme(themeId: string, options: { persist?: boolean } = {}) {
  const { persist = true } = options;
  const valid = THEMES.some((t) => t.id === themeId) ? (themeId as ThemeId) : "daylight";
  setState("selectedTheme", valid);
  document.documentElement.dataset.theme = valid;
  if (persist) {
    void persistWebUiPreferences().catch(() => {});
  }
}

export function setSelectedMode(modeKey: string | null, options: { persist?: boolean } = {}) {
  const { persist = true } = options;
  setState("selectedModeKey", modeKey?.trim() || null);
  if (persist) {
    void persistWebUiPreferences().catch(() => {});
  }
}

export function setSelectedModel(modelKey: string | null, options: { persist?: boolean } = {}) {
  const { persist = true } = options;
  setState("selectedModel", modelKey?.trim() || null);
  if (persist) {
    void persistWebUiPreferences().catch(() => {});
  }
}

export function applyStreamUsage(payload: UsageEvent) {
  batch(() => {
    if (payload.prompt_tokens != null) setState("promptTokens", payload.prompt_tokens);
    if (payload.completion_tokens != null) setState("completionTokens", payload.completion_tokens);
    if (payload.promptTokens != null) setState("promptTokens", payload.promptTokens);
    if (payload.completionTokens != null) setState("completionTokens", payload.completionTokens);
  });
}

// ── Session actions ────────────────────────────────────────────────────────

export async function refreshSessionsIndex(): Promise<void> {
  const response = await api("/session?roots=true&limit=120");
  const data = await response.json();
  const sessions = normalizeSessions(data);

  batch(() => {
    const prevSelected = state.selectedSession;
    const prevProject = state.selectedProject;

    setState("sessions", sessions);
    buildProjects();

    if (prevProject && state.projects.some((p) => p.key === prevProject)) {
      setState("selectedProject", prevProject);
    } else if (state.projects.length > 0) {
      setState("selectedProject", state.projects[0].key);
    } else {
      setState("selectedProject", null);
    }

    if (prevSelected && sessions.some((s) => s.id === prevSelected)) {
      setState("selectedSession", prevSelected);
    } else {
      const proj = state.projects.find((p) => p.key === state.selectedProject);
      setState("selectedSession", proj?.sessions[0]?.id ?? null);
    }

    if (state.parentSessionId && !sessions.some((s) => s.id === state.parentSessionId)) {
      setState("parentSessionId", null);
    }
  });
}

export async function createAndSelectSession(): Promise<string> {
  const response = await api("/session", {
    method: "POST",
    body: JSON.stringify({}),
  });
  const created = await response.json();
  batch(() => {
    setState("selectedSession", created.id);
    setState("selectedProject", projectKey(created));
  });
  await refreshSessionsIndex();
  return created.id;
}

export async function deleteSession(sessionId: string): Promise<void> {
  await api(`/session/${sessionId}`, { method: "DELETE" });
  batch(() => {
    const remaining = state.sessions.filter((s) => s.id !== sessionId);
    setState("sessions", remaining);
    if (state.selectedSession === sessionId) {
      if (remaining.length > 0) {
        setState("selectedSession", remaining[0].id);
        setState("selectedProject", projectKey(remaining[0]));
      } else {
        setState("selectedSession", null);
      }
    }
    buildProjects();
  });
}

export async function forkSession(sessionId: string): Promise<string> {
  const response = await api(`/session/${sessionId}/fork`, {
    method: "POST",
    body: JSON.stringify({ message_id: null }),
  });
  const forked = await response.json();
  batch(() => {
    setState("selectedSession", forked.id);
    setState("selectedProject", projectKey(forked));
  });
  await refreshSessionsIndex();
  return forked.id;
}

export async function renameSession(sessionId: string, title: string): Promise<void> {
  await api(`/session/${sessionId}/title`, {
    method: "PATCH",
    body: JSON.stringify({ title }),
  });
  await refreshSessionsIndex();
}

export async function compactSession(sessionId: string): Promise<void> {
  await api(`/session/${sessionId}/compaction`, { method: "POST" });
}

export async function toggleShareSession(sessionId: string): Promise<string | null> {
  const session = state.sessions.find((s) => s.id === sessionId);
  if (session?.share_url) {
    await api(`/session/${sessionId}/share`, { method: "DELETE" });
    await refreshSessionsIndex();
    return null;
  }
  const response = await api(`/session/${sessionId}/share`, { method: "POST" });
  const data = await response.json();
  await refreshSessionsIndex();
  return data?.url ?? null;
}

// ── Data loading ───────────────────────────────────────────────────────────

export async function loadProviders(): Promise<void> {
  const response = await api("/config/providers");
  const data = await response.json();
  setState("providers", data.providers || data.all || []);
}

export async function loadModes(): Promise<void> {
  const response = await api("/mode");
  const data: ExecutionMode[] = await response.json();
  const modes = (data || [])
    .filter((mode) => mode.hidden !== true)
    .filter((mode) => mode.kind !== "agent" || mode.mode !== "subagent")
    .map((mode) => ({
      ...mode,
      kind: mode.kind || "agent",
    }));
  setState("modes", modes);

  if (state.selectedModeKey) {
    const found = modes.some((m) => `${m.kind}:${m.id}` === state.selectedModeKey);
    if (!found) setSelectedMode(null, { persist: false });
  }
}

export async function loadUiCommands(): Promise<void> {
  const response = await api("/command/ui");
  const data: UiCommand[] = await response.json();
  setState("uiCommands", data || []);
}

// ── Preferences ────────────────────────────────────────────────────────────

export async function persistWebUiPreferences(): Promise<void> {
  await api("/config", {
    method: "PATCH",
    body: JSON.stringify({
      uiPreferences: {
        webTheme: state.selectedTheme,
        webMode: state.selectedModeKey,
        webModel: state.selectedModel,
        showThinking: state.showThinking,
      },
    }),
  });
}

export function applyWebUiPreferences(config: Record<string, unknown>) {
  const ui = (config?.uiPreferences ?? config?.ui_preferences ?? {}) as Record<string, unknown>;
  const webTheme = (ui.webTheme ?? ui.web_theme ?? null) as string | null;
  const webMode = (ui.webMode ?? ui.web_mode ?? null) as string | null;
  const webModel = (ui.webModel ?? ui.web_model ?? null) as string | null;
  const showThinking = (ui.showThinking ?? ui.show_thinking ?? state.showThinking) as boolean;

  batch(() => {
    setTheme(webTheme || state.selectedTheme || "daylight", { persist: false });
    setState("showThinking", Boolean(showThinking));
    setSelectedMode(webMode, { persist: false });
    setSelectedModel(webModel, { persist: false });
  });
}

export async function loadWebUiPreferences(): Promise<void> {
  const response = await api("/config");
  const config = await response.json();
  applyWebUiPreferences(config);
}

// ── Abort ──────────────────────────────────────────────────────────────────

export async function abortCurrentExecution(): Promise<void> {
  if (!state.selectedSession || !state.streaming || state.abortRequested) return;
  setState("abortRequested", true);

  const mode = selectedMode();
  const isScheduler = mode && (mode.kind === "preset" || mode.kind === "profile");
  const path = isScheduler
    ? `/session/${state.selectedSession}/scheduler/stage/abort`
    : `/session/${state.selectedSession}/abort`;

  try {
    await api(path, { method: "POST" });
  } catch {
    setState("abortRequested", false);
    throw new Error("Abort failed");
  }
}

// ── Execution topology ─────────────────────────────────────────────────────

export async function refreshExecutionTopology(sessionId?: string): Promise<void> {
  const sid = sessionId || state.selectedSession;
  if (!sid) {
    setState("executionTopology", null);
    setState("recoveryProtocol", null);
    return;
  }

  try {
    const [execRes, recoveryRes] = await Promise.all([
      api(`/session/${sid}/executions`).then((r) => r.json()),
      api(`/session/${sid}/recovery`).then((r) => r.json()).catch(() => null),
    ]);
    batch(() => {
      setState("executionTopology", execRes);
      if (recoveryRes) setState("recoveryProtocol", recoveryRes);
    });
  } catch {
    // Silently ignore — topology refresh is best-effort
  }
}

// ── Prompt sending ─────────────────────────────────────────────────────────

export async function sendPrompt(content: string): Promise<void> {
  if (!content || interactionLocked()) return;

  // Show user message immediately in the feed
  _outputBlockListener?.({
    kind: "message",
    phase: "full",
    role: "user",
    text: content,
  }, undefined);

  batch(() => {
    setState("streaming", true);
    setState("abortRequested", false);
    setState("promptTokens", 0);
    setState("completionTokens", 0);
  });

  try {
    if (!state.selectedSession) {
      await createAndSelectSession();
    }

    const mode = selectedMode();
    const payload: Record<string, unknown> = {
      content,
      stream: true,
      model: state.selectedModel,
    };
    if (mode) {
      if (mode.kind === "agent") {
        payload.agent = mode.id;
      } else if (mode.kind === "preset" || mode.kind === "profile") {
        payload.scheduler_profile = mode.id;
      }
    }

    const response = await api(`/session/${state.selectedSession}/stream`, {
      method: "POST",
      body: JSON.stringify(payload),
    });

    // Consume the SSE response — output_block events also arrive via global /event,
    // but we consume here to keep the connection alive until the stream ends.
    await parseSSE(response, () => {
      // Events are handled by the global SSE handler
    });
  } catch (error) {
    throw error;
  } finally {
    batch(() => {
      setState("streaming", false);
      setState("abortRequested", false);
    });
    void refreshExecutionTopology().catch(() => {});
  }
}

// ── Session selection ───────────────────────────────────────────────────────

export async function selectSession(sessionId: string): Promise<void> {
  if (state.selectedSession === sessionId) return;
  const session = state.sessions.find((s) => s.id === sessionId);
  if (!session) return;

  batch(() => {
    setState("selectedSession", sessionId);
    setState("selectedProject", projectKey(session));
    setState("streaming", false);
    setState("abortRequested", false);
  });
}

export async function loadSessionMessages(sessionId: string): Promise<OutputBlock[]> {
  const response = await api(`/session/${sessionId}/message`);
  const data = await response.json();
  const blocks: OutputBlock[] = [];
  for (const msg of data || []) {
    // Each message has parts[]; collect text parts into a single block
    const parts = (msg.parts ?? []) as { type?: string; text?: string; tool_call?: unknown; tool_result?: unknown }[];
    const textParts = parts
      .filter((p) => p.type === "text" && p.text)
      .map((p) => p.text!)
      .join("\n");
    if (textParts) {
      blocks.push({
        kind: "message",
        phase: "full",
        role: msg.role ?? "assistant",
        text: textParts,
      });
    }
  }
  return blocks;
}

// ── Domain tab ──────────────────────────────────────────────────────────────

export function setActiveDomain(domain: AppState["activeDomain"]) {
  setState("activeDomain", domain);
}

// ── Global SSE event handler ────────────────────────────────────────────────

export function handleSSEEvent(_name: string, payload: unknown) {
  const event = payload as Record<string, unknown>;
  const type = event?.type as string | undefined;
  if (!type) return;

  const eventSessionId = (event.sessionID ?? event.session_id) as string | undefined;

  switch (type) {
    case "output_block": {
      // Only process events for the currently selected session
      if (eventSessionId && eventSessionId === state.selectedSession) {
        const block = event.block as OutputBlock | undefined;
        if (block) {
          // Dispatch to the global output block listener if registered
          _outputBlockListener?.(block, event.id as string | undefined);
        }
      }
      break;
    }
    case "usage": {
      if (eventSessionId && eventSessionId === state.selectedSession) {
        applyStreamUsage(event as unknown as UsageEvent);
      }
      break;
    }
    case "error": {
      if (eventSessionId && eventSessionId === state.selectedSession) {
        const done = event.done as boolean | undefined;
        if (done) {
          batch(() => {
            setState("streaming", false);
            setState("abortRequested", false);
          });
        }
        _outputBlockListener?.({
          kind: "status",
          tone: "error",
          text: (event.error as string) || "Unknown error",
        }, undefined);
      }
      break;
    }
    case "session.updated": {
      void refreshSessionsIndex().catch(() => {});
      break;
    }
    case "session.status": {
      const status = event.status as string | undefined;
      if (eventSessionId === state.selectedSession) {
        if (status === "idle" || status === "complete" || status === "error") {
          batch(() => {
            setState("streaming", false);
            setState("abortRequested", false);
          });
        }
      }
      break;
    }
    case "question.created": {
      if (eventSessionId === state.selectedSession) {
        setState("activeQuestionInteraction", {
          request_id: event.requestID as string,
          session_id: eventSessionId,
          questions: event.questions as QuestionInteraction["questions"],
        });
      }
      break;
    }
    case "question.resolved": {
      if (eventSessionId === state.selectedSession) {
        setState("activeQuestionInteraction", null);
        setState("questionSubmitting", false);
      }
      break;
    }
    case "permission.requested": {
      if (eventSessionId === state.selectedSession) {
        const info = event.info as Record<string, unknown> | undefined;
        setState("activePermissionInteraction", {
          permission_id: event.permissionID as string,
          session_id: eventSessionId,
          message: (info?.message ?? info?.description) as string | undefined,
          permission: info?.permission as string | undefined,
          command: info?.command as string | undefined,
          filepath: info?.filepath as string | undefined,
          patterns: info?.patterns as string[] | undefined,
        });
      }
      break;
    }
    case "permission.resolved": {
      if (eventSessionId === state.selectedSession) {
        setState("activePermissionInteraction", null);
        setState("permissionSubmitting", false);
      }
      break;
    }
    case "execution.topology.changed": {
      if (eventSessionId === state.selectedSession) {
        void refreshExecutionTopology().catch(() => {});
      }
      break;
    }
    case "config.updated": {
      void loadProviders().catch(() => {});
      void loadModes().catch(() => {});
      break;
    }
  }
}

// ── Output block listener (set by ChatDomain) ──────────────────────────────

type OutputBlockListenerFn = (block: OutputBlock, id: string | undefined) => void;
let _outputBlockListener: OutputBlockListenerFn | null = null;

export function setOutputBlockListener(listener: OutputBlockListenerFn | null) {
  _outputBlockListener = listener;
}
