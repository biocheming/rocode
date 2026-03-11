// ── Slash Commands ─────────────────────────────────────────────────────────

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

  if (
    interactionLocked() &&
    name !== "help" &&
    name !== "commands" &&
    name !== "abort" &&
    name !== "status"
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
        "/agent <name|auto>   switch agent mode",
        "/preset <name|auto>   switch preset mode",
        "/abort   cancel current run or stage",
        "/status   show runtime status",
        "/session <id|list|new|fork|compact|delete>   manage session",
      ].join("\n"),
    });
    return true;
  }

  if (name === "abort") {
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
  }

  if (name === "status" || name === "stats") {
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
    updateComposerMeta();
    updateSessionRuntimeMeta(currentSession());
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

  if (name === "mode" || name === "agent" || name === "preset") {
    if (!arg) {
      openCommandPanel(name === "preset" ? "mode" : "mode");
      return true;
    }
    if (arg === "auto") {
      setSelectedMode(null);
      applyOutputBlock({ kind: "status", tone: "success", text: "Mode set to auto" });
      return true;
    }
    const lowerArg = arg.toLowerCase();
    const found = state.modes.find((mode) => {
      if (name === "agent" && mode.kind !== "agent") return false;
      if (name === "preset" && mode.kind !== "preset" && mode.kind !== "profile") return false;
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
