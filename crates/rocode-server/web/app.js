const THEMES = [
  { id: "midnight", label: "Midnight" },
  { id: "graphite", label: "Graphite" },
  { id: "sunset", label: "Sunset" },
  { id: "daylight", label: "Daylight" },
];

const storedMode = localStorage.getItem("rocode_web_mode");
const storedAgent = localStorage.getItem("rocode_web_agent");
const initialMode = storedMode && storedMode !== "auto" ? storedMode : storedAgent && storedAgent !== "auto" ? `agent:${storedAgent}` : null;

const state = {
  sessions: [],
  projects: [],
  providers: [],
  modes: [],
  selectedProject: null,
  selectedSession: null,
  selectedModel: null,
  selectedModeKey: initialMode,
  selectedTheme: localStorage.getItem("rocode_web_theme") || "midnight",
  streaming: false,
  busyAction: null,
  promptTokens: 0,
  completionTokens: 0,
  streamMessageNode: null,
  streamToolBlocks: new Map(),
};

const nodes = {
  shell: document.getElementById("appShell"),
  sidebarToggle: document.getElementById("sidebarToggle"),
  projectSearch: document.getElementById("projectSearch"),
  projectCount: document.getElementById("projectCount"),
  projectTree: document.getElementById("projectTree"),
  heroPanel: document.getElementById("heroPanel"),
  conversationPanel: document.getElementById("conversationPanel"),
  sessionTitle: document.getElementById("sessionTitle"),
  sessionMeta: document.getElementById("sessionMeta"),
  messageFeed: document.getElementById("messageFeed"),
  refreshSession: document.getElementById("refreshSession"),
  newSessionBtn: document.getElementById("newSessionBtn"),
  forkSessionBtn: document.getElementById("forkSessionBtn"),
  compactSessionBtn: document.getElementById("compactSessionBtn"),
  renameSessionBtn: document.getElementById("renameSessionBtn"),
  shareSessionBtn: document.getElementById("shareSessionBtn"),
  deleteSessionBtn: document.getElementById("deleteSessionBtn"),
  commandBtn: document.getElementById("commandBtn"),
  commandPanel: document.getElementById("commandPanel"),
  commandClose: document.getElementById("commandClose"),
  modelSelect: document.getElementById("modelSelect"),
  themeSelect: document.getElementById("themeSelect"),
  agentSelect: document.getElementById("agentSelect"),
  commandSessionNewBtn: document.getElementById("commandSessionNewBtn"),
  commandSessionForkBtn: document.getElementById("commandSessionForkBtn"),
  commandSessionCompactBtn: document.getElementById("commandSessionCompactBtn"),
  commandSessionRenameBtn: document.getElementById("commandSessionRenameBtn"),
  commandSessionShareBtn: document.getElementById("commandSessionShareBtn"),
  commandSessionDeleteBtn: document.getElementById("commandSessionDeleteBtn"),
  commandSessionList: document.getElementById("commandSessionList"),
  composerForm: document.getElementById("composerForm"),
  composerInput: document.getElementById("composerInput"),
  sendButton: document.getElementById("sendButton"),
  statusBadge: document.getElementById("statusBadge"),
  tokenUsage: document.getElementById("tokenUsage"),
  agentBadge: document.getElementById("agentBadge"),
  heroGreeting: document.getElementById("heroGreeting"),
  chipActions: Array.from(document.querySelectorAll(".chip-action")),
};

function timeGreeting() {
  const hour = new Date().getHours();
  if (hour < 6) return "Good night";
  if (hour < 12) return "Good morning";
  if (hour < 18) return "Good afternoon";
  return "Good evening";
}

function escapeHtml(input) {
  return String(input)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\"/g, "&quot;");
}

function formatTime(ts) {
  if (!ts) return "--";
  const date = new Date(Number(ts));
  if (Number.isNaN(date.getTime())) return "--";
  return date.toLocaleString();
}

function short(text, max = 42) {
  if (!text) return "(untitled)";
  const clean = String(text).trim();
  if (clean.length <= max) return clean;
  return clean.slice(0, max - 1) + "...";
}

function baseName(path) {
  if (!path) return "workspace";
  const chunks = String(path).split(/[\\/]/).filter(Boolean);
  return chunks[chunks.length - 1] || "workspace";
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: {
      "Content-Type": "application/json",
      ...(options.headers || {}),
    },
    ...options,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${response.status} ${response.statusText}: ${text}`);
  }
  return response;
}

function projectKey(session) {
  if (session.project_id && session.project_id !== "default") return session.project_id;
  return session.directory || "default";
}

function projectLabel(session) {
  if (session.project_id && session.project_id !== "default") return baseName(session.project_id);
  return baseName(session.directory);
}

function normalizeSession(session) {
  return {
    id: session.id,
    title: session.title || "(untitled)",
    project_id: session.project_id || "default",
    directory: session.directory || "",
    updated: session.time && session.time.updated ? session.time.updated : Date.now(),
    share_url: session.share && session.share.url ? session.share.url : null,
    metadata: session.metadata || null,
  };
}

function sortSessions(items) {
  return items.sort((a, b) => Number(b.updated) - Number(a.updated));
}

function normalizeSessions(items) {
  return sortSessions((items || []).filter((s) => !s.parent_id).map(normalizeSession));
}

function upsertSessionSnapshot(session) {
  const normalized = normalizeSession(session);
  const index = state.sessions.findIndex((item) => item.id === normalized.id);
  if (index >= 0) {
    state.sessions[index] = { ...state.sessions[index], ...normalized };
  } else {
    state.sessions.push(normalized);
  }
  sortSessions(state.sessions);
  buildProjects();
  renderProjects();
  syncInteractionState();
  return normalized;
}

function buildProjects() {
  const map = new Map();
  for (const session of state.sessions) {
    const key = projectKey(session);
    if (!map.has(key)) {
      map.set(key, { key, label: projectLabel(session), sessions: [] });
    }
    map.get(key).sessions.push(session);
  }

  const query = (nodes.projectSearch.value || "").trim().toLowerCase();
  state.projects = Array.from(map.values())
    .map((project) => {
      if (!query) return project;
      const byProject = project.label.toLowerCase().includes(query);
      if (byProject) return project;
      const sessions = project.sessions.filter((s) => s.title.toLowerCase().includes(query));
      return { ...project, sessions };
    })
    .filter((project) => project.sessions.length > 0)
    .sort((a, b) => Number(b.sessions[0].updated) - Number(a.sessions[0].updated));
}

function selectedMode() {
  return state.modes.find((mode) => mode.key === state.selectedModeKey) || null;
}

function selectedModeLabel() {
  const mode = selectedMode();
  if (!mode) return "auto";
  return mode.kind === "agent" ? mode.name : `${mode.kind}:${mode.name}`;
}

function updateModeBadge() {
  if (!nodes.agentBadge) return;
  nodes.agentBadge.textContent = `mode: ${selectedModeLabel()}`;
}

function setSelectedMode(modeKey) {
  state.selectedModeKey = modeKey && String(modeKey).trim() ? String(modeKey).trim() : null;
  localStorage.setItem("rocode_web_mode", state.selectedModeKey || "auto");
  if (nodes.agentSelect) {
    nodes.agentSelect.value = state.selectedModeKey || "";
  }
  updateModeBadge();
}

function sessionMetaEntries(session) {
  const metadata = (session && session.metadata) || {};
  const entries = [];

  if (metadata.agent) {
    entries.push({ label: "agent", value: String(metadata.agent) });
  }

  const provider = metadata.model_provider ? String(metadata.model_provider) : "";
  const modelId = metadata.model_id ? String(metadata.model_id) : "";
  if (provider && modelId) {
    entries.push({ label: "model", value: `${provider}/${modelId}` });
  } else if (modelId) {
    entries.push({ label: "model", value: modelId });
  }

  if (metadata.scheduler_applied === true) {
    let schedulerValue = metadata.scheduler_profile ? String(metadata.scheduler_profile) : "active";
    if (metadata.scheduler_root_agent) {
      schedulerValue += ` · root=${String(metadata.scheduler_root_agent)}`;
    }
    if (metadata.scheduler_skill_tree_applied === true) {
      schedulerValue += " · skill-tree";
    }
    entries.push({ label: "scheduler", value: schedulerValue });
  }

  return entries;
}

function renderMetaPills(entries) {
  return entries
    .map(
      (entry) =>
        `<span class="meta-pill"><span class="meta-label">${escapeHtml(entry.label)}</span><span>${escapeHtml(entry.value)}</span></span>`,
    )
    .join("");
}

function updateSessionMeta(session) {
  if (!nodes.sessionMeta) return;
  const entries = sessionMetaEntries(session);
  nodes.sessionMeta.innerHTML = renderMetaPills(entries);
}

function runtimeBadgeText(session) {
  const entries = sessionMetaEntries(session);
  if (entries.length === 0) return "Running...";
  const summary = entries
    .slice(0, 2)
    .map((entry) => `${entry.label}:${entry.value}`)
    .join(" · ");
  return `Running · ${summary}`;
}

async function refreshSessionSnapshot(sessionId = state.selectedSession) {
  if (!sessionId) return null;
  const response = await api(`/session/${sessionId}`);
  const session = upsertSessionSnapshot(await response.json());
  if (session.id === state.selectedSession) {
    updateSessionMeta(session);
  }
  return session;
}

function setBadge(text, tone = "idle") {
  nodes.statusBadge.textContent = text;
  nodes.statusBadge.className = "badge";
  if (tone === "ok") nodes.statusBadge.classList.add("ok");
  if (tone === "warn") nodes.statusBadge.classList.add("warn");
  if (tone === "error") nodes.statusBadge.classList.add("error");
}

function updateTokenUsage() {
  nodes.tokenUsage.textContent = `tokens: ${state.promptTokens} / ${state.completionTokens}`;
}

function applyTheme(themeId) {
  const valid = THEMES.some((t) => t.id === themeId) ? themeId : "midnight";
  state.selectedTheme = valid;
  nodes.shell.dataset.theme = valid;
  localStorage.setItem("rocode_web_theme", valid);
  if (nodes.themeSelect.value !== valid) {
    nodes.themeSelect.value = valid;
  }
  applyOutputBlock({
    kind: "status",
    tone: "success",
    text: `Theme switched to ${valid}`,
    silent: true,
  });
}

function updatePanels() {
  const hasSession = Boolean(state.selectedSession);
  nodes.heroPanel.classList.toggle("hidden", hasSession);
  nodes.conversationPanel.classList.toggle("hidden", !hasSession);
}

function interactionLocked() {
  return state.streaming || Boolean(state.busyAction);
}

function toneForBadge(tone) {
  if (tone === "error") return "error";
  if (tone === "warning") return "warn";
  if (tone === "success") return "ok";
  return "warn";
}

function toneForMessage(tone) {
  if (tone === "error") return "error";
  if (tone === "warning") return "warning";
  if (tone === "success") return "success";
  return "normal";
}

function updateSessionControls() {
  const current = currentSession();
  const disabled = !current || interactionLocked();
  nodes.sendButton.disabled = interactionLocked();
  nodes.refreshSession.disabled = interactionLocked();
  nodes.newSessionBtn.disabled = interactionLocked();
  nodes.commandBtn.disabled = interactionLocked();
  nodes.forkSessionBtn.disabled = disabled;
  nodes.compactSessionBtn.disabled = disabled;
  nodes.renameSessionBtn.disabled = disabled;
  nodes.shareSessionBtn.disabled = disabled;
  nodes.deleteSessionBtn.disabled = disabled;
  if (disabled) {
    nodes.shareSessionBtn.textContent = "Share";
    return;
  }
  nodes.shareSessionBtn.textContent = current.share_url ? "Unshare" : "Share";
}

function updateCommandActionControls() {
  const current = currentSession();
  const locked = interactionLocked();
  const disabled = !current || locked;

  nodes.modelSelect.disabled = locked;
  nodes.themeSelect.disabled = locked;
  nodes.agentSelect.disabled = locked;
  nodes.commandSessionNewBtn.disabled = locked;
  nodes.commandSessionForkBtn.disabled = disabled;
  nodes.commandSessionCompactBtn.disabled = disabled;
  nodes.commandSessionRenameBtn.disabled = disabled;
  nodes.commandSessionShareBtn.disabled = disabled;
  nodes.commandSessionDeleteBtn.disabled = disabled;
  nodes.commandSessionShareBtn.textContent = current && current.share_url ? "Unshare" : "Share";
}

function renderProjects() {
  nodes.projectTree.innerHTML = "";
  nodes.projectCount.textContent = String(state.projects.length);
  const locked = interactionLocked();

  if (state.projects.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "No sessions yet. Send your first prompt.";
    nodes.projectTree.appendChild(empty);
    return;
  }

  for (const project of state.projects) {
    const card = document.createElement("article");
    card.className = "project-card";

    const trigger = document.createElement("button");
    trigger.className = "project-trigger";
    trigger.disabled = locked;
    if (project.key === state.selectedProject) trigger.classList.add("active");
    trigger.innerHTML = `
      <span class="project-name">${escapeHtml(project.label)}</span>
      <span class="project-meta">${project.sessions.length}</span>
    `;
    trigger.addEventListener("click", () => {
      state.selectedProject = project.key;
      if (!state.selectedSession && project.sessions.length > 0) {
        state.selectedSession = project.sessions[0].id;
      }
      renderProjects();
      void loadMessages();
      if (window.innerWidth <= 980) nodes.shell.classList.remove("sidebar-open");
    });
    card.appendChild(trigger);

    if (project.key === state.selectedProject) {
      const sessionList = document.createElement("div");
      sessionList.className = "session-list";
      for (const session of project.sessions) {
        const button = document.createElement("button");
        button.className = "session-btn";
        button.disabled = locked;
        if (session.id === state.selectedSession) button.classList.add("active");
        const runtimeEntries = sessionMetaEntries(session).slice(0, 2);
        button.innerHTML = `
          <span class="session-title">${escapeHtml(short(session.title, 34))}</span>
          <span class="session-date">${escapeHtml(formatTime(session.updated))}</span>
          ${runtimeEntries.length ? `<span class="session-runtime">${renderMetaPills(runtimeEntries)}</span>` : ""}
        `;
        button.addEventListener("click", () => {
          state.selectedSession = session.id;
          renderProjects();
          void loadMessages();
          renderCommandSessionList();
          if (window.innerWidth <= 980) nodes.shell.classList.remove("sidebar-open");
        });
        sessionList.appendChild(button);
      }
      card.appendChild(sessionList);
    }

    nodes.projectTree.appendChild(card);
  }

  updateSessionControls();
  updateCommandActionControls();
}

function clearFeed() {
  nodes.messageFeed.innerHTML = "";
  state.streamMessageNode = null;
  state.streamToolBlocks.clear();
}

function appendMessage(role, text, ts, options = {}) {
  const article = document.createElement("article");
  article.className = `message ${role}`;
  if (options.tone) article.classList.add(options.tone);

  const title = options.title || role;
  const body = text && text.trim().length > 0 ? text : "(empty)";

  const meta = document.createElement("div");
  meta.className = "message-meta";

  const titleNode = document.createElement("span");
  titleNode.textContent = title;
  meta.appendChild(titleNode);

  const timeNode = document.createElement("span");
  timeNode.textContent = formatTime(ts || Date.now());
  meta.appendChild(timeNode);

  const bodyNode = document.createElement("pre");
  bodyNode.className = "message-text";
  bodyNode.textContent = body;

  article.appendChild(meta);
  article.appendChild(bodyNode);
  nodes.messageFeed.appendChild(article);
  nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
  return { article, titleNode, timeNode, bodyNode };
}

function applyOutputBlock(block) {
  if (!block || !block.kind) return;

  if (block.kind === "status") {
    const tone = toneForMessage(block.tone || "normal");
    setBadge(block.text || "status", toneForBadge(tone));
    if (!block.silent) {
      appendMessage("status", block.text || "status", Date.now(), {
        title: `status · ${tone}`,
        tone,
      });
    }
    return;
  }

  if (block.kind === "message") {
    if (block.phase === "start") {
      state.streamMessageNode = appendMessage(block.role || "assistant", "", Date.now(), {
        title: block.role || "assistant",
      }).bodyNode;
      return;
    }
    if (block.phase === "delta") {
      if (!state.streamMessageNode) {
        state.streamMessageNode = appendMessage(block.role || "assistant", "", Date.now(), {
          title: block.role || "assistant",
        }).bodyNode;
      }
      state.streamMessageNode.textContent += block.text || "";
      nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
      return;
    }
    if (block.phase === "end") {
      state.streamMessageNode = null;
      return;
    }
    if (block.phase === "full") {
      appendMessage(block.role || "assistant", block.text || "", block.ts || Date.now(), {
        title: block.title || (block.role || "assistant"),
      });
    }
    return;
  }

  if (block.kind === "tool") {
    const phase = block.phase || "start";
    const key = block.id || block.name || `tool-${Date.now()}`;
    const tone = phase === "error" ? "error" : phase === "done" || phase === "result" ? "success" : "warning";
    const title = `${block.name || "tool"} · ${phase}`;
    const body = block.detail ? `${block.name || "tool"}\n${block.detail}` : block.name || "tool";

    let entry = state.streamToolBlocks.get(key);
    if (!entry) {
      entry = appendMessage("tool", body, Date.now(), { title, tone });
      state.streamToolBlocks.set(key, entry);
    } else {
      entry.article.classList.remove("warning", "success", "error");
      entry.article.classList.add(tone);
      entry.titleNode.textContent = title;
      entry.timeNode.textContent = formatTime(Date.now());
      entry.bodyNode.textContent = body;
      nodes.messageFeed.scrollTop = nodes.messageFeed.scrollHeight;
    }

    if (phase === "done" || phase === "result" || phase === "error") {
      state.streamToolBlocks.delete(key);
    }
  }
}

function messageBodyFromParts(parts) {
  if (!Array.isArray(parts) || parts.length === 0) return "";
  const out = [];
  for (const part of parts) {
    const type = part.type;
    if ((type === "text" || type === "reasoning" || type === "compaction") && part.text) {
      out.push(part.text);
    } else if (type === "tool_call" && part.tool_call) {
      out.push(`[tool] ${part.tool_call.name}`);
    } else if (type === "tool_result" && part.tool_result) {
      out.push(`[result] ${part.tool_result.content}`);
    }
  }
  return out.join("\n").trim();
}

function renderModelOptions() {
  nodes.modelSelect.innerHTML = "";
  const refs = [];
  for (const provider of state.providers) {
    for (const model of provider.models || []) {
      refs.push(`${provider.id}/${model.id}`);
    }
  }
  refs.sort((a, b) => a.localeCompare(b));

  for (const ref of refs) {
    const option = document.createElement("option");
    option.value = ref;
    option.textContent = ref;
    nodes.modelSelect.appendChild(option);
  }

  if (!state.selectedModel && refs.length > 0) {
    state.selectedModel = refs[0];
  }
  if (state.selectedModel) {
    nodes.modelSelect.value = state.selectedModel;
  }
}

function renderModeOptions() {
  if (!nodes.agentSelect) return;
  nodes.agentSelect.innerHTML = "";

  const autoOption = document.createElement("option");
  autoOption.value = "";
  autoOption.textContent = "auto";
  nodes.agentSelect.appendChild(autoOption);

  for (const mode of state.modes) {
    const option = document.createElement("option");
    option.value = mode.key;
    const kind = mode.kind ? ` [${mode.kind}]` : "";
    const detail = mode.mode ? ` · ${mode.mode}` : mode.orchestrator ? ` · ${mode.orchestrator}` : "";
    option.textContent = `${mode.name}${kind}${detail}`;
    nodes.agentSelect.appendChild(option);
  }

  nodes.agentSelect.value = state.selectedModeKey || "";
}

function renderThemeOptions() {
  nodes.themeSelect.innerHTML = "";
  for (const theme of THEMES) {
    const option = document.createElement("option");
    option.value = theme.id;
    option.textContent = theme.label;
    nodes.themeSelect.appendChild(option);
  }
  nodes.themeSelect.value = state.selectedTheme;
}

function renderCommandSessionList() {
  nodes.commandSessionList.innerHTML = "";
  const locked = interactionLocked();
  if (state.sessions.length === 0) {
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = "No sessions";
    nodes.commandSessionList.appendChild(p);
    return;
  }

  for (const session of state.sessions.slice(0, 40)) {
    const button = document.createElement("button");
    button.className = "command-session-btn";
    if (session.id === state.selectedSession) button.classList.add("active");
    button.disabled = locked;
    button.innerHTML = `${escapeHtml(short(session.title, 58))}<br><span class="muted">${escapeHtml(session.id)}</span>`;
    button.addEventListener("click", () => {
      state.selectedSession = session.id;
      state.selectedProject = projectKey(session);
      closeCommandPanel();
      renderProjects();
      void loadMessages();
    });
    nodes.commandSessionList.appendChild(button);
  }
}

function openCommandPanel(section) {
  renderModelOptions();
  renderThemeOptions();
  renderModeOptions();
  renderCommandSessionList();
  updateCommandActionControls();
  nodes.commandPanel.classList.remove("hidden");

  if (section === "model") nodes.modelSelect.focus();
  else if (section === "theme") nodes.themeSelect.focus();
  else if (section === "mode" || section === "agent") nodes.agentSelect.focus();
  else if (section === "session") {
    const first = nodes.commandSessionList.querySelector("button");
    if (first) first.focus();
  }
}

function closeCommandPanel() {
  nodes.commandPanel.classList.add("hidden");
}

function resolveSessionFromArg(arg) {
  const trimmed = arg.trim().toLowerCase();
  if (!trimmed) return null;
  let found = state.sessions.find((s) => s.id.toLowerCase() === trimmed);
  if (found) return found;
  found = state.sessions.find((s) => s.id.toLowerCase().startsWith(trimmed));
  if (found) return found;
  found = state.sessions.find((s) => s.title.toLowerCase().includes(trimmed));
  return found || null;
}

async function handleSlashCommand(input) {
  const trimmed = input.trim();
  if (!trimmed.startsWith("/")) return false;

  const body = trimmed.slice(1).trim();
  if (!body) return false;
  const [nameRaw, ...rest] = body.split(/\s+/);
  const name = nameRaw.toLowerCase();
  const arg = rest.join(" ").trim();

  if (interactionLocked() && name !== "help" && name !== "commands") {
    applyOutputBlock({
      kind: "status",
      tone: "warning",
      text: "Another action is running. Wait until it finishes.",
    });
    return true;
  }

  if (name === "help" || name === "commands") {
    applyOutputBlock({
      kind: "message",
      phase: "full",
      role: "system",
      title: "commands",
      text: [
        "/model <provider/model>   set active model",
        "/theme <midnight|graphite|sunset|daylight>   switch theme",
        "/mode <name|kind:name|auto>   set active mode",
        "/session <id|list|new|fork|compact|delete>   manage session",
      ].join("\n"),
    });
    return true;
  }

  if (name === "model") {
    if (!arg) {
      openCommandPanel("model");
      return true;
    }
    const ok = Array.from(nodes.modelSelect.options).some((opt) => opt.value === arg);
    if (!ok) {
      applyOutputBlock({ kind: "status", tone: "error", text: `Unknown model: ${arg}` });
      return true;
    }
    state.selectedModel = arg;
    nodes.modelSelect.value = arg;
    applyOutputBlock({ kind: "status", tone: "success", text: `Model set to ${arg}` });
    return true;
  }

  if (name === "theme") {
    if (!arg) {
      openCommandPanel("theme");
      return true;
    }
    applyTheme(arg);
    return true;
  }

  if (name === "mode" || name === "agent") {
    if (!arg) {
      openCommandPanel("mode");
      return true;
    }
    if (arg === "auto") {
      setSelectedMode(null);
      applyOutputBlock({ kind: "status", tone: "success", text: "Mode set to auto" });
      return true;
    }
    const lowerArg = arg.toLowerCase();
    const found = state.modes.find((mode) => {
      if (mode.key.toLowerCase() === lowerArg) return true;
      if (mode.name.toLowerCase() === lowerArg) return true;
      return `${mode.kind}:${mode.name}`.toLowerCase() === lowerArg;
    });
    if (!found) {
      applyOutputBlock({ kind: "status", tone: "error", text: `Unknown mode: ${arg}` });
      return true;
    }
    setSelectedMode(found.key);
    applyOutputBlock({ kind: "status", tone: "success", text: `Mode set to ${selectedModeLabel()}` });
    return true;
  }

  if (name === "session" || name === "sessions") {
    if (!arg || arg === "list") {
      openCommandPanel("session");
      return true;
    }
    if (arg === "new") {
      await runUiAction("creating session", async () => {
        await createAndSelectSession();
      });
      return true;
    }
    if (arg === "fork") {
      await runUiAction("forking session", async () => {
        await forkCurrentSession();
      });
      return true;
    }
    if (arg === "compact") {
      await runUiAction("compacting session", async () => {
        await compactCurrentSession();
      });
      return true;
    }
    if (arg === "delete") {
      await runUiAction("deleting session", async () => {
        await deleteCurrentSession();
      });
      return true;
    }

    const resolved = resolveSessionFromArg(arg);
    if (!resolved) {
      applyOutputBlock({ kind: "status", tone: "error", text: `Session not found: ${arg}` });
      return true;
    }

    state.selectedSession = resolved.id;
    state.selectedProject = projectKey(resolved);
    renderProjects();
    await loadMessages();
    applyOutputBlock({ kind: "status", tone: "success", text: `Session switched: ${resolved.id}` });
    return true;
  }

  return false;
}

async function loadProviders() {
  try {
    const response = await api("/provider");
    const data = await response.json();
    state.providers = data.all || [];

    if (!state.selectedModel && data.default) {
      const providers = Object.keys(data.default);
      if (providers.length > 0) {
        const p = providers[0];
        state.selectedModel = `${p}/${data.default[p]}`;
      }
    }

    renderModelOptions();
  } catch (error) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load providers: ${String(error)}` });
  }
}

async function loadModes() {
  try {
    const response = await api("/mode");
    const data = await response.json();
    state.modes = (data || [])
      .filter((mode) => mode.hidden !== true)
      .filter((mode) => mode.kind !== "agent" || mode.mode !== "subagent")
      .map((mode) => ({
        key: `${mode.kind}:${mode.id}`,
        id: mode.id,
        name: mode.name,
        kind: mode.kind || "agent",
        description: mode.description || "",
        mode: mode.mode || null,
        orchestrator: mode.orchestrator || null,
      }));

    if (state.selectedModeKey) {
      const found = state.modes.some((mode) => mode.key === state.selectedModeKey);
      if (!found) {
        setSelectedMode(null);
      }
    }

    renderModeOptions();
  } catch (error) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load modes: ${String(error)}` });
  }
}

async function loadMessages() {
  if (!state.selectedSession) {
    updatePanels();
    syncInteractionState();
    return;
  }

  updatePanels();
  syncInteractionState();

  try {
    const response = await api(`/session/${state.selectedSession}/message`);
    const messages = await response.json();
    clearFeed();

    for (const message of messages) {
      const body = messageBodyFromParts(message.parts);
      applyOutputBlock({
        kind: "message",
        phase: "full",
        role: message.role || "assistant",
        title: `${message.role || "assistant"}${message.model ? ` · ${message.model}` : ""}`,
        text: body,
        ts: message.created_at,
      });
    }

    const current = state.sessions.find((s) => s.id === state.selectedSession);
    nodes.sessionTitle.textContent = current ? short(current.title, 56) : state.selectedSession;
    updateSessionMeta(current);
  } catch (error) {
    clearFeed();
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load messages: ${String(error)}` });
  }
}

async function loadSessions() {
  try {
    const response = await api("/session?roots=true&limit=120");
    state.sessions = normalizeSessions(await response.json());
    buildProjects();

    if (!state.selectedProject && state.projects.length > 0) {
      state.selectedProject = state.projects[0].key;
    }

    if (!state.selectedSession) {
      const currentProject = state.projects.find((p) => p.key === state.selectedProject);
      if (currentProject && currentProject.sessions.length > 0) {
        state.selectedSession = currentProject.sessions[0].id;
      }
    }

    renderProjects();
    updateSessionMeta(currentSession());
    syncInteractionState();
    await loadMessages();
  } catch (error) {
    setBadge("offline", "error");
    clearFeed();
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load sessions: ${String(error)}` });
  }
}

async function createAndSelectSession() {
  const response = await api("/session", {
    method: "POST",
    body: JSON.stringify({}),
  });
  const created = await response.json();
  state.selectedSession = created.id;
  state.selectedProject = projectKey(created);
  await loadSessions();
  applyOutputBlock({ kind: "status", tone: "success", text: `Session created: ${created.id}` });
  return created.id;
}

function currentSession() {
  return state.sessions.find((s) => s.id === state.selectedSession) || null;
}

function syncInteractionState() {
  updateSessionControls();
  updateCommandActionControls();
  renderCommandSessionList();
}

async function runUiAction(label, task) {
  if (interactionLocked()) return null;

  const runningBadgeText = `${label}...`;
  state.busyAction = label;
  setBadge(runningBadgeText, "warn");
  syncInteractionState();

  try {
    return await task();
  } catch (error) {
    applyOutputBlock({
      kind: "status",
      tone: "error",
      text: `${label} failed: ${String(error)}`,
    });
    return null;
  } finally {
    state.busyAction = null;
    syncInteractionState();
    if (!state.streaming && nodes.statusBadge.textContent === runningBadgeText) {
      setBadge("ready", "ok");
    }
  }
}

async function renameCurrentSession() {
  if (!state.selectedSession) return;
  const current = currentSession();
  const nextTitle = prompt("Rename session", current ? current.title : "");
  if (!nextTitle || !nextTitle.trim()) return;

  await api(`/session/${state.selectedSession}/title`, {
    method: "PATCH",
    body: JSON.stringify({ title: nextTitle.trim() }),
  });

  await loadSessions();
  await loadMessages();
  applyOutputBlock({ kind: "status", tone: "success", text: "Session renamed" });
}

async function toggleShareCurrentSession() {
  if (!state.selectedSession) return;
  const current = currentSession();
  if (!current) return;

  if (current.share_url) {
    await api(`/session/${state.selectedSession}/share`, { method: "DELETE" });
    applyOutputBlock({ kind: "status", tone: "success", text: "Session unshared" });
  } else {
    const response = await api(`/session/${state.selectedSession}/share`, { method: "POST" });
    const data = await response.json();
    if (data && data.url && navigator.clipboard && navigator.clipboard.writeText) {
      try {
        await navigator.clipboard.writeText(data.url);
      } catch (_) {
        // ignore clipboard failures
      }
    }
    applyOutputBlock({
      kind: "status",
      tone: "success",
      text: data && data.url ? `Share link: ${data.url}` : "Session shared",
    });
  }

  await loadSessions();
  await loadMessages();
}

async function forkCurrentSession() {
  if (!state.selectedSession) return;
  const response = await api(`/session/${state.selectedSession}/fork`, {
    method: "POST",
    body: JSON.stringify({ message_id: null }),
  });
  const forked = await response.json();
  state.selectedSession = forked.id;
  state.selectedProject = projectKey(forked);
  await loadSessions();
  await loadMessages();
  applyOutputBlock({ kind: "status", tone: "success", text: `Forked session: ${forked.id}` });
}

async function compactCurrentSession() {
  if (!state.selectedSession) return;
  await api(`/session/${state.selectedSession}/compaction`, {
    method: "POST",
  });
  applyOutputBlock({
    kind: "status",
    tone: "warning",
    text: "Compaction started",
  });
  await loadSessions();
  await loadMessages();
}

async function deleteCurrentSession() {
  if (!state.selectedSession) return;
  const current = currentSession();
  const title = current ? short(current.title, 48) : state.selectedSession;
  const confirmed = confirm(`Delete session \"${title}\"? This cannot be undone.`);
  if (!confirmed) return;

  const deleteId = state.selectedSession;
  await api(`/session/${deleteId}`, { method: "DELETE" });

  state.selectedSession = null;
  const remaining = state.sessions.filter((s) => s.id !== deleteId);
  if (remaining.length > 0) {
    state.selectedSession = remaining[0].id;
    state.selectedProject = projectKey(remaining[0]);
  }

  await loadSessions();
  await loadMessages();
  applyOutputBlock({ kind: "status", tone: "success", text: "Session deleted" });
}

async function parseSSE(response, onEvent) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let eventName = null;
  let dataLines = [];

  const flush = () => {
    if (dataLines.length === 0) {
      eventName = null;
      return;
    }
    const data = dataLines.join("\n");
    dataLines = [];

    let parsed;
    try {
      parsed = JSON.parse(data);
    } catch (_) {
      parsed = { raw: data };
    }
    onEvent(eventName || "message", parsed);
    eventName = null;
  };

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";

    for (const lineRaw of lines) {
      const line = lineRaw.endsWith("\r") ? lineRaw.slice(0, -1) : lineRaw;
      if (!line) {
        flush();
        continue;
      }
      if (line.startsWith("event:")) {
        eventName = line.slice(6).trim();
      } else if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trimStart());
      }
    }
  }

  flush();
}

async function sendPrompt(content) {
  if (!content || interactionLocked()) return;

  state.streaming = true;
  syncInteractionState();
  applyOutputBlock({ kind: "status", tone: "warning", text: "Running...", silent: true });

  try {
    if (!state.selectedSession) {
      await createAndSelectSession();
    }

    applyOutputBlock({ kind: "message", phase: "full", role: "user", title: "you", text: content });

    const mode = selectedMode();
    const payload = {
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

    try {
      const snapshot = await refreshSessionSnapshot(state.selectedSession);
      setBadge(runtimeBadgeText(snapshot), "warn");
    } catch (_) {
      setBadge("Running...", "warn");
    }

    await parseSSE(response, (name, payload) => {
      if (name === "output_block") {
        applyOutputBlock(payload);
        return;
      }

      if (name === "usage") {
        state.promptTokens = payload.prompt_tokens || state.promptTokens;
        state.completionTokens = payload.completion_tokens || state.completionTokens;
        updateTokenUsage();
      } else if (name === "error") {
        applyOutputBlock({ kind: "status", tone: "error", text: payload.error || "Stream error" });
      }
    });

    applyOutputBlock({ kind: "status", tone: "success", text: "Done.", silent: true });
    await loadSessions();
  } catch (error) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Send failed: ${String(error)}` });
  } finally {
    state.streaming = false;
    syncInteractionState();
  }
}

function autoSizeInput() {
  nodes.composerInput.style.height = "auto";
  nodes.composerInput.style.height = `${Math.min(nodes.composerInput.scrollHeight, 140)}px`;
}

function wireEvents() {
  nodes.sidebarToggle.addEventListener("click", () => {
    nodes.shell.classList.toggle("sidebar-open");
  });

  nodes.projectSearch.addEventListener("input", () => {
    buildProjects();
    renderProjects();
  });

  nodes.refreshSession.addEventListener("click", () => {
    void loadMessages();
  });

  nodes.newSessionBtn.addEventListener("click", () => {
    void runUiAction("creating session", async () => {
      await createAndSelectSession();
    });
  });

  nodes.forkSessionBtn.addEventListener("click", () => {
    void runUiAction("forking session", async () => {
      await forkCurrentSession();
    });
  });

  nodes.compactSessionBtn.addEventListener("click", () => {
    void runUiAction("compacting session", async () => {
      await compactCurrentSession();
    });
  });

  nodes.renameSessionBtn.addEventListener("click", () => {
    void runUiAction("renaming session", async () => {
      await renameCurrentSession();
    });
  });

  nodes.shareSessionBtn.addEventListener("click", () => {
    void runUiAction("sharing session", async () => {
      await toggleShareCurrentSession();
    });
  });

  nodes.deleteSessionBtn.addEventListener("click", () => {
    void runUiAction("deleting session", async () => {
      await deleteCurrentSession();
    });
  });

  nodes.commandBtn.addEventListener("click", () => {
    openCommandPanel("model");
  });

  nodes.commandClose.addEventListener("click", closeCommandPanel);
  nodes.commandPanel.addEventListener("click", (event) => {
    if (event.target === nodes.commandPanel) {
      closeCommandPanel();
    }
  });

  nodes.modelSelect.addEventListener("change", () => {
    state.selectedModel = nodes.modelSelect.value;
    applyOutputBlock({ kind: "status", tone: "success", text: `Model set to ${state.selectedModel}`, silent: true });
  });

  nodes.agentSelect.addEventListener("change", () => {
    setSelectedMode(nodes.agentSelect.value || null);
    applyOutputBlock({ kind: "status", tone: "success", text: `Mode set to ${selectedModeLabel()}`, silent: true });
  });

  nodes.themeSelect.addEventListener("change", () => {
    applyTheme(nodes.themeSelect.value);
  });

  nodes.commandSessionNewBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("creating session", async () => {
      await createAndSelectSession();
    });
  });

  nodes.commandSessionForkBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("forking session", async () => {
      await forkCurrentSession();
    });
  });

  nodes.commandSessionCompactBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("compacting session", async () => {
      await compactCurrentSession();
    });
  });

  nodes.commandSessionRenameBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("renaming session", async () => {
      await renameCurrentSession();
    });
  });

  nodes.commandSessionShareBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("sharing session", async () => {
      await toggleShareCurrentSession();
    });
  });

  nodes.commandSessionDeleteBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("deleting session", async () => {
      await deleteCurrentSession();
    });
  });

  nodes.composerForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const content = nodes.composerInput.value.trim();
    nodes.composerInput.value = "";
    autoSizeInput();
    if (!content) return;

    if (content.startsWith("/")) {
      const handled = await handleSlashCommand(content);
      if (handled) return;
    }

    await sendPrompt(content);
  });

  nodes.composerInput.addEventListener("input", autoSizeInput);
  nodes.composerInput.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !nodes.commandPanel.classList.contains("hidden")) {
      closeCommandPanel();
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      nodes.composerForm.requestSubmit();
    }
  });

  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      openCommandPanel("model");
    }
    if (event.key === "Escape" && !nodes.commandPanel.classList.contains("hidden")) {
      closeCommandPanel();
    }
  });

  for (const chip of nodes.chipActions) {
    chip.addEventListener("click", () => {
      nodes.composerInput.value = chip.dataset.template || "";
      autoSizeInput();
      nodes.composerInput.focus();
    });
  }
}

async function bootstrap() {
  nodes.heroGreeting.textContent = timeGreeting();
  applyTheme(state.selectedTheme);
  updateTokenUsage();
  setBadge("loading", "warn");
  wireEvents();
  autoSizeInput();
  renderThemeOptions();
  syncInteractionState();

  await Promise.all([loadProviders(), loadModes(), loadSessions()]);

  if (!state.streaming) {
    setBadge("ready", "ok");
  }
}

void bootstrap();
