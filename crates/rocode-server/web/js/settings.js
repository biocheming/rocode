// ── Settings Panel ─────────────────────────────────────────────────────────

function collectModelRefs() {
  const refs = [];
  for (const provider of state.providers) {
    for (const model of provider.models || []) {
      refs.push(`${provider.id}/${model.id}`);
    }
  }
  refs.sort((a, b) => a.localeCompare(b));
  return refs;
}

function repairSelectedModel(refs, preferredRefs = []) {
  const available = new Set(refs);
  if (state.selectedModel && available.has(state.selectedModel)) {
    return;
  }

  const preferred = preferredRefs.find((ref) => available.has(ref));
  state.selectedModel = preferred || refs[0] || null;
}

function renderModelOptions() {
  nodes.modelSelect.innerHTML = "";
  const refs = collectModelRefs();
  repairSelectedModel(refs);

  for (const ref of refs) {
    const option = document.createElement("option");
    option.value = ref;
    option.textContent = ref;
    nodes.modelSelect.appendChild(option);
  }

  if (state.selectedModel && refs.includes(state.selectedModel)) {
    nodes.modelSelect.value = state.selectedModel;
  } else {
    nodes.modelSelect.value = "";
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

function renderCommandCatalog() {
  if (!nodes.commandCatalog) return;
  nodes.commandCatalog.innerHTML = "";

  const commands = Array.isArray(state.uiCommands) ? state.uiCommands : [];
  if (commands.length === 0) {
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = "No shared commands loaded";
    nodes.commandCatalog.appendChild(p);
    return;
  }

  for (const command of commands) {
    if (!command || !command.slash) continue;
    const item = document.createElement("div");
    item.className = "command-session-btn";
    const category = String(command.category || "")
      .split("_")
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ");
    const aliases = Array.isArray(command.slash.aliases) && command.slash.aliases.length > 0
      ? ` · ${command.slash.aliases.join(", ")}`
      : "";
    const keybind = command.keybind ? ` · ${command.keybind}` : "";
    const title = document.createElement("div");
    title.textContent = command.slash.name;
    const meta = document.createElement("span");
    meta.className = "muted";
    meta.textContent = `${command.title} · ${category}${aliases}${keybind}`;
    item.appendChild(title);
    item.appendChild(meta);
    nodes.commandCatalog.appendChild(item);
  }
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

function settingsTabPanels() {
  return {
    general: nodes.settingsGeneralPanel,
    providers: nodes.settingsProvidersPanel,
    scheduler: nodes.settingsSchedulerPanel,
    mcp: nodes.settingsMcpPanel,
    plugins: nodes.settingsPluginsPanel,
    lsp: nodes.settingsLspPanel,
    commands: nodes.settingsCommandsPanel,
    sessions: nodes.settingsSessionsPanel,
  };
}

function settingsTabFromSection(section) {
  switch (section) {
    case "provider":
      return "providers";
    case "scheduler":
    case "preset":
      return "scheduler";
    case "mcp":
      return "mcp";
    case "plugin":
    case "plugins":
      return "plugins";
    case "lsp":
    case "formatter":
      return "lsp";
    case "command":
      return "commands";
    case "session":
      return "sessions";
    default:
      return "general";
  }
}

function setSettingsTab(tab) {
  state.settingsActiveTab = tab;
  const panels = settingsTabPanels();
  for (const [key, panel] of Object.entries(panels)) {
    if (!panel) continue;
    panel.classList.toggle("hidden", key !== tab);
  }
  if (!nodes.settingsTabList) return;
  for (const button of nodes.settingsTabList.querySelectorAll("button")) {
    button.classList.toggle("active", button.dataset && button.dataset.settingsTab === tab);
  }
}

function currentProviderMap() {
  const config = state.configSnapshot || {};
  return config.provider || config.providers || {};
}

function currentMcpMap() {
  const config = state.configSnapshot || {};
  return config.mcp || {};
}

function currentPluginMap() {
  const config = state.configSnapshot || {};
  return config.plugin || {};
}

function currentLspConfig() {
  const config = state.configSnapshot || {};
  return config.lsp || null;
}

function currentFormatterConfig() {
  const config = state.configSnapshot || {};
  return config.formatter || null;
}

function sortedKeys(value) {
  return Object.keys(value || {}).sort((a, b) => a.localeCompare(b));
}

function ensureSettingsSelection() {
  const providerKeys = sortedKeys(currentProviderMap());
  if (!providerKeys.includes(state.settingsProviderSelection)) {
    state.settingsProviderSelection = providerKeys[0] || null;
  }

  const provider = state.settingsProviderSelection
    ? currentProviderMap()[state.settingsProviderSelection]
    : null;
  const modelKeys = sortedKeys((provider && provider.models) || {});
  if (!modelKeys.includes(state.settingsModelSelection)) {
    state.settingsModelSelection = modelKeys[0] || null;
  }

  const mcpKeys = sortedKeys(currentMcpMap());
  if (!mcpKeys.includes(state.settingsMcpSelection)) {
    state.settingsMcpSelection = mcpKeys[0] || null;
  }

  const pluginKeys = sortedKeys(currentPluginMap());
  if (!pluginKeys.includes(state.settingsPluginSelection)) {
    state.settingsPluginSelection = pluginKeys[0] || null;
  }
}

function setStatusNode(node, text, tone) {
  if (!node) return;
  node.className = tone === "error" ? "text-danger settings-status-flash is-error" : "text-tertiary settings-status-flash";
  if (tone === "success") {
    node.classList.add("is-success");
  }
  node.textContent = text;
  if (node._statusTimer) {
    clearTimeout(node._statusTimer);
  }
  node._statusTimer = setTimeout(() => {
    node.classList.remove("settings-status-flash", "is-success", "is-error");
    node._statusTimer = null;
  }, 900);
}

function setProviderStatus(text, tone) {
  setStatusNode(nodes.settingsProviderStatus, text, tone);
}

function setSchedulerStatus(text, tone) {
  setStatusNode(nodes.settingsSchedulerStatus, text, tone);
}

function setMcpStatus(text, tone) {
  setStatusNode(nodes.settingsMcpStatus, text, tone);
}

function setPluginStatus(text, tone) {
  setStatusNode(nodes.settingsPluginStatus, text, tone);
}

function setLspStatus(text, tone) {
  setStatusNode(nodes.settingsLspStatus, text, tone);
}

function readJsonTextarea(node, label) {
  const raw = String(node && node.value ? node.value : "").trim();
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new Error(`${label} must be valid JSON: ${error.message}`);
  }
}

function csvToList(raw) {
  return String(raw || "")
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

function listToCsv(items) {
  return Array.isArray(items) ? items.join(", ") : "";
}

function jsonText(value) {
  if (!value || (typeof value === "object" && Object.keys(value).length === 0)) return "";
  return JSON.stringify(value, null, 2);
}

function parseJsonValue(raw, label) {
  const text = String(raw || "").trim();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`${label} must be valid JSON: ${error.message}`);
  }
}

function clearInlineAction(scope) {
  if (!state.settingsInlineActions) return;
  state.settingsInlineActions[scope] = null;
}

function openInlineAction(scope, action) {
  if (!state.settingsInlineActions) return;
  state.settingsInlineActions[scope] = action;
}

function renderInlineAction(container, scope, options) {
  if (!container) return;
  container.innerHTML = "";
  const action = state.settingsInlineActions ? state.settingsInlineActions[scope] : null;
  const expectedTarget = options && options.targetKey ? options.targetKey : null;
  if (!action || (expectedTarget && action.targetKey !== expectedTarget)) {
    container.classList.add("hidden");
    return;
  }

  container.classList.remove("hidden");

  const wrap = document.createElement("div");
  wrap.className = "settings-inline-action-card";

  const label = document.createElement("div");
  label.className = "settings-inline-action-copy";

  let focusNode = null;

  if (action.kind === "rename" || action.kind === "create") {
    label.textContent = options.renameLabel;
    const input = document.createElement("input");
    input.type = "text";
    input.className = "form-input settings-inline-action-input";
    input.value = action.draft || options.currentValue || "";
    input.placeholder = options.placeholder || "";
    input.addEventListener("input", () => {
      if (state.settingsInlineActions && state.settingsInlineActions[scope]) {
        state.settingsInlineActions[scope].draft = input.value;
      }
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void options.onConfirm(input.value);
      } else if (event.key === "Escape") {
        event.preventDefault();
        options.onCancel();
      }
    });
    wrap.appendChild(label);
    wrap.appendChild(input);
    focusNode = input;
  } else if (action.kind === "delete") {
    label.textContent = options.deleteLabel;
    wrap.appendChild(label);
  }

  const actions = document.createElement("div");
  actions.className = "settings-inline-action-buttons";
  const confirmBtn = document.createElement("button");
  confirmBtn.type = "button";
  confirmBtn.className = action.kind === "delete" ? "btn btn-danger" : "btn btn-primary";
  confirmBtn.textContent =
    action.kind === "delete" ? "Delete" : action.kind === "create" ? "Create" : "Rename";
  confirmBtn.addEventListener("click", () => {
    const nextValue =
      action.kind === "rename" || action.kind === "create"
        ? (state.settingsInlineActions[scope]?.draft || options.currentValue || "")
        : undefined;
    void options.onConfirm(nextValue);
  });
  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "btn btn-secondary";
  cancelBtn.textContent = "Cancel";
  cancelBtn.addEventListener("click", options.onCancel);
  actions.appendChild(confirmBtn);
  actions.appendChild(cancelBtn);
  wrap.appendChild(actions);
  container.appendChild(wrap);
  if (!focusNode && action.kind === "delete") {
    focusNode = confirmBtn;
  }
  if (focusNode && typeof focusNode.focus === "function") {
    setTimeout(() => {
      focusNode.focus();
      if (typeof focusNode.select === "function" && (action.kind === "rename" || action.kind === "create")) {
        focusNode.select();
      }
    }, 0);
  }
}

function updateProviderSelection(providerKey) {
  state.settingsProviderSelection = providerKey || null;
  clearInlineAction("provider");
  clearInlineAction("model");
  ensureSettingsSelection();
  renderProviderSettings();
}

function updateModelSelection(modelKey) {
  state.settingsModelSelection = modelKey || null;
  clearInlineAction("model");
  renderProviderSettings();
}

function renderProviderList() {
  if (!nodes.settingsProviderList) return;
  nodes.settingsProviderList.innerHTML = "";

  const providerMap = currentProviderMap();
  const keys = sortedKeys(providerMap);
  if (keys.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No configured providers yet.";
    nodes.settingsProviderList.appendChild(empty);
    return;
  }

  for (const key of keys) {
    const provider = providerMap[key] || {};
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-session-btn";
    if (key === state.settingsProviderSelection) {
      button.classList.add("active");
    }
    const modelCount = sortedKeys(provider.models || {}).length;
    button.innerHTML = `${escapeHtml(provider.name || key)}<br><span class="muted">${escapeHtml(key)} · ${modelCount} models</span>`;
    button.addEventListener("click", () => updateProviderSelection(key));
    nodes.settingsProviderList.appendChild(button);
  }
}

function renderKnownProviderList() {
  if (!nodes.settingsKnownProviderList) return;
  nodes.settingsKnownProviderList.innerHTML = "";
  const configured = new Set(sortedKeys(currentProviderMap()));
  const known = Array.isArray(state.knownProviders) ? state.knownProviders : [];

  const available = known.filter((provider) => provider && !configured.has(provider.id));
  if (available.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No additional known providers to import.";
    nodes.settingsKnownProviderList.appendChild(empty);
    return;
  }

  for (const provider of available.slice(0, 18)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-session-btn";
    button.innerHTML = `${escapeHtml(provider.name || provider.id)}<br><span class="muted">${escapeHtml(provider.id)} · add starter config</span>`;
    button.addEventListener("click", () => {
      const providerMap = currentProviderMap();
      providerMap[provider.id] = {
        name: provider.name || provider.id,
        id: provider.id,
        models: {},
      };
      state.configSnapshot.provider = providerMap;
      updateProviderSelection(provider.id);
      setProviderStatus(`Created draft for ${provider.id}. Fill API key and models, then save.`, "info");
    });
    nodes.settingsKnownProviderList.appendChild(button);
  }
}

function renderModelList(provider) {
  if (!nodes.settingsModelList) return;
  nodes.settingsModelList.innerHTML = "";

  const modelKeys = sortedKeys((provider && provider.models) || {});
  if (modelKeys.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No models configured for this provider.";
    nodes.settingsModelList.appendChild(empty);
    return;
  }

  for (const modelKey of modelKeys) {
    const model = provider.models[modelKey] || {};
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-session-btn";
    if (modelKey === state.settingsModelSelection) {
      button.classList.add("active");
    }
    button.innerHTML = `${escapeHtml(model.name || modelKey)}<br><span class="muted">${escapeHtml(model.model || modelKey)}</span>`;
    button.addEventListener("click", () => updateModelSelection(modelKey));
    nodes.settingsModelList.appendChild(button);
  }
}

function renderProviderEditorFields(providerKey, provider) {
  const providerAction = state.settingsInlineActions ? state.settingsInlineActions.provider : null;
  const hasProvider = Boolean((providerKey && provider) || (providerAction && providerAction.kind === "create"));
  nodes.settingsProviderEmptyState.classList.toggle("hidden", hasProvider);
  nodes.settingsProviderEditor.classList.toggle("hidden", !hasProvider);
  if (!hasProvider) return;

  if (!providerKey || !provider) {
    nodes.settingsProviderKey.value = "";
    nodes.settingsProviderId.value = "";
    nodes.settingsProviderName.value = "";
    nodes.settingsProviderBaseUrl.value = "";
    nodes.settingsProviderApiKey.value = "";
    nodes.settingsProviderEnv.value = "";
    nodes.settingsProviderOptions.value = "";
    nodes.settingsModelList.innerHTML = "";
    nodes.settingsModelEmptyState.classList.remove("hidden");
    nodes.settingsModelEditor.classList.add("hidden");
    return;
  }

  nodes.settingsProviderKey.value = providerKey;
  nodes.settingsProviderKey.disabled = true;
  nodes.settingsProviderId.value = provider.id || "";
  nodes.settingsProviderName.value = provider.name || "";
  nodes.settingsProviderBaseUrl.value = provider.base_url || provider.baseUrl || "";
  nodes.settingsProviderApiKey.value = provider.api_key || provider.apiKey || "";
  nodes.settingsProviderEnv.value = Array.isArray(provider.env) ? provider.env.join(", ") : "";
  nodes.settingsProviderOptions.value = jsonText(provider.options);

  renderModelList(provider);

  const modelKey = state.settingsModelSelection;
  const model = modelKey && provider.models ? provider.models[modelKey] : null;
  const hasModel = Boolean(modelKey && model);
  nodes.settingsModelEmptyState.classList.toggle("hidden", hasModel);
  nodes.settingsModelEditor.classList.toggle("hidden", !hasModel);
  if (!hasModel) return;

  nodes.settingsModelKey.value = modelKey;
  nodes.settingsModelKey.disabled = true;
  nodes.settingsModelRuntimeId.value = model.model || model.id || "";
  nodes.settingsModelName.value = model.name || "";
  nodes.settingsModelFamily.value = model.family || "";
  nodes.settingsModelReasoning.checked = model.reasoning === true;
  nodes.settingsModelToolCall.checked = model.tool_call === true || model.toolCall === true;
  nodes.settingsModelTemperature.checked = model.temperature === true;
  nodes.settingsModelAttachment.checked = model.attachment === true;
  nodes.settingsModelHeaders.value = jsonText(model.headers);
  nodes.settingsModelOptions.value = jsonText(model.options);
  nodes.settingsModelVariants.value = listToCsv(Object.keys(model.variants || {}));
}

function renderProviderSettings() {
  ensureSettingsSelection();
  renderProviderList();
  renderKnownProviderList();
  const providerKey = state.settingsProviderSelection;
  const provider = providerKey ? currentProviderMap()[providerKey] : null;
  const providerAction = state.settingsInlineActions ? state.settingsInlineActions.provider : null;
  const modelAction = state.settingsInlineActions ? state.settingsInlineActions.model : null;
  renderProviderEditorFields(providerKey, provider);
  renderInlineAction(nodes.settingsProviderInlineAction, "provider", {
    targetKey: providerAction && providerAction.kind === "create" ? "__create__:provider" : providerKey,
    currentValue: providerKey || "",
    renameLabel:
      providerAction && providerAction.kind === "create"
        ? "Create a new provider key"
        : `Rename provider ${providerKey || ""}`,
    placeholder: "provider-key",
    deleteLabel: `Delete provider ${providerKey || ""}? This removes its configured models too.`,
    onConfirm: async (value) => {
      const kind = (state.settingsInlineActions.provider || {}).kind;
      if (kind === "create") {
        await commitCreateProvider(value);
      } else if (kind === "rename") {
        await commitRenameSelectedProvider(value);
      } else {
        await commitDeleteSelectedProvider();
      }
    },
    onCancel: () => {
      clearInlineAction("provider");
      renderProviderSettings();
    },
  });
  renderInlineAction(nodes.settingsModelInlineAction, "model", {
    targetKey:
      modelAction && modelAction.kind === "create"
        ? `__create__:${providerKey || ""}`
        : state.settingsModelSelection,
    currentValue: state.settingsModelSelection || "",
    renameLabel:
      modelAction && modelAction.kind === "create"
        ? `Create a new model under ${providerKey || "provider"}`
        : `Rename model ${state.settingsModelSelection || ""}`,
    placeholder: "model-key",
    deleteLabel: `Delete model ${state.settingsModelSelection || ""}?`,
    onConfirm: async (value) => {
      const kind = (state.settingsInlineActions.model || {}).kind;
      if (kind === "create") {
        await commitCreateModel(value);
      } else if (kind === "rename") {
        await commitRenameSelectedModel(value);
      } else {
        await commitDeleteSelectedModel();
      }
    },
    onCancel: () => {
      clearInlineAction("model");
      renderProviderSettings();
    },
  });
}

function updateMcpSelection(name) {
  state.settingsMcpSelection = name || null;
  clearInlineAction("mcp");
  renderMcpSettings();
}

function renderMcpList() {
  if (!nodes.settingsMcpList) return;
  nodes.settingsMcpList.innerHTML = "";
  const configMap = currentMcpMap();
  const runtimeMap = state.mcpStatusSnapshot || {};
  const names = Array.from(new Set([...sortedKeys(configMap), ...sortedKeys(runtimeMap)])).sort((a, b) => a.localeCompare(b));

  if (names.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No MCP servers configured.";
    nodes.settingsMcpList.appendChild(empty);
    return;
  }

  for (const name of names) {
    const runtime = runtimeMap[name] || {};
    const config = configMap[name] || {};
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-session-btn";
    if (name === state.settingsMcpSelection) button.classList.add("active");
    const status = runtime.status || ((config.enabled === false || config.enabled?.enabled === false) ? "disabled" : "configured");
    button.innerHTML = `${escapeHtml(name)}<br><span class="muted">${escapeHtml(status)} · tools ${String(runtime.tools || 0)} · resources ${String(runtime.resources || 0)}</span>`;
    button.addEventListener("click", () => updateMcpSelection(name));
    nodes.settingsMcpList.appendChild(button);
  }
}

function normalizeMcpConfig(entry) {
  if (!entry) return {};
  if (typeof entry === "object" && "enabled" in entry && Object.keys(entry).length === 1) {
    return { enabled: entry.enabled };
  }
  return entry;
}

function renderMcpSettings() {
  ensureSettingsSelection();
  renderMcpList();
  const name = state.settingsMcpSelection;
  const entry = name ? normalizeMcpConfig(currentMcpMap()[name]) : null;
  const runtime = name ? (state.mcpStatusSnapshot || {})[name] : null;
  const mcpAction = state.settingsInlineActions ? state.settingsInlineActions.mcp : null;
  const hasEntry = Boolean((name && entry) || (mcpAction && mcpAction.kind === "create"));
  nodes.settingsMcpEmptyState.classList.toggle("hidden", hasEntry);
  nodes.settingsMcpEditor.classList.toggle("hidden", !hasEntry);
  if (!hasEntry) return;

  if (!name || !entry) {
    nodes.settingsMcpName.value = "";
    nodes.settingsMcpType.value = "local";
    nodes.settingsMcpUrl.value = "";
    nodes.settingsMcpTimeout.value = "";
    nodes.settingsMcpEnabled.checked = true;
    nodes.settingsMcpCommand.value = "[]";
    nodes.settingsMcpEnv.value = "";
    nodes.settingsMcpOauth.value = "";
  } else {
    nodes.settingsMcpName.value = name;
    nodes.settingsMcpName.disabled = true;
    nodes.settingsMcpType.value = entry.type || entry.server_type || (entry.url ? "remote" : "local");
    nodes.settingsMcpUrl.value = entry.url || "";
    nodes.settingsMcpTimeout.value = entry.timeout || "";
    nodes.settingsMcpEnabled.checked = entry.enabled !== false;
    nodes.settingsMcpCommand.value = jsonText(Array.isArray(entry.command) ? entry.command : entry.command ? [entry.command] : []);
    nodes.settingsMcpEnv.value = jsonText(entry.environment || entry.env);
    nodes.settingsMcpOauth.value = entry.oauth === false ? "false" : jsonText(entry.oauth);
  }
  nodes.settingsMcpName.disabled = true;

  if (runtime && runtime.error) {
    setMcpStatus(`Runtime error: ${runtime.error}`, "error");
  } else if (runtime) {
    setMcpStatus(`Runtime status: ${runtime.status || "unknown"} · tools ${runtime.tools || 0} · resources ${runtime.resources || 0}`, "info");
  } else {
    setMcpStatus("Save to config authority, then connect or restart from here.", "info");
  }
  renderInlineAction(nodes.settingsMcpInlineAction, "mcp", {
    targetKey: mcpAction && mcpAction.kind === "create" ? "__create__:mcp" : name,
    currentValue: name || "",
    renameLabel:
      mcpAction && mcpAction.kind === "create"
        ? "Create a new MCP server"
        : `Rename MCP server ${name || ""}`,
    placeholder: "mcp-server-name",
    deleteLabel: `Delete MCP server ${name || ""}?`,
    onConfirm: async (value) => {
      const kind = (state.settingsInlineActions.mcp || {}).kind;
      if (kind === "create") {
        await commitCreateMcp(value);
      } else if (kind === "rename") {
        await commitRenameSelectedMcp(value);
      } else {
        await commitDeleteSelectedMcp();
      }
    },
    onCancel: () => {
      clearInlineAction("mcp");
      renderMcpSettings();
    },
  });
}

function updatePluginSelection(key) {
  state.settingsPluginSelection = key || null;
  clearInlineAction("plugin");
  renderPluginSettings();
}

function renderPluginList() {
  if (!nodes.settingsPluginList) return;
  nodes.settingsPluginList.innerHTML = "";
  const keys = sortedKeys(currentPluginMap());
  if (keys.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No plugins configured.";
    nodes.settingsPluginList.appendChild(empty);
    return;
  }

  for (const key of keys) {
    const plugin = currentPluginMap()[key] || {};
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-session-btn";
    if (key === state.settingsPluginSelection) button.classList.add("active");
    button.innerHTML = `${escapeHtml(key)}<br><span class="muted">${escapeHtml(plugin.plugin_type || plugin.type || "plugin")} · ${escapeHtml(plugin.package || plugin.path || "")}</span>`;
    button.addEventListener("click", () => updatePluginSelection(key));
    nodes.settingsPluginList.appendChild(button);
  }
}

function renderPluginAuthList() {
  if (!nodes.settingsPluginAuthList) return;
  nodes.settingsPluginAuthList.innerHTML = "";
  const bridges = Array.isArray(state.pluginAuthSnapshot) ? state.pluginAuthSnapshot : [];
  if (bridges.length === 0) {
    const empty = document.createElement("div");
    empty.className = "settings-empty-state";
    empty.textContent = "No plugin auth bridges reported.";
    nodes.settingsPluginAuthList.appendChild(empty);
    return;
  }
  for (const bridge of bridges) {
    const item = document.createElement("div");
    item.className = "command-session-btn";
    const methods = Array.isArray(bridge.methods) ? bridge.methods.map((method) => method.label || method.type).join(", ") : "";
    item.innerHTML = `${escapeHtml(bridge.provider || "plugin")}<br><span class="muted">${escapeHtml(methods || "no auth methods")}</span>`;
    nodes.settingsPluginAuthList.appendChild(item);
  }
}

function renderPluginSettings() {
  ensureSettingsSelection();
  renderPluginList();
  renderPluginAuthList();
  const key = state.settingsPluginSelection;
  const plugin = key ? currentPluginMap()[key] : null;
  const pluginAction = state.settingsInlineActions ? state.settingsInlineActions.plugin : null;
  const hasPlugin = Boolean((key && plugin) || (pluginAction && pluginAction.kind === "create"));
  nodes.settingsPluginEmptyState.classList.toggle("hidden", hasPlugin);
  nodes.settingsPluginEditor.classList.toggle("hidden", !hasPlugin);
  if (!hasPlugin) return;
  if (!key || !plugin) {
    nodes.settingsPluginKey.value = "";
    nodes.settingsPluginType.value = "npm";
    nodes.settingsPluginPackage.value = "";
    nodes.settingsPluginVersion.value = "";
    nodes.settingsPluginPath.value = "";
    nodes.settingsPluginRuntime.value = "";
    nodes.settingsPluginOptions.value = "";
  } else {
    nodes.settingsPluginKey.value = key;
    nodes.settingsPluginType.value = plugin.plugin_type || plugin.type || "npm";
    nodes.settingsPluginPackage.value = plugin.package || "";
    nodes.settingsPluginVersion.value = plugin.version || "";
    nodes.settingsPluginPath.value = plugin.path || "";
    nodes.settingsPluginRuntime.value = plugin.runtime || "";
    nodes.settingsPluginOptions.value = jsonText(plugin.options);
  }
  nodes.settingsPluginKey.disabled = true;
  setPluginStatus("Plugin config is authoritative once saved; auth bridges reflect the running plugin loader.", "info");
  renderInlineAction(nodes.settingsPluginInlineAction, "plugin", {
    targetKey: pluginAction && pluginAction.kind === "create" ? "__create__:plugin" : key,
    currentValue: key || "",
    renameLabel:
      pluginAction && pluginAction.kind === "create"
        ? "Create a new plugin key"
        : `Rename plugin ${key || ""}`,
    placeholder: "plugin-key",
    deleteLabel: `Delete plugin ${key || ""}?`,
    onConfirm: async (value) => {
      const kind = (state.settingsInlineActions.plugin || {}).kind;
      if (kind === "create") {
        await commitCreatePlugin(value);
      } else if (kind === "rename") {
        await commitRenameSelectedPlugin(value);
      } else {
        await commitDeleteSelectedPlugin();
      }
    },
    onCancel: () => {
      clearInlineAction("plugin");
      renderPluginSettings();
    },
  });
}

function renderLspSettings() {
  const lsp = currentLspConfig();
  const formatter = currentFormatterConfig();
  nodes.settingsLspConfig.value = jsonText(lsp) || "";
  nodes.settingsFormatterConfig.value = jsonText(formatter) || "";
  const lspCount = Array.isArray(state.lspStatusSnapshot.servers) ? state.lspStatusSnapshot.servers.length : 0;
  const formatterCount = Array.isArray(state.formatterStatusSnapshot.formatters) ? state.formatterStatusSnapshot.formatters.length : 0;
  nodes.settingsLspStatusSummary.textContent = `runtime servers: ${lspCount} · runtime formatters: ${formatterCount}`;
  setLspStatus("Edit the authority JSON directly; save patches both lsp and formatter config together.", "info");
}

function schedulerTemplate() {
  return `{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "defaults": { "profile": "custom-default" },
  "profiles": {
    "custom-default": {
      "orchestrator": "sisyphus",
      "description": "Project-local execution preset",
      "stages": [
        "request-analysis",
        "route",
        "execution-orchestration",
        "synthesis"
      ]
    }
  }
}`;
}

function renderSchedulerSettings() {
  const snapshot = state.schedulerConfigSnapshot || {};
  nodes.settingsSchedulerPath.value = snapshot.path || ".rocode/scheduler.jsonc";
  nodes.settingsSchedulerContent.value = snapshot.content || "";

  const meta = [];
  if (snapshot.resolvedPath) meta.push(`resolved: ${snapshot.resolvedPath}`);
  meta.push(snapshot.exists ? "file: present" : "file: new");
  if (snapshot.defaultProfile) meta.push(`default: ${snapshot.defaultProfile}`);
  nodes.settingsSchedulerMeta.textContent = meta.join(" · ");

  nodes.settingsSchedulerProfiles.innerHTML = "";
  const profiles = Array.isArray(snapshot.profiles) ? snapshot.profiles : [];
  if (profiles.length === 0) {
    const empty = document.createElement("span");
    empty.className = "meta-pill";
    empty.innerHTML = `<span class="meta-label">profiles</span><span>none parsed</span>`;
    nodes.settingsSchedulerProfiles.appendChild(empty);
  } else {
    for (const profile of profiles) {
      const chip = document.createElement("span");
      chip.className = "meta-pill";
      const stageSummary = Array.isArray(profile.stages) && profile.stages.length > 0
        ? profile.stages.join(" → ")
        : "no stages";
      chip.innerHTML =
        `<span class="meta-label">${escapeHtml(profile.key)}</span><span>${escapeHtml(profile.orchestrator || "custom")} · ${escapeHtml(stageSummary)}</span>`;
      nodes.settingsSchedulerProfiles.appendChild(chip);
    }
  }

  if (snapshot.parseError) {
    setSchedulerStatus(`Parse error: ${snapshot.parseError}`, "error");
  } else if (snapshot.exists) {
    setSchedulerStatus("Scheduler file loaded. Saving will write the file and refresh execution modes.", "info");
  } else {
    setSchedulerStatus("No scheduler file yet. Seed a template or write your own JSONC, then save.", "info");
  }
}

async function loadSettingsWorkspace(options = {}) {
  const { force = false } = options;
  if (
    !force &&
    state.configSnapshot &&
    state.schedulerConfigSnapshot &&
    state.knownProviders.length > 0
  ) {
    renderProviderSettings();
    renderSchedulerSettings();
    renderMcpSettings();
    renderPluginSettings();
    renderLspSettings();
    return;
  }

  const [
    configResponse,
    knownProvidersResponse,
    schedulerResponse,
    mcpResponse,
    pluginAuthResponse,
    lspResponse,
    formatterResponse,
  ] = await Promise.all([
    api("/config"),
    api("/provider/known"),
    api("/config/scheduler"),
    api("/mcp"),
    api("/plugin/auth"),
    api("/lsp"),
    api("/formatter"),
  ]);

  state.configSnapshot = await configResponse.json();
  const knownProvidersPayload = await knownProvidersResponse.json();
  state.knownProviders = knownProvidersPayload.providers || [];
  state.schedulerConfigSnapshot = await schedulerResponse.json();
  state.mcpStatusSnapshot = await mcpResponse.json();
  state.pluginAuthSnapshot = await pluginAuthResponse.json();
  state.lspStatusSnapshot = await lspResponse.json();
  state.formatterStatusSnapshot = await formatterResponse.json();
  ensureSettingsSelection();
  renderProviderSettings();
  renderSchedulerSettings();
  renderMcpSettings();
  renderPluginSettings();
  renderLspSettings();
}

async function saveSelectedProvider() {
  const providerKey = state.settingsProviderSelection;
  if (!providerKey) {
    setProviderStatus("Select or create a provider first.", "error");
    return;
  }

  const providerMap = currentProviderMap();
  const existing = providerMap[providerKey] || {};
  const modelKey = state.settingsModelSelection;
  const models = { ...(existing.models || {}) };

  if (modelKey) {
    const variants = {};
    for (const variant of csvToList(nodes.settingsModelVariants.value)) {
      variants[variant] = {};
    }
    models[modelKey] = {
      ...(models[modelKey] || {}),
      name: nodes.settingsModelName.value.trim() || null,
      model: nodes.settingsModelRuntimeId.value.trim() || null,
      family: nodes.settingsModelFamily.value.trim() || null,
      reasoning: nodes.settingsModelReasoning.checked || undefined,
      tool_call: nodes.settingsModelToolCall.checked || undefined,
      temperature: nodes.settingsModelTemperature.checked || undefined,
      attachment: nodes.settingsModelAttachment.checked || undefined,
      headers: readJsonTextarea(nodes.settingsModelHeaders, "Model headers") || undefined,
      options: readJsonTextarea(nodes.settingsModelOptions, "Model options") || undefined,
      variants: Object.keys(variants).length > 0 ? variants : undefined,
    };
  }

  const providerConfig = {
    ...existing,
    name: nodes.settingsProviderName.value.trim() || null,
    id: nodes.settingsProviderId.value.trim() || null,
    api_key: nodes.settingsProviderApiKey.value.trim() || null,
    base_url: nodes.settingsProviderBaseUrl.value.trim() || null,
    env: csvToList(nodes.settingsProviderEnv.value),
    options: readJsonTextarea(nodes.settingsProviderOptions, "Provider options") || undefined,
    models,
  };

  await api(`/config/provider/${encodeURIComponent(providerKey)}`, {
    method: "PUT",
    body: JSON.stringify(providerConfig),
  });

  setProviderStatus(`Saved ${providerKey} to shared config authority.`, "info");
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
}

async function reloadProviderSettings() {
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
  setProviderStatus("Provider config reloaded from server authority.", "info");
}

function createBlankProvider() {
  openInlineAction("provider", {
    kind: "create",
    targetKey: "__create__:provider",
    draft: "",
  });
  renderProviderSettings();
}

async function commitCreateProvider(nextKey) {
  const normalized = String(nextKey || "").trim();
  if (!normalized) {
    setProviderStatus("Provider key is required.", "error");
    return;
  }
  const providerMap = currentProviderMap();
  if (providerMap[normalized]) {
    state.settingsProviderSelection = normalized;
    clearInlineAction("provider");
    renderProviderSettings();
    setProviderStatus(`${normalized} already exists; opened existing config.`, "info");
    return;
  }
  providerMap[normalized] = {
    id: normalized,
    name: normalized,
    models: {},
  };
  state.configSnapshot.provider = providerMap;
  state.settingsProviderSelection = normalized;
  clearInlineAction("provider");
  renderProviderSettings();
  setProviderStatus(`Draft provider ${normalized} created. Fill fields and save.`, "success");
}

function createBlankModel() {
  const providerKey = state.settingsProviderSelection;
  if (!providerKey) {
    setProviderStatus("Create or select a provider before adding a model.", "error");
    return;
  }
  openInlineAction("model", {
    kind: "create",
    targetKey: `__create__:${providerKey}`,
    draft: "",
  });
  renderProviderSettings();
}

async function commitCreateModel(nextKey) {
  const providerKey = state.settingsProviderSelection;
  const normalized = String(nextKey || "").trim();
  if (!providerKey) {
    setProviderStatus("Select a provider before creating a model.", "error");
    return;
  }
  if (!normalized) {
    setProviderStatus("Model key is required.", "error");
    return;
  }
  const providerMap = currentProviderMap();
  const provider = providerMap[providerKey] || {};
  provider.models = provider.models || {};
  if (provider.models[normalized]) {
    state.settingsModelSelection = normalized;
    clearInlineAction("model");
    renderProviderSettings();
    setProviderStatus(`${normalized} already exists; opened existing model.`, "info");
    return;
  }
  provider.models[normalized] = { name: normalized, model: normalized };
  providerMap[providerKey] = provider;
  state.configSnapshot.provider = providerMap;
  state.settingsModelSelection = normalized;
  clearInlineAction("model");
  renderProviderSettings();
  setProviderStatus(`Draft model ${normalized} created. Fill fields and save.`, "success");
}

async function renameSelectedProvider() {
  const currentKey = state.settingsProviderSelection;
  if (!currentKey) {
    setProviderStatus("Select a provider first.", "error");
    return;
  }
  openInlineAction("provider", {
    kind: "rename",
    targetKey: currentKey,
    draft: currentKey,
  });
  renderProviderSettings();
}

async function commitRenameSelectedProvider(nextKey) {
  const currentKey = state.settingsProviderSelection;
  const normalized = String(nextKey || "").trim();
  if (!currentKey || !normalized || normalized === currentKey) {
    clearInlineAction("provider");
    renderProviderSettings();
    return;
  }
  const provider = currentProviderMap()[currentKey];
  await api(`/config/provider/${encodeURIComponent(normalized)}`, {
    method: "PUT",
    body: JSON.stringify(provider),
  });
  await api(`/config/provider/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsProviderSelection = normalized;
  clearInlineAction("provider");
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
  setProviderStatus(`Renamed provider ${currentKey} → ${normalized}.`, "info");
}

async function deleteSelectedProvider() {
  const currentKey = state.settingsProviderSelection;
  if (!currentKey) {
    setProviderStatus("Select a provider first.", "error");
    return;
  }
  openInlineAction("provider", {
    kind: "delete",
    targetKey: currentKey,
  });
  renderProviderSettings();
}

async function commitDeleteSelectedProvider() {
  const currentKey = state.settingsProviderSelection;
  if (!currentKey) return;
  await api(`/config/provider/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsProviderSelection = null;
  state.settingsModelSelection = null;
  clearInlineAction("provider");
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
  setProviderStatus(`Deleted provider ${currentKey}.`, "info");
}

async function renameSelectedModel() {
  const providerKey = state.settingsProviderSelection;
  const currentKey = state.settingsModelSelection;
  if (!providerKey || !currentKey) {
    setProviderStatus("Select a model first.", "error");
    return;
  }
  openInlineAction("model", {
    kind: "rename",
    targetKey: currentKey,
    draft: currentKey,
  });
  renderProviderSettings();
}

async function commitRenameSelectedModel(nextKey) {
  const providerKey = state.settingsProviderSelection;
  const currentKey = state.settingsModelSelection;
  const normalized = String(nextKey || "").trim();
  if (!providerKey || !currentKey || !normalized || normalized === currentKey) {
    clearInlineAction("model");
    renderProviderSettings();
    return;
  }
  const provider = currentProviderMap()[providerKey] || {};
  const model = ((provider.models || {})[currentKey]) || {};
  await api(`/config/provider/${encodeURIComponent(providerKey)}/models/${encodeURIComponent(normalized)}`, {
    method: "PUT",
    body: JSON.stringify(model),
  });
  await api(`/config/provider/${encodeURIComponent(providerKey)}/models/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsModelSelection = normalized;
  clearInlineAction("model");
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
  setProviderStatus(`Renamed model ${currentKey} → ${normalized}.`, "info");
}

async function deleteSelectedModel() {
  const providerKey = state.settingsProviderSelection;
  const currentKey = state.settingsModelSelection;
  if (!providerKey || !currentKey) {
    setProviderStatus("Select a model first.", "error");
    return;
  }
  openInlineAction("model", {
    kind: "delete",
    targetKey: currentKey,
  });
  renderProviderSettings();
}

async function commitDeleteSelectedModel() {
  const providerKey = state.settingsProviderSelection;
  const currentKey = state.settingsModelSelection;
  if (!providerKey || !currentKey) return;
  await api(`/config/provider/${encodeURIComponent(providerKey)}/models/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsModelSelection = null;
  clearInlineAction("model");
  await Promise.all([loadProviders(), loadModes()]);
  await loadSettingsWorkspace({ force: true });
  setProviderStatus(`Deleted model ${currentKey}.`, "info");
}

async function saveSchedulerSettings() {
  const path = nodes.settingsSchedulerPath.value.trim();
  const content = nodes.settingsSchedulerContent.value;
  const response = await api("/config/scheduler", {
    method: "PUT",
    body: JSON.stringify({
      path: path || null,
      content,
    }),
  });
  state.schedulerConfigSnapshot = await response.json();
  await loadModes();
  await loadSettingsWorkspace({ force: true });
}

async function reloadMcpSettings() {
  await loadSettingsWorkspace({ force: true });
  setMcpStatus("MCP status and config reloaded.", "info");
}

function createBlankMcp() {
  openInlineAction("mcp", {
    kind: "create",
    targetKey: "__create__:mcp",
    draft: "",
  });
  renderMcpSettings();
}

async function commitCreateMcp(nextKey) {
  const normalized = String(nextKey || "").trim();
  if (!normalized) {
    setMcpStatus("MCP server name is required.", "error");
    return;
  }
  const map = currentMcpMap();
  if (!map[normalized]) {
    map[normalized] = {
      type: "local",
      command: [],
      enabled: true,
    };
    state.configSnapshot.mcp = map;
  }
  state.settingsMcpSelection = normalized;
  clearInlineAction("mcp");
  renderMcpSettings();
  setMcpStatus(`Draft MCP server ${normalized} created. Fill fields and save.`, "success");
}

async function saveSelectedMcp() {
  const name = state.settingsMcpSelection;
  if (!name) {
    setMcpStatus("Select or create an MCP server first.", "error");
    return;
  }
  const type = nodes.settingsMcpType.value || "local";
  const command = parseJsonValue(nodes.settingsMcpCommand.value, "MCP command") || [];
  if (!Array.isArray(command)) {
    throw new Error("MCP command must be a JSON array");
  }
  const env = parseJsonValue(nodes.settingsMcpEnv.value, "MCP environment");
  const oauth = parseJsonValue(nodes.settingsMcpOauth.value, "MCP oauth");
  const timeoutRaw = String(nodes.settingsMcpTimeout.value || "").trim();
  const payload = {
    type,
    url: nodes.settingsMcpUrl.value.trim() || undefined,
    command,
    environment: env || undefined,
    oauth: oauth === null ? undefined : oauth,
    timeout: timeoutRaw ? Number(timeoutRaw) : undefined,
    enabled: nodes.settingsMcpEnabled.checked,
  };
  await api(`/config/mcp/${encodeURIComponent(name)}`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  await loadSettingsWorkspace({ force: true });
  setMcpStatus(`Saved MCP server ${name}.`, "info");
}

async function connectSelectedMcp() {
  const name = state.settingsMcpSelection;
  if (!name) {
    setMcpStatus("Select an MCP server first.", "error");
    return;
  }
  await api(`/mcp/${name}/connect`, { method: "POST" });
  await loadSettingsWorkspace({ force: true });
  setMcpStatus(`Connect requested for ${name}.`, "info");
}

async function restartSelectedMcp() {
  const name = state.settingsMcpSelection;
  if (!name) {
    setMcpStatus("Select an MCP server first.", "error");
    return;
  }
  await api(`/mcp/${name}/restart`, { method: "POST" });
  await loadSettingsWorkspace({ force: true });
  setMcpStatus(`Restart requested for ${name}.`, "info");
}

async function renameSelectedMcp() {
  const currentKey = state.settingsMcpSelection;
  if (!currentKey) {
    setMcpStatus("Select an MCP server first.", "error");
    return;
  }
  openInlineAction("mcp", {
    kind: "rename",
    targetKey: currentKey,
    draft: currentKey,
  });
  renderMcpSettings();
}

async function commitRenameSelectedMcp(nextKey) {
  const currentKey = state.settingsMcpSelection;
  const normalized = String(nextKey || "").trim();
  if (!currentKey || !normalized || normalized === currentKey) {
    clearInlineAction("mcp");
    renderMcpSettings();
    return;
  }
  const entry = currentMcpMap()[currentKey];
  await api(`/config/mcp/${encodeURIComponent(normalized)}`, {
    method: "PUT",
    body: JSON.stringify(entry),
  });
  await api(`/config/mcp/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsMcpSelection = normalized;
  clearInlineAction("mcp");
  await loadSettingsWorkspace({ force: true });
  setMcpStatus(`Renamed MCP server ${currentKey} → ${normalized}.`, "info");
}

async function deleteSelectedMcp() {
  const currentKey = state.settingsMcpSelection;
  if (!currentKey) {
    setMcpStatus("Select an MCP server first.", "error");
    return;
  }
  openInlineAction("mcp", {
    kind: "delete",
    targetKey: currentKey,
  });
  renderMcpSettings();
}

async function commitDeleteSelectedMcp() {
  const currentKey = state.settingsMcpSelection;
  if (!currentKey) return;
  await api(`/config/mcp/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsMcpSelection = null;
  clearInlineAction("mcp");
  await loadSettingsWorkspace({ force: true });
  setMcpStatus(`Deleted MCP server ${currentKey}.`, "info");
}

async function reloadPluginSettings() {
  await loadSettingsWorkspace({ force: true });
  setPluginStatus("Plugin config and auth bridge info reloaded.", "info");
}

function createBlankPlugin() {
  openInlineAction("plugin", {
    kind: "create",
    targetKey: "__create__:plugin",
    draft: "",
  });
  renderPluginSettings();
}

async function commitCreatePlugin(nextKey) {
  const normalized = String(nextKey || "").trim();
  if (!normalized) {
    setPluginStatus("Plugin key is required.", "error");
    return;
  }
  const map = currentPluginMap();
  if (!map[normalized]) {
    map[normalized] = { type: "npm", options: {} };
    state.configSnapshot.plugin = map;
  }
  state.settingsPluginSelection = normalized;
  clearInlineAction("plugin");
  renderPluginSettings();
  setPluginStatus(`Draft plugin ${normalized} created. Fill fields and save.`, "success");
}

async function saveSelectedPlugin() {
  const key = state.settingsPluginSelection;
  if (!key) {
    setPluginStatus("Select or create a plugin first.", "error");
    return;
  }
  const payload = {
    type: nodes.settingsPluginType.value,
    package: nodes.settingsPluginPackage.value.trim() || undefined,
    version: nodes.settingsPluginVersion.value.trim() || undefined,
    path: nodes.settingsPluginPath.value.trim() || undefined,
    runtime: nodes.settingsPluginRuntime.value.trim() || undefined,
    options: readJsonTextarea(nodes.settingsPluginOptions, "Plugin options") || {},
  };
  await api(`/config/plugin/${encodeURIComponent(key)}`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  await loadSettingsWorkspace({ force: true });
  setPluginStatus(`Saved plugin ${key}.`, "info");
}

async function renameSelectedPlugin() {
  const currentKey = state.settingsPluginSelection;
  if (!currentKey) {
    setPluginStatus("Select a plugin first.", "error");
    return;
  }
  openInlineAction("plugin", {
    kind: "rename",
    targetKey: currentKey,
    draft: currentKey,
  });
  renderPluginSettings();
}

async function commitRenameSelectedPlugin(nextKey) {
  const currentKey = state.settingsPluginSelection;
  const normalized = String(nextKey || "").trim();
  if (!currentKey || !normalized || normalized === currentKey) {
    clearInlineAction("plugin");
    renderPluginSettings();
    return;
  }
  const plugin = currentPluginMap()[currentKey];
  await api(`/config/plugin/${encodeURIComponent(normalized)}`, {
    method: "PUT",
    body: JSON.stringify(plugin),
  });
  await api(`/config/plugin/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsPluginSelection = normalized;
  clearInlineAction("plugin");
  await loadSettingsWorkspace({ force: true });
  setPluginStatus(`Renamed plugin ${currentKey} → ${normalized}.`, "info");
}

async function deleteSelectedPlugin() {
  const currentKey = state.settingsPluginSelection;
  if (!currentKey) {
    setPluginStatus("Select a plugin first.", "error");
    return;
  }
  openInlineAction("plugin", {
    kind: "delete",
    targetKey: currentKey,
  });
  renderPluginSettings();
}

async function commitDeleteSelectedPlugin() {
  const currentKey = state.settingsPluginSelection;
  if (!currentKey) return;
  await api(`/config/plugin/${encodeURIComponent(currentKey)}`, {
    method: "DELETE",
  });
  state.settingsPluginSelection = null;
  clearInlineAction("plugin");
  await loadSettingsWorkspace({ force: true });
  setPluginStatus(`Deleted plugin ${currentKey}.`, "info");
}

async function reloadLspSettings() {
  await loadSettingsWorkspace({ force: true });
  setLspStatus("LSP and formatter state reloaded.", "info");
}

async function saveLspSettings() {
  const lspValue = parseJsonValue(nodes.settingsLspConfig.value, "LSP config");
  const formatterValue = parseJsonValue(nodes.settingsFormatterConfig.value, "Formatter config");
  await api("/config", {
    method: "PATCH",
    body: JSON.stringify({
      lsp: lspValue,
      formatter: formatterValue,
    }),
  });
  await loadSettingsWorkspace({ force: true });
  setLspStatus("Saved LSP and formatter authority.", "info");
}

function seedSchedulerTemplate() {
  nodes.settingsSchedulerContent.value = schedulerTemplate();
  if (!nodes.settingsSchedulerPath.value.trim()) {
    nodes.settingsSchedulerPath.value = ".rocode/scheduler.jsonc";
  }
  setSchedulerStatus("Template inserted. Save to write the file and activate it via /config.", "info");
}

function openCommandPanel(section) {
  renderModelOptions();
  renderThemeOptions();
  renderModeOptions();
  renderCommandSessionList();
  renderCommandCatalog();
  updateCommandActionControls();

  const tab = settingsTabFromSection(section);
  setSettingsTab(tab);
  nodes.commandPanel.classList.remove("hidden");
  void loadSettingsWorkspace({ force: true }).catch((error) => {
    applyOutputBlock({
      kind: "status",
      tone: "error",
      text: `Failed to load settings: ${String(error)}`,
    });
  });

  if (canAbortCurrentExecution() && nodes.commandAbortBtn) {
    nodes.commandAbortBtn.focus();
  } else if (tab === "general" && section === "theme") nodes.themeSelect.focus();
  else if (tab === "general" && (section === "mode" || section === "agent")) nodes.agentSelect.focus();
  else if (tab === "general") nodes.modelSelect.focus();
  else if (tab === "providers" && nodes.settingsProviderList) nodes.settingsProviderList.focus?.();
  else if (tab === "scheduler" && nodes.settingsSchedulerPath) nodes.settingsSchedulerPath.focus();
  else if (tab === "mcp" && nodes.settingsMcpList) nodes.settingsMcpList.focus?.();
  else if (tab === "plugins" && nodes.settingsPluginList) nodes.settingsPluginList.focus?.();
  else if (tab === "lsp" && nodes.settingsLspConfig) nodes.settingsLspConfig.focus();
  else if (tab === "sessions") {
    const first = nodes.commandSessionList.querySelector("button");
    if (first) first.focus();
  }
}

function closeCommandPanel() {
  if (state.settingsInlineActions) {
    state.settingsInlineActions.provider = null;
    state.settingsInlineActions.model = null;
    state.settingsInlineActions.mcp = null;
    state.settingsInlineActions.plugin = null;
  }
  nodes.commandPanel.classList.add("hidden");
}
