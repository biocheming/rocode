// ── Slash Commands ─────────────────────────────────────────────────────────

function slashCatalog() {
  return Array.isArray(state.uiCommands)
    ? state.uiCommands.filter((command) => command && command.slash)
    : [];
}

function normalizeSlashName(name) {
  const trimmed = String(name || "").trim().toLowerCase();
  if (!trimmed) return "";
  return trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
}

function findUiSlashCommand(name) {
  const normalized = normalizeSlashName(name);
  if (!normalized) return null;

  return slashCatalog().find((command) => {
    const slash = command && command.slash ? command.slash : null;
    if (!slash) return false;
    if (normalizeSlashName(slash.name) === normalized) return true;
    return Array.isArray(slash.aliases)
      ? slash.aliases.some((alias) => normalizeSlashName(alias) === normalized)
      : false;
  }) || null;
}

async function resolveUiCommandInvocation(input) {
  try {
    const response = await api("/command/ui/resolve", {
      method: "POST",
      body: JSON.stringify({ input }),
    });
    return await response.json();
  } catch (_) {
    const trimmed = String(input || "").trim();
    if (!trimmed.startsWith("/")) return null;
    const body = trimmed.slice(1).trim();
    if (!body) return null;
    const [nameRaw, ...rest] = body.split(/\s+/);
    const command = findUiSlashCommand(nameRaw);
    if (!command) return null;
    const argument = rest.join(" ").trim();
    return {
      action_id: commandActionId(command),
      argument_kind: commandArgumentKind(command),
      argument: argument || null,
    };
  }
}

function commandActionId(command) {
  return command && command.action_id ? String(command.action_id) : "";
}

function commandArgumentKind(command) {
  const direct = command && (command.argument_kind || command.argumentKind)
    ? String(command.argument_kind || command.argumentKind)
    : "";
  if (direct) return direct;

  switch (commandActionId(command)) {
    case "open_session_list":
      return "session_target";
    case "open_model_list":
      return "model_ref";
    case "open_mode_list":
      return "mode_ref";
    case "open_agent_list":
      return "agent_ref";
    case "open_preset_list":
      return "preset_ref";
    case "open_theme_list":
      return "theme_id";
    default:
      return "none";
  }
}

function sharedHelpLines() {
  const lines = slashCatalog()
    .filter((command) => command.slash && command.slash.suggested)
    .map((command) => {
      const description = command.description || command.title || command.slash.name;
      return `${command.slash.name}   ${description}`;
    });

  if (lines.length > 0) return lines;

  return [
    "/model <provider/model>   set active model",
    "/theme <midnight|graphite|sunset|daylight>   switch theme",
    "/mode <name|kind:name|auto>   set active mode",
    "/agent <name|auto>   switch agent mode",
    "/preset <name|auto>   switch preset mode",
    "/abort   cancel current run or stage",
    "/status   show runtime status",
    "/session <id|list|new|fork|compact|delete>   manage session",
  ];
}

async function loadUiCommands() {
  const response = await api("/command/ui");
  state.uiCommands = await response.json();
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

function modelOptionList() {
  return Array.from(nodes.modelSelect && nodes.modelSelect.options
    ? nodes.modelSelect.options
    : nodes.modelSelect && nodes.modelSelect.children
      ? nodes.modelSelect.children
      : []);
}

function resolveModeScope(actionId) {
  if (actionId === "open_preset_list") {
    return "preset";
  }
  if (actionId === "open_agent_list") {
    return "agent";
  }
  return "mode";
}

function resolveModePanelSection(actionId) {
  return resolveModeScope(actionId) === "preset" ? "mode" : "mode";
}

function resolveModeFromArg(arg, scope) {
  const lowerArg = String(arg || "").trim().toLowerCase();
  if (!lowerArg) return null;

  return state.modes.find((mode) => {
    if (scope === "agent" && mode.kind !== "agent") return false;
    if (scope === "preset" && mode.kind !== "preset" && mode.kind !== "profile") return false;
    if (mode.key.toLowerCase() === lowerArg) return true;
    if (mode.name.toLowerCase() === lowerArg) return true;
    return `${mode.kind}:${mode.name}`.toLowerCase() === lowerArg;
  }) || null;
}

function setSelectedModelByArg(arg) {
  const value = String(arg || "").trim();
  const ok = modelOptionList().some((opt) => opt.value === value);
  if (!ok) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Unknown model: ${value}` });
    return false;
  }
  state.selectedModel = value;
  nodes.modelSelect.value = value;
  updateComposerMeta();
  updateSessionRuntimeMeta(currentSession());
  applyOutputBlock({ kind: "status", tone: "success", text: `Model set to ${value}` });
  return true;
}

function setSelectedModeByArg(arg, scope) {
  const value = String(arg || "").trim();
  if (value === "auto") {
    setSelectedMode(null);
    applyOutputBlock({ kind: "status", tone: "success", text: "Mode set to auto" });
    return true;
  }

  const found = resolveModeFromArg(value, scope);
  if (!found) {
    applyOutputBlock({ kind: "status", tone: "error", text: `Unknown mode: ${value}` });
    return true;
  }

  setSelectedMode(found.key);
  applyOutputBlock({ kind: "status", tone: "success", text: `Mode set to ${selectedModeLabel()}` });
  return true;
}

async function switchToSession(session) {
  if (!session) return false;
  state.selectedSession = session.id;
  state.selectedProject = projectKey(session);
  renderProjects();
  await loadMessages();
  return true;
}

function renderStatusMessage() {
  const current = currentSession();
  applyOutputBlock({
    kind: "message",
    phase: "full",
    role: "system",
    title: "status",
    text: [
      `state: ${runtimeStatusLabel()}`,
      `session: ${current ? `${current.id} (${short(current.title, 40)})` : "none"}`,
      `mode: ${current ? sessionModeLabel(current) : selectedModeLabel()}`,
      `model: ${current ? sessionModelLabel(current) : state.selectedModel || "auto"}`,
      `directory: ${current ? sessionDirectoryLabel(current) : "workspace"}`,
      `tokens: ${state.promptTokens} / ${state.completionTokens}`,
    ].join("\n"),
  });
}

async function navigateParentSessionView() {
  if (!state.parentSessionId) {
    applyOutputBlock({
      kind: "status",
      tone: "warning",
      text: "No parent session is currently attached.",
    });
    return true;
  }
  const parentId = state.parentSessionId;
  state.selectedSession = parentId;
  state.parentSessionId = null;
  renderProjects();
  await loadMessages();
  applyOutputBlock({
    kind: "status",
    tone: "success",
    text: `Returned to parent session: ${parentId}`,
  });
  return true;
}

async function copyCurrentSessionTranscript() {
  if (!state.selectedSession) {
    applyOutputBlock({
      kind: "status",
      tone: "warning",
      text: "No active session to copy.",
    });
    return true;
  }
  const transcript = String(nodes.messageFeed ? nodes.messageFeed.textContent || "" : "").trim();
  if (!transcript) {
    applyOutputBlock({
      kind: "status",
      tone: "warning",
      text: "No transcript available for current session.",
    });
    return true;
  }
  if (!navigator.clipboard || !navigator.clipboard.writeText) {
    applyOutputBlock({
      kind: "status",
      tone: "error",
      text: "Clipboard access is not available in this browser.",
    });
    return true;
  }
  await navigator.clipboard.writeText(transcript);
  applyOutputBlock({
    kind: "status",
    tone: "success",
    text: "Session transcript copied to clipboard.",
  });
  return true;
}

async function executeParameterizedUiCommand(actionId, argumentKind, arg = "") {
  switch (argumentKind) {
    case "model_ref":
      if (!arg) {
        openCommandPanel("model");
        return true;
      }
      return setSelectedModelByArg(arg);
    case "theme_id":
      if (!arg) {
        openCommandPanel("theme");
        return true;
      }
      applyTheme(arg);
      return true;
    case "mode_ref":
    case "agent_ref":
    case "preset_ref":
      if (!arg) {
        openCommandPanel(resolveModePanelSection(actionId));
        return true;
      }
      return setSelectedModeByArg(arg, resolveModeScope(actionId));
    case "session_target":
      if (!arg) {
        openCommandPanel("session");
        return true;
      }
      if (arg === "list") {
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
      {
        const resolved = resolveSessionFromArg(arg);
        if (!resolved) {
          applyOutputBlock({ kind: "status", tone: "error", text: `Session not found: ${arg}` });
          return true;
        }
        await switchToSession(resolved);
        applyOutputBlock({
          kind: "status",
          tone: "success",
          text: `Session switched: ${resolved.id}`,
        });
        return true;
      }
    default:
      return false;
  }
}

async function executeSharedUiAction(actionId, arg = "") {
  switch (actionId) {
    case "abort_execution":
      if (arg) return false;
      if (!state.streaming) {
        applyOutputBlock({
          kind: "status",
          tone: "warning",
          text: "No active run to abort. Use /abort while a response is running.",
        });
        return true;
      }
      if (state.abortRequested) {
        applyOutputBlock({
          kind: "status",
          tone: "warning",
          text: "Cancellation already requested.",
        });
        return true;
      }
      await abortCurrentExecution();
      return true;
    case "show_help":
      applyOutputBlock({
        kind: "message",
        phase: "full",
        role: "system",
        title: "commands",
        text: sharedHelpLines().join("\n"),
      });
      return true;
    case "show_status":
      renderStatusMessage();
      return true;
    case "new_session":
      if (arg) return false;
      await runUiAction("creating session", async () => {
        await createAndSelectSession();
      });
      return true;
    case "fork_session":
      if (arg) return false;
      await runUiAction("forking session", async () => {
        await forkCurrentSession();
      });
      return true;
    case "compact_session":
      if (arg) return false;
      await runUiAction("compacting session", async () => {
        await compactCurrentSession();
      });
      return true;
    case "rename_session":
      if (arg) return false;
      await runUiAction("renaming session", async () => {
        await renameCurrentSession();
      });
      return true;
    case "share_session":
      if (arg) return false;
      await runUiAction("sharing session", async () => {
        const current = currentSession();
        if (current && current.share_url) {
          applyOutputBlock({
            kind: "status",
            tone: "warning",
            text: "Session is already shared. Use /unshare to revoke the link.",
          });
          return;
        }
        await toggleShareCurrentSession();
      });
      return true;
    case "unshare_session":
      if (arg) return false;
      await runUiAction("unsharing session", async () => {
        const current = currentSession();
        if (!current || !current.share_url) {
          applyOutputBlock({
            kind: "status",
            tone: "warning",
            text: "Session is not currently shared.",
          });
          return;
        }
        await toggleShareCurrentSession();
      });
      return true;
    case "copy_session":
      if (arg) return false;
      await runUiAction("copying session", async () => {
        await copyCurrentSessionTranscript();
      });
      return true;
    case "navigate_parent_session":
      if (arg) return false;
      return navigateParentSessionView();
    case "toggle_command_palette":
      if (arg) return false;
      openCommandPanel("model");
      return true;
    default:
      return false;
  }
}

async function handleSlashCommand(input) {
  const trimmed = input.trim();
  if (!trimmed.startsWith("/")) return false;

  const resolved = await resolveUiCommandInvocation(trimmed);
  const sharedActionId = resolved && resolved.action_id ? String(resolved.action_id) : "";
  const arg = resolved && resolved.argument ? String(resolved.argument) : "";

  if (
    interactionLocked() &&
    sharedActionId !== "abort_execution" &&
    sharedActionId !== "show_help" &&
    sharedActionId !== "show_status"
  ) {
    applyOutputBlock({
      kind: "status",
      tone: "warning",
      text: state.streaming
        ? "A response is running. Use /abort to cancel or wait until it finishes."
        : "Another action is running. Wait until it finishes.",
    });
    return true;
  }

  if (resolved) {
    if (await executeParameterizedUiCommand(sharedActionId, resolved.argument_kind, arg)) {
      return true;
    }
    if (await executeSharedUiAction(sharedActionId, arg)) {
      return true;
    }
  }

  return false;
}
