// ── Execution & Recovery Panels ────────────────────────────────────────────

async function refreshSessionSnapshot(sessionId = state.selectedSession) {
  if (!sessionId) return null;
  const [sessionResponse, topologyResponse, recoveryResponse] = await Promise.all([
    api(`/session/${sessionId}`),
    api(`/session/${sessionId}/executions`),
    api(`/session/${sessionId}/recovery`),
  ]);
  const session = upsertSessionSnapshot(await sessionResponse.json());
  const topology = await topologyResponse.json();
  const recovery = await recoveryResponse.json();
  if (session.id === state.selectedSession) {
    state.executionTopology = topology;
    state.recoveryProtocol = recovery;
    updateSessionMeta(session);
    updateComposerMeta();
    renderExecutionPanel(topology);
    renderRecoveryPanel(recovery);
    updateRuntimeChrome();
  }
  return session;
}

async function refreshExecutionTopology(sessionId = state.selectedSession) {
  if (!sessionId) {
    state.executionTopology = null;
    state.recoveryProtocol = null;
    renderExecutionPanel(null);
    renderRecoveryPanel(null);
    return null;
  }
  const [topologyResponse, recoveryResponse] = await Promise.all([
    api(`/session/${sessionId}/executions`),
    api(`/session/${sessionId}/recovery`),
  ]);
  const topology = await topologyResponse.json();
  const recovery = await recoveryResponse.json();
  if (sessionId === state.selectedSession) {
    state.executionTopology = topology;
    state.recoveryProtocol = recovery;
    updateSessionRuntimeMeta(currentSession());
    renderExecutionPanel(topology);
    renderRecoveryPanel(recovery);
    updateRuntimeChrome();
  }
  return topology;
}

function scheduleExecutionTopologyRefresh(delay = 120) {
  if (!state.selectedSession) return;
  if (state.executionRefreshTimer) {
    clearTimeout(state.executionRefreshTimer);
  }
  state.executionRefreshTimer = setTimeout(() => {
    state.executionRefreshTimer = null;
    void refreshExecutionTopology().catch(() => {});
  }, delay);
}

function executionSummaryText(topology) {
  if (!topology || !topology.active_count) return "No active execution";
  return `${topology.active_count} active · ${topology.running_count} running · ${topology.waiting_count} waiting`;
}

function executionStatusTone(status) {
  if (status === "waiting" || status === "cancelling" || status === "retry") return "warn";
  if (status === "running") return "ok";
  return "";
}

function humanExecutionKind(kind) {
  switch (kind) {
    case "prompt_run":
      return "prompt";
    case "scheduler_run":
      return "scheduler";
    case "scheduler_stage":
      return "stage";
    case "tool_call":
      return "tool";
    case "agent_task":
      return "agent task";
    case "question":
      return "question";
    default:
      return kind || "execution";
  }
}

function renderExecutionNode(node) {
  const children = Array.isArray(node.children) ? node.children : [];
  const statusTone = executionStatusTone(node.status);
  const meta = [];
  if (node.waiting_on) meta.push(`waiting ${node.waiting_on}`);
  if (node.recent_event) meta.push(node.recent_event);
  return `
    <li class="execution-node">
      <div class="execution-node-row">
        <span class="execution-kind">${escapeHtml(humanExecutionKind(node.kind))}</span>
        <span class="execution-label">${escapeHtml(node.label || node.id)}</span>
        <span class="badge ${statusTone}">${escapeHtml(node.status || "running")}</span>
      </div>
      ${
        meta.length
          ? `<div class="execution-node-meta">${meta.map((item) => `<span>${escapeHtml(item)}</span>`).join("<span>•</span>")}</div>`
          : ""
      }
      ${children.length ? `<ul class="execution-tree">${children.map(renderExecutionNode).join("")}</ul>` : ""}
    </li>
  `;
}

function renderExecutionPanel(topology) {
  if (!nodes.executionPanel) return;
  if (!topology || !topology.active_count) {
    nodes.executionPanel.classList.add("hidden");
    nodes.executionPanel.innerHTML = "";
    return;
  }
  nodes.executionPanel.classList.remove("hidden");
  nodes.executionPanel.innerHTML = `
    <div class="execution-panel-head">
      <div>
        <p class="label">Execution Topology</p>
        <h4>${escapeHtml(executionSummaryText(topology))}</h4>
      </div>
      <div class="execution-summary-chips">
        <span class="meta-pill"><span class="meta-label">running</span><span>${topology.running_count}</span></span>
        <span class="meta-pill"><span class="meta-label">waiting</span><span>${topology.waiting_count}</span></span>
        <span class="meta-pill"><span class="meta-label">cancelling</span><span>${topology.cancelling_count}</span></span>
      </div>
    </div>
    <ul class="execution-tree">${(topology.roots || []).map(renderExecutionNode).join("")}</ul>
  `;
}

function renderRecoveryPanel(recovery) {
  if (!nodes.recoveryPanel) return;
  if (!recovery) {
    nodes.recoveryPanel.classList.add("hidden");
    nodes.recoveryPanel.innerHTML = "";
    return;
  }
  const actions = Array.isArray(recovery.actions) ? recovery.actions : [];
  const checkpoints = Array.isArray(recovery.checkpoints) ? recovery.checkpoints : [];
  if (!actions.length && !checkpoints.length && recovery.status === "idle") {
    nodes.recoveryPanel.classList.add("hidden");
    nodes.recoveryPanel.innerHTML = "";
    return;
  }
  nodes.recoveryPanel.classList.remove("hidden");
  nodes.recoveryPanel.innerHTML = `
    <div class="execution-panel-head">
      <div>
        <p class="label">Recovery Protocol</p>
        <h4>${escapeHtml(recovery.status || "idle")}</h4>
        ${recovery.summary ? `<p class="muted">${escapeHtml(recovery.summary)}</p>` : ""}
      </div>
      <div class="execution-summary-chips">
        <span class="meta-pill"><span class="meta-label">actions</span><span>${actions.length}</span></span>
        <span class="meta-pill"><span class="meta-label">checkpoints</span><span>${checkpoints.length}</span></span>
      </div>
    </div>
    ${
      actions.length
        ? `<div class="recovery-actions">${actions
            .map((action, index) => `
            <div class="recovery-item">
              <h5>${escapeHtml(action.label || action.kind || "action")}</h5>
              <p>${escapeHtml(action.description || "")}</p>
              <div class="recovery-item-actions">
                <button
                  class="command-action-btn"
                  type="button"
                  data-recovery-action="${escapeHtml(action.kind || "")}"
                  data-recovery-target-id="${escapeHtml(action.target_id || "")}"
                  data-recovery-index="${index + 1}"
                  ${state.busyAction ? "disabled" : ""}
                >
                  Run
                </button>
              </div>
            </div>`)
            .join("")}</div>`
        : ""
    }
    ${
      checkpoints.length
        ? `<div class="recovery-checkpoints" style="margin-top:12px;">${checkpoints
            .slice(0, 4)
            .map(
              (checkpoint) => `
            <div class="recovery-item">
              <h5>${escapeHtml(checkpoint.kind)} · ${escapeHtml(checkpoint.label)}</h5>
              <p>${escapeHtml(checkpoint.status)}${checkpoint.summary ? ` · ${escapeHtml(checkpoint.summary)}` : ""}</p>
            </div>`
            )
            .join("")}</div>`
        : ""
    }
  `;
}

async function executeRecoveryAction(action, targetId = null, label = "recovery") {
  if (!state.selectedSession) return;
  await runUiAction(`recovery ${label.toLowerCase()}`, async () => {
    const response = await api(`/session/${state.selectedSession}/recovery/execute`, {
      method: "POST",
      body: JSON.stringify({
        action,
        target_id: targetId || null,
      }),
    });
    const result = await response.json();
    applyOutputBlock({
      kind: "status",
      tone: "success",
      text: `Recovery action started: ${label}`,
    });
    await refreshSessionSnapshot(state.selectedSession);
    return result;
  });
}

async function handleRecoveryPanelClick(event) {
  const button = event.target.closest("[data-recovery-action]");
  if (!button) return;
  const action = button.dataset.recoveryAction;
  if (!action) return;
  const targetId = button.dataset.recoveryTargetId || null;
  const label =
    button.closest(".recovery-item")?.querySelector("h5")?.textContent?.trim() || action;
  await executeRecoveryAction(action, targetId, label);
}
