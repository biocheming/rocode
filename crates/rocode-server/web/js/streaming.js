// ── SSE Streaming & Prompt ──────────────────────────────────────────────────

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
  state.abortRequested = false;
  syncInteractionState();
  setBadge("running", "warn");
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
        if (payload && (payload.kind === "scheduler_stage" || payload.kind === "tool")) {
          scheduleExecutionTopologyRefresh();
        }
        return;
      }

      if (name === "question.created") {
        const sessionId = payload.sessionID || payload.sessionId;
        if (!sessionId || sessionId === state.selectedSession) {
          openQuestionPanel(interactionFromLiveQuestionEvent(payload));
          scheduleExecutionTopologyRefresh();
        }
        return;
      }

      if (name === "question.replied" || name === "question.rejected") {
        const requestId = payload.requestID || payload.requestId;
        if (
          state.activeQuestionInteraction &&
          state.activeQuestionInteraction.request_id &&
          state.activeQuestionInteraction.request_id === requestId
        ) {
          closeQuestionPanel();
          void loadMessages();
        }
        scheduleExecutionTopologyRefresh();
        return;
      }

      if (name === "usage") {
        applyStreamUsage(payload);
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
    state.abortRequested = false;
    await refreshExecutionTopology().catch(() => {});
    syncInteractionState();
    if (!state.busyAction) {
      setBadge("ready", "ok");
    }
  }
}
