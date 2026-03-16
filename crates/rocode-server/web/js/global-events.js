// ── Global ServerEvent Subscription ────────────────────────────────────────

let globalServerEventStarted = false;
let globalServerEventGeneration = 0;
let globalServerEventReconnectTimer = null;
let globalSessionRefreshTimer = null;
let globalMessageRefreshTimer = null;
let globalSessionSnapshotTimer = null;
let globalConfigRefreshTimer = null;

function globalServerEventType(name, payload) {
  if (name && name !== "message") return name;
  return payload && payload.type ? payload.type : "message";
}

function globalServerEventSessionId(payload) {
  return payload && (
    payload.sessionID ||
    payload.sessionId ||
    payload.parentID ||
    payload.parentId ||
    payload.childID ||
    payload.childId
  );
}

function scheduleGlobalSessionIndexRefresh(delay = 180) {
  if (globalSessionRefreshTimer) {
    clearTimeout(globalSessionRefreshTimer);
  }
  globalSessionRefreshTimer = setTimeout(() => {
    globalSessionRefreshTimer = null;
    const previousSelectedSession = state.selectedSession;
    void refreshSessionsIndex()
      .then(() => {
        if (!state.streaming && previousSelectedSession !== state.selectedSession) {
          return loadMessages();
        }
        return null;
      })
      .catch(() => {});
  }, delay);
}

function scheduleGlobalSelectedMessagesRefresh(delay = 180) {
  if (!state.selectedSession || state.streaming) return;
  if (globalMessageRefreshTimer) {
    clearTimeout(globalMessageRefreshTimer);
  }
  globalMessageRefreshTimer = setTimeout(() => {
    globalMessageRefreshTimer = null;
    if (!state.streaming) {
      void loadMessages().catch(() => {});
    }
  }, delay);
}

function scheduleGlobalSelectedSessionSnapshotRefresh(delay = 120) {
  if (!state.selectedSession) return;
  if (globalSessionSnapshotTimer) {
    clearTimeout(globalSessionSnapshotTimer);
  }
  globalSessionSnapshotTimer = setTimeout(() => {
    globalSessionSnapshotTimer = null;
    void refreshSessionSnapshot().catch(() => {});
  }, delay);
}

function scheduleGlobalConfigRefresh(delay = 250) {
  if (globalConfigRefreshTimer) {
    clearTimeout(globalConfigRefreshTimer);
  }
  globalConfigRefreshTimer = setTimeout(() => {
    globalConfigRefreshTimer = null;
    void Promise.all([loadProviders(), loadModes(), loadUiCommands()])
      .then(() => loadWebUiPreferences())
      .then(() => {
        if (nodes.commandPanel && !nodes.commandPanel.classList.contains("hidden")) {
          return loadSettingsWorkspace({ force: true });
        }
        return null;
      })
      .catch(() => {});
  }, delay);
}

function maybeOpenGlobalQuestion(payload) {
  const sessionId = payload.sessionID || payload.sessionId;
  if (!sessionId || sessionId !== state.selectedSession) return;

  const interaction = interactionFromLiveQuestionEvent(payload);
  if (!interaction || !interaction.request_id) return;
  if (
    state.activeQuestionInteraction &&
    state.activeQuestionInteraction.request_id === interaction.request_id
  ) {
    return;
  }

  openQuestionPanel(interaction);
}

function maybeResolveGlobalQuestion(payload) {
  const requestId = payload.requestID || payload.requestId;
  if (
    requestId &&
    state.activeQuestionInteraction &&
    state.activeQuestionInteraction.request_id === requestId
  ) {
    closeQuestionPanel();
  }
}

function maybeOpenGlobalPermission(payload) {
  const interaction = permissionInteractionFromLiveEvent(payload);
  if (!interaction || !interaction.permission_id) return;
  if (!interaction.session_id || interaction.session_id !== state.selectedSession) return;
  if (
    state.activePermissionInteraction &&
    state.activePermissionInteraction.permission_id === interaction.permission_id
  ) {
    return;
  }

  openPermissionPanel(interaction);
}

function maybeResolveGlobalPermission(payload) {
  const permissionId = payload.permissionID || payload.permissionId || payload.requestID || payload.requestId;
  if (
    permissionId &&
    state.activePermissionInteraction &&
    state.activePermissionInteraction.permission_id === permissionId
  ) {
    closePermissionPanel();
  }
}

function handleGlobalServerEvent(name, payload) {
  const type = globalServerEventType(name, payload);

  if (type === "message") {
    return;
  }

  if (type === "output_block") {
    if (state.streaming) return;
    const handled = applyOutputBlockEvent(payload);
    if (!handled && !applyFocusedChildOutputBlockEvent(payload)) return;
    const block = payload && payload.block ? payload.block : payload;
    if (block && (block.kind === "scheduler_stage" || block.kind === "tool")) {
      scheduleExecutionTopologyRefresh(60);
    }
    return;
  }

  if (type === "usage") {
    if (!state.streaming && globalServerEventSessionId(payload) === state.selectedSession) {
      applyStreamUsage(payload);
    }
    return;
  }

  if (type === "error") {
    if (!state.streaming && globalServerEventSessionId(payload) === state.selectedSession) {
      applyOutputBlock({ kind: "status", tone: "error", text: payload.error || payload.message || "Stream error" });
    }
    return;
  }

  if (type === "session.updated") {
    scheduleGlobalSessionIndexRefresh();
    return;
  }

  if (type === "session.status") {
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleGlobalSelectedSessionSnapshotRefresh(80);
    }
    return;
  }

  if (type === "execution.topology.changed") {
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
    }
    return;
  }

  if (type === "question.created") {
    maybeOpenGlobalQuestion(payload);
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
    }
    return;
  }

  if (
    type === "question.resolved" ||
    type === "question.replied" ||
    type === "question.rejected"
  ) {
    maybeResolveGlobalQuestion(payload);
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
      scheduleGlobalSelectedMessagesRefresh(120);
    }
    return;
  }

  if (type === "permission.requested") {
    maybeOpenGlobalPermission(payload);
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
    }
    return;
  }

  if (type === "permission.resolved" || type === "permission.replied") {
    maybeResolveGlobalPermission(payload);
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
      scheduleGlobalSelectedMessagesRefresh(120);
    }
    return;
  }

  if (type === "config.updated") {
    scheduleGlobalConfigRefresh();
    return;
  }

  if (type === "child_session.attached" || type === "child_session.detached") {
    const parentId = payload.parentID || payload.parentId;
    const childId = payload.childID || payload.childId;
    scheduleGlobalSessionIndexRefresh();
    if (state.selectedSession && (state.selectedSession === parentId || state.selectedSession === childId)) {
      scheduleExecutionTopologyRefresh(60);
      scheduleGlobalSelectedMessagesRefresh(120);
    }
    return;
  }

  if (type === "tool_call.lifecycle" || type === "diff.updated") {
    if (globalServerEventSessionId(payload) === state.selectedSession) {
      scheduleExecutionTopologyRefresh(60);
    }
  }
}

function startGlobalServerEventStream() {
  if (globalServerEventStarted) return;

  globalServerEventStarted = true;
  globalServerEventGeneration += 1;
  const generation = globalServerEventGeneration;

  void (async () => {
    while (globalServerEventStarted && generation === globalServerEventGeneration) {
      try {
        const response = await fetch("/event", {
          headers: {
            Accept: "text/event-stream",
          },
        });

        if (!response.ok) {
          throw new Error(`${response.status} ${response.statusText}`);
        }

        await parseSSE(response, (eventName, eventPayload) => {
          handleGlobalServerEvent(eventName, eventPayload);
        });
      } catch (error) {
        console.warn("Global ServerEvent stream disconnected", error);
      }

      if (!globalServerEventStarted || generation !== globalServerEventGeneration) {
        break;
      }

      await new Promise((resolve) => {
        globalServerEventReconnectTimer = setTimeout(() => {
          globalServerEventReconnectTimer = null;
          resolve();
        }, 1500);
      });
    }
  })();
}
