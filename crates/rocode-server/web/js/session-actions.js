// ── Session Actions & Data Loading ──────────────────────────────────────────

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

async function abortCurrentExecution() {
  if (!state.selectedSession || !state.streaming || state.abortRequested) return;
  state.abortRequested = true;
  setBadge("cancelling", "warn");
  syncInteractionState();
  const path =
    runningSchedulerMode()
      ? `/session/${state.selectedSession}/scheduler/stage/abort`
      : `/session/${state.selectedSession}/abort`;
  try {
    const response = await api(path, { method: "POST" });
    const result = await response.json();
    const label =
      result && result.target === "stage"
        ? `Cancellation requested: ${result.stage || "current stage"}`
        : "Cancellation requested";
    applyOutputBlock({ kind: "status", tone: "warning", text: label, silent: true });
  } catch (error) {
    state.abortRequested = false;
    syncInteractionState();
    throw error;
  }
}

async function loadProviders() {
  try {
    const response = await api("/config/providers");
    const data = await response.json();
    state.providers = data.providers || data.all || [];
    const preferredRefs = [];
    if (data.default) {
      for (const [provider, model] of Object.entries(data.default)) {
        preferredRefs.push(`${provider}/${model}`);
      }
    }
    repairSelectedModel(collectModelRefs(), preferredRefs);

    renderModelOptions();
    updateComposerMeta();
    updateSessionRuntimeMeta(currentSession());
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
        setSelectedMode(null, { persist: false });
      }
    }

    renderModeOptions();
  } catch (error) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load modes: ${String(error)}` });
  }
}

async function loadMessages() {
  if (!state.selectedSession) {
    state.executionTopology = null;
    renderExecutionPanel(null);
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

    // Show "Back" button when viewing a child session
    if (state.parentSessionId) {
      const backBtn = document.createElement("button");
      backBtn.className = "back-to-parent";
      backBtn.textContent = "\u2190 Back to parent session";
      backBtn.addEventListener("click", () => {
        state.selectedSession = state.parentSessionId;
        state.parentSessionId = null;
        void loadMessages();
        renderProjects();
        syncInteractionState();
      });
      nodes.messageFeed.appendChild(backBtn);
    }

    for (const message of messages) {
      const body = messageBodyFromParts(message.parts);
      const historyBlocks = historyOutputBlocksFromParts(message.parts);
      const stageBlock = schedulerStageBlockFromMessage(message, body);
      if (stageBlock) {
        applyOutputBlock(stageBlock);
        // Do NOT continue — fall through to emit historyBlocks that may be
        // attached to the same message (tool results, session events, etc.).
      } else if (body) {
        applyOutputBlock({
          kind: "message",
          phase: "full",
          role: message.role || "assistant",
          title: `${message.role || "assistant"}${message.model ? ` · ${message.model}` : ""}`,
          text: body,
          ts: message.created_at,
        });
      }
      for (const block of historyBlocks) {
        applyOutputBlock(block);
      }
    }

    const current = state.sessions.find((s) => s.id === state.selectedSession);
    nodes.sessionTitle.textContent = current ? short(current.title, 56) : state.selectedSession;
    updateSessionMeta(current);
    updateComposerMeta();
    await refreshExecutionTopology(state.selectedSession);
  } catch (error) {
    clearFeed();
    applyOutputBlock({ kind: "status", tone: "error", text: `Failed to load messages: ${String(error)}` });
  }
}

async function refreshSessionsIndex() {
  const response = await api("/session?roots=true&limit=120");
  const selectedSession = state.selectedSession;
  const selectedProject = state.selectedProject;

  state.sessions = normalizeSessions(await response.json());
  buildProjects();

  if (selectedProject && state.projects.some((project) => project.key === selectedProject)) {
    state.selectedProject = selectedProject;
  } else if (state.projects.length > 0) {
    state.selectedProject = state.projects[0].key;
  } else {
    state.selectedProject = null;
  }

  if (selectedSession && state.sessions.some((session) => session.id === selectedSession)) {
    state.selectedSession = selectedSession;
  } else {
    const currentProject = state.projects.find((project) => project.key === state.selectedProject);
    state.selectedSession = currentProject && currentProject.sessions.length > 0
      ? currentProject.sessions[0].id
      : null;
  }

  if (state.parentSessionId && !state.sessions.some((session) => session.id === state.parentSessionId)) {
    state.parentSessionId = null;
  }

  renderProjects();
  updateSessionMeta(currentSession());
  updateComposerMeta();
  updatePanels();
  syncInteractionState();
}

async function loadSessions() {
  try {
    await refreshSessionsIndex();
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
