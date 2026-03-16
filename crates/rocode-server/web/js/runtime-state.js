// ── Runtime State & UI Chrome ───────────────────────────────────────────────

function selectedMode() {
  return state.modes.find((mode) => mode.key === state.selectedModeKey) || null;
}

function selectedModeLabel() {
  const mode = selectedMode();
  if (!mode) return "auto";
  return mode.kind === "agent" ? mode.name : `${mode.kind}:${mode.name}`;
}

function setMetaBadge(node, label, value) {
  if (!node) return;
  node.innerHTML = `<span class="meta-label">${escapeHtml(label)}</span><span>${escapeHtml(value)}</span>`;
}

function metadataModelLabel(metadata = {}) {
  const provider = metadata.model_provider ? String(metadata.model_provider) : "";
  const modelId = metadata.model_id ? String(metadata.model_id) : "";
  if (provider && modelId) return `${provider}/${modelId}`;
  if (modelId) return modelId;
  return null;
}

function sessionModeLabel(session) {
  const metadata = (session && session.metadata) || {};
  if (metadata.scheduler_profile) return `preset:${String(metadata.scheduler_profile)}`;
  if (metadata.agent) return `agent:${String(metadata.agent)}`;
  return selectedModeLabel();
}

function sessionModelLabel(session) {
  const metadata = (session && session.metadata) || {};
  return metadataModelLabel(metadata) || state.selectedModel || "auto";
}

function sessionDirectoryLabel(session) {
  return compactPath(session && session.directory ? session.directory : "");
}

function updateComposerMeta() {
  const current = currentSession();
  setMetaBadge(nodes.modeBadge, "mode", selectedModeLabel());
  setMetaBadge(nodes.modelBadge, "model", state.selectedModel || "auto");
  setMetaBadge(nodes.directoryBadge, "directory", sessionDirectoryLabel(current));
}

function updateSessionRuntimeMeta(session) {
  if (!nodes.sessionRuntimeMeta) return;
  const entries = [
    { label: "mode", value: sessionModeLabel(session) },
    { label: "model", value: sessionModelLabel(session) },
    { label: "directory", value: sessionDirectoryLabel(session) },
  ];
  if (state.executionTopology && state.executionTopology.active_count > 0) {
    entries.push({
      label: "execution",
      value: `${state.executionTopology.active_count} active · ${state.executionTopology.waiting_count} waiting`,
    });
  }
  nodes.sessionRuntimeMeta.innerHTML = renderMetaPills(entries);
}

function currentWebUiPreferencesPatch() {
  return {
    webTheme: state.selectedTheme || "midnight",
    webMode: state.selectedModeKey || null,
    showThinking: Boolean(state.showThinking),
  };
}

async function persistWebUiPreferences() {
  await api("/config", {
    method: "PATCH",
    body: JSON.stringify({
      uiPreferences: currentWebUiPreferencesPatch(),
    }),
  });
}

function applyWebUiPreferences(config = {}) {
  const ui = config && (config.uiPreferences || config.ui_preferences) ? (config.uiPreferences || config.ui_preferences) : {};
  const webTheme = ui.webTheme || ui.web_theme || null;
  const webMode = ui.webMode || ui.web_mode || null;
  const showThinking = ui.showThinking ?? ui.show_thinking ?? state.showThinking;

  if (webTheme) {
    applyTheme(webTheme, { persist: false, announce: false });
  } else {
    applyTheme(state.selectedTheme || "midnight", { persist: false, announce: false });
  }

  state.showThinking = Boolean(showThinking);
  setSelectedMode(webMode || null, { persist: false });
}

async function loadWebUiPreferences() {
  const response = await api("/config");
  const config = await response.json();
  applyWebUiPreferences(config);
  return config;
}

function setSelectedMode(modeKey, options = {}) {
  const { persist = true } = options;
  state.selectedModeKey = modeKey && String(modeKey).trim() ? String(modeKey).trim() : null;
  if (nodes.agentSelect) {
    nodes.agentSelect.value = state.selectedModeKey || "";
  }
  updateComposerMeta();
  updateSessionRuntimeMeta(currentSession());
  if (persist) {
    void persistWebUiPreferences().catch(() => {});
  }
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
  updateSessionRuntimeMeta(session);
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

function runningSchedulerMode(mode = selectedMode()) {
  return Boolean(mode && (mode.kind === "preset" || mode.kind === "profile"));
}

function canAbortCurrentExecution() {
  return Boolean(state.selectedSession) && state.streaming && !state.busyAction && !state.abortRequested;
}

function cancelActionLabel() {
  if (state.abortRequested) return "Cancelling…";
  return runningSchedulerMode() ? "Cancel Stage" : "Cancel Run";
}

function commandHintText() {
  if (state.abortRequested) {
    return "Cancellation requested…";
  }
  if (state.streaming) {
    return "Use /abort to cancel • Commands still available";
  }
  if (state.busyAction) {
    return "Another action is running";
  }
  const suggested = Array.isArray(state.uiCommands)
    ? state.uiCommands
        .filter((command) => command && command.slash && command.slash.suggested)
        .slice(0, 4)
        .map((command) => command.slash.name)
    : [];
  if (suggested.length > 0) {
    return `Use ${suggested.join(", ")}`;
  }
  return "Use /help, /agent, /preset, or /model";
}

function runtimeStatusLabel() {
  if (state.abortRequested) return "cancelling";
  if (state.streaming) return "running";
  if (state.busyAction) return state.busyAction;
  return "ready";
}

function runtimeStatusTone() {
  if (state.abortRequested) return "warn";
  if (state.streaming || state.busyAction) return "warn";
  return "ok";
}

function runtimeSummaryText() {
  const current = currentSession();
  if (state.abortRequested) {
    return runningSchedulerMode()
      ? "Scheduler stage cancellation requested"
      : "Run cancellation requested";
  }
  if (state.streaming) {
    return runtimeBadgeText(current);
  }
  if (state.busyAction) {
    return `${state.busyAction}…`;
  }
  if (state.executionTopology && state.executionTopology.active_count > 0) {
    return executionSummaryText(state.executionTopology);
  }
  if (!current) {
    return "Idle";
  }
  return `${sessionModeLabel(current)} · ${sessionModelLabel(current)}`;
}

function updateCommandHint() {
  if (!nodes.commandHint) return;
  nodes.commandHint.textContent = commandHintText();
}

function updateRuntimeChrome() {
  if (nodes.commandRuntimeBadge) {
    nodes.commandRuntimeBadge.textContent = runtimeStatusLabel();
    nodes.commandRuntimeBadge.className = "badge";
    nodes.commandRuntimeBadge.classList.add(runtimeStatusTone());
  }
  if (nodes.commandRuntimeText) {
    nodes.commandRuntimeText.textContent = runtimeSummaryText();
  }
  updateCommandHint();
}

function interactionLocked() {
  return state.streaming || Boolean(state.busyAction);
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

function applyStreamUsage(payload) {
  state.promptTokens = payload.prompt_tokens ?? state.promptTokens;
  state.completionTokens = payload.completion_tokens ?? state.completionTokens;
  updateTokenUsage();
}

function applyTheme(themeId, options = {}) {
  const { persist = true, announce = true } = options;
  const valid = THEMES.some((t) => t.id === themeId) ? themeId : "midnight";
  state.selectedTheme = valid;
  nodes.shell.dataset.theme = valid;
  if (nodes.themeSelect.value !== valid) {
    nodes.themeSelect.value = valid;
  }
  if (persist) {
    void persistWebUiPreferences().catch(() => {});
  }
  if (announce) {
    applyOutputBlock({
      kind: "status",
      tone: "success",
      text: `Theme switched to ${valid}`,
      silent: true,
    });
  }
}

function updatePanels() {
  const hasSession = Boolean(state.selectedSession);
  nodes.heroPanel.classList.toggle("hidden", hasSession);
  nodes.conversationPanel.classList.toggle("hidden", !hasSession);
  updateComposerMeta();
  updateRuntimeChrome();
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

function syncInteractionState() {
  updateSessionControls();
  updateCommandActionControls();
  renderCommandSessionList();
}
