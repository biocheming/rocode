// ── Execution & Recovery Panels ────────────────────────────────────────────

function childSessionIdForNode(node) {
  if (!node) return null;
  if (node.child_session_id) return node.child_session_id;
  const meta = node.metadata || {};
  return meta.child_session_id || meta.childSessionId || null;
}

function childSessionIdsFromTopology(topology = state.executionTopology) {
  return new Set(
    uniqueChildSessionEntries(collectChildSessionEntries((topology && topology.roots) || [])).map(
      (entry) => entry.childSessionId
    )
  );
}

function ensureChildSessionLiveState(sessionId) {
  if (!state.childSessionLiveBlocks.has(sessionId)) {
    state.childSessionLiveBlocks.set(sessionId, []);
  }
  return state.childSessionLiveBlocks.get(sessionId);
}

function childSessionLiveSummary(sessionId) {
  const entries = state.childSessionLiveBlocks.get(sessionId) || [];
  const last = [...entries].reverse().find((entry) => entry && entry.text && String(entry.text).trim());
  return last ? String(last.text).trim() : null;
}

function upsertChildSessionLiveBlock(sessionId, block) {
  if (!sessionId || !block || !block.kind) return;
  const entries = ensureChildSessionLiveState(sessionId);

  if (block.kind === "message" || block.kind === "reasoning") {
    const streamKind = block.kind;
    const streamRole = block.role || null;
    if (block.phase === "start") {
      entries.push({ kind: streamKind, role: streamRole, text: "" });
      return;
    }
    if (block.phase === "delta") {
      const last = entries[entries.length - 1];
      if (last && last.kind === streamKind && (last.role || null) === streamRole) {
        last.text = `${last.text || ""}${block.text || ""}`;
      } else {
        entries.push({ kind: streamKind, role: streamRole, text: block.text || "" });
      }
      return;
    }
    if (block.phase === "end") {
      return;
    }
    entries.push({ kind: streamKind, role: streamRole, text: block.text || "" });
    return;
  }

  if (block.kind === "tool") {
    const label = [block.name || "tool", block.phase || "running"].filter(Boolean).join(" · ");
    const detail = block.output || block.text || block.input || "";
    entries.push({ kind: "tool", text: detail ? `${label}\n${detail}` : label });
    return;
  }

  if (block.kind === "scheduler_stage") {
    const title = schedulerStageTitle(block);
    const detail = schedulerStageText(block) || block.activity || block.last_event || "";
    entries.push({ kind: "stage", text: detail ? `${title}\n${detail}` : title });
    return;
  }

  if (block.kind === "status") {
    entries.push({ kind: "status", text: block.text || "status" });
  }
}

function focusedChildTranscriptMarkup(sessionId) {
  const entries = state.childSessionLiveBlocks.get(sessionId) || [];
  if (!entries.length) {
    return `<div class="focused-child-empty">Waiting for live child output…</div>`;
  }

  return entries
    .slice(-12)
    .map((entry) => {
      const label =
        entry.kind === "reasoning"
          ? "thinking"
          : entry.kind === "message"
            ? entry.role || "assistant"
            : entry.kind;
      return `
        <article class="focused-child-entry focused-child-entry-${escapeHtml(entry.kind)}">
          <div class="focused-child-entry-head">${escapeHtml(label)}</div>
          <div class="focused-child-entry-body">${escapeHtml(entry.text || "")}</div>
        </article>
      `;
    })
    .join("");
}

function applyFocusedChildOutputBlockEvent(payload) {
  if (!payload) return false;
  const sessionId = payload.sessionID || payload.sessionId || null;
  if (!sessionId || sessionId === state.selectedSession || sessionId !== state.focusedChildSessionId) {
    return false;
  }
  if (!childSessionIdsFromTopology().has(sessionId)) {
    return false;
  }
  const block = payload && payload.block ? payload.block : payload;
  upsertChildSessionLiveBlock(sessionId, block);
  renderChildSessionRail(state.executionTopology);
  return true;
}

function collectChildSessionEntries(nodes, into = []) {
  for (const node of nodes || []) {
    const childSessionId = childSessionIdForNode(node);
    if (childSessionId) {
      into.push({
        childSessionId,
        label: node.label || humanExecutionKind(node.kind),
        kind: node.kind || "agent_task",
        status: node.status || "running",
        waitingOn: node.waiting_on || null,
        recentEvent: node.recent_event || null,
        stageId: node.stage_id || null,
        updatedAt: node.updated_at || Date.now(),
      });
    }
    if (Array.isArray(node.children) && node.children.length > 0) {
      collectChildSessionEntries(node.children, into);
    }
  }
  return into;
}

function uniqueChildSessionEntries(entries) {
  const byId = new Map();
  for (const entry of entries) {
    const existing = byId.get(entry.childSessionId);
    if (!existing || Number(entry.updatedAt) >= Number(existing.updatedAt)) {
      byId.set(entry.childSessionId, entry);
    }
  }
  return Array.from(byId.values()).sort((a, b) => Number(b.updatedAt) - Number(a.updatedAt));
}

function childSessionStatusTone(status) {
  if (status === "done") return "success";
  if (status === "waiting" || status === "retry") return "warning";
  if (status === "cancelling") return "warning";
  return "running";
}

function renderChildSessionRail(topology) {
  const rail = nodes.childSessionRail;
  const focusPanel = nodes.focusedChildPanel;
  if (!rail || !focusPanel) return;

  const entries = topology ? uniqueChildSessionEntries(collectChildSessionEntries(topology.roots || [])) : [];
  if (!entries.length) {
    state.focusedChildSessionId = null;
    rail.classList.add("hidden");
    rail.innerHTML = "";
    focusPanel.classList.add("hidden");
    focusPanel.innerHTML = "";
    return;
  }

  rail.classList.remove("hidden");
  if (!state.focusedChildSessionId || !entries.some((entry) => entry.childSessionId === state.focusedChildSessionId)) {
    state.focusedChildSessionId = entries[0].childSessionId;
  }

  rail.innerHTML = `
    <div class="child-session-rail-head">
      <div>
        <div class="child-session-rail-kicker">Child Sessions</div>
        <h5>${entries.length} active branch${entries.length === 1 ? "" : "es"}</h5>
      </div>
      <p>Compressed by default. Focus one when you need detail.</p>
    </div>
    <div class="child-session-grid">
      ${entries
        .map((entry) => {
          const active = entry.childSessionId === state.focusedChildSessionId ? " active" : "";
          return `
            <button class="child-session-card${active}" type="button" data-child-focus="${escapeHtml(entry.childSessionId)}">
              <div class="child-session-card-head">
                <span class="child-session-name">${escapeHtml(short(entry.label || entry.childSessionId, 28))}</span>
                <span class="badge ${childSessionStatusTone(entry.status)}">${escapeHtml(entry.status)}</span>
              </div>
              <div class="child-session-card-meta">
                <span>${escapeHtml(entry.kind)}</span>
                ${entry.stageId ? `<span>${escapeHtml(short(entry.stageId, 18))}</span>` : ""}
              </div>
              <div class="child-session-card-summary">
                ${escapeHtml(childSessionLiveSummary(entry.childSessionId) || entry.recentEvent || entry.waitingOn || "Live output available")}
              </div>
            </button>
          `;
        })
        .join("")}
    </div>
  `;

  rail.querySelectorAll("[data-child-focus]").forEach((button) => {
    button.addEventListener("click", () => {
      state.focusedChildSessionId = button.dataset.childFocus || null;
      renderChildSessionRail(topology);
    });
  });

  const focused = entries.find((entry) => entry.childSessionId === state.focusedChildSessionId) || entries[0];
  focusPanel.classList.remove("hidden");
  focusPanel.innerHTML = `
    <div class="focused-child-head">
      <div>
        <div class="focused-child-kicker">Focused Child</div>
        <h5>${escapeHtml(focused.label || focused.childSessionId)}</h5>
      </div>
      <div class="focused-child-actions">
        <span class="badge ${childSessionStatusTone(focused.status)}">${escapeHtml(focused.status)}</span>
        <button class="btn btn-secondary" type="button" data-open-focused-child>Open Session</button>
      </div>
    </div>
    <div class="focused-child-grid">
      <div class="focused-child-field">
        <span class="focused-child-label">Execution</span>
        <span>${escapeHtml(focused.kind)}</span>
      </div>
      <div class="focused-child-field">
        <span class="focused-child-label">Waiting</span>
        <span>${escapeHtml(focused.waitingOn || "\u2014")}</span>
      </div>
      <div class="focused-child-field">
        <span class="focused-child-label">Recent</span>
        <span>${escapeHtml(focused.recentEvent || "Live output available")}</span>
      </div>
      <div class="focused-child-field">
        <span class="focused-child-label">Session ID</span>
        <span>${escapeHtml(short(focused.childSessionId, 24))}</span>
      </div>
    </div>
    <div class="focused-child-transcript">${focusedChildTranscriptMarkup(focused.childSessionId)}</div>
  `;

  const openButton = focusPanel.querySelector("[data-open-focused-child]");
  if (openButton) {
    openButton.addEventListener("click", () => {
      if (!focused.childSessionId) return;
      state.parentSessionId = state.selectedSession;
      state.selectedSession = focused.childSessionId;
      void loadMessages();
      renderProjects();
      syncInteractionState();
    });
  }
}

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
    renderChildSessionRail(topology);
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
    renderChildSessionRail(null);
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
    renderChildSessionRail(topology);
    renderExecutionPanel(topology);
    renderRecoveryPanel(recovery);
    updateRuntimeChrome();
    refreshStageInspector();
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
    renderChildSessionRail(topology);
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
