// ── Settings Panel ─────────────────────────────────────────────────────────

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

  if (canAbortCurrentExecution() && nodes.commandAbortBtn) {
    nodes.commandAbortBtn.focus();
  } else if (section === "model") nodes.modelSelect.focus();
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
