// ── Bottom Terminal Panel ───────────────────────────────────────────────────

function defaultTerminalCommand() {
  return "/bin/bash";
}

function terminalWsUrl(sessionId) {
  const protocol = globalThis.location && globalThis.location.protocol === "https:" ? "wss:" : "ws:";
  const host = globalThis.location && globalThis.location.host ? globalThis.location.host : "127.0.0.1:3000";
  return `${protocol}//${host}/pty/${encodeURIComponent(sessionId)}/connect?cursor=-1`;
}

function terminalSessionById(sessionId) {
  return state.terminalSessions.find((session) => session.id === sessionId) || null;
}

function setTerminalOpen(open) {
  state.terminalOpen = Boolean(open);
  if (nodes.terminalPanel) {
    nodes.terminalPanel.classList.toggle("hidden", !state.terminalOpen);
  }
  if (nodes.terminalToggleBtn) {
    nodes.terminalToggleBtn.classList.toggle("active", state.terminalOpen);
  }
}

function setWorkspaceOpen(open) {
  if (!nodes.workspacePanel || !nodes.workspaceToggleBtn) return;
  nodes.workspacePanel.classList.toggle("hidden", !open);
  nodes.workspaceToggleBtn.classList.toggle("active", open);
}

function ensureTerminalSelection() {
  if (!state.terminalActiveId && state.terminalSessions.length > 0) {
    state.terminalActiveId = state.terminalSessions[0].id;
  }
  if (
    state.terminalActiveId &&
    !state.terminalSessions.some((session) => session.id === state.terminalActiveId)
  ) {
    state.terminalActiveId = state.terminalSessions.length > 0 ? state.terminalSessions[0].id : null;
  }
}

function appendTerminalOutput(sessionId, chunk) {
  const current = state.terminalBuffers.get(sessionId) || "";
  const next = `${current}${chunk}`;
  state.terminalBuffers.set(sessionId, next.slice(-200000));
  if (sessionId === state.terminalActiveId) {
    renderTerminalViewport();
  }
}

function renderTerminalTabs() {
  if (!nodes.terminalTabList) return;
  nodes.terminalTabList.innerHTML = "";

  if (state.terminalSessions.length === 0) {
    const empty = document.createElement("div");
    empty.className = "terminal-tab-empty";
    empty.textContent = "No terminals";
    nodes.terminalTabList.appendChild(empty);
    return;
  }

  for (const session of state.terminalSessions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "terminal-tab";
    if (session.id === state.terminalActiveId) {
      button.classList.add("active");
    }
    button.dataset.terminalId = session.id;
    button.innerHTML = `
      <span class="terminal-tab-label">${escapeHtml(short(session.command || session.id, 18))}</span>
      <span class="terminal-tab-status">${escapeHtml(session.status || "running")}</span>
      <span class="terminal-tab-close" data-terminal-close="${escapeHtml(session.id)}">×</span>
    `;
    nodes.terminalTabList.appendChild(button);
  }
}

function renderTerminalViewport() {
  if (!nodes.terminalViewport) return;

  const activeId = state.terminalActiveId;
  if (!activeId) {
    nodes.terminalViewport.innerHTML = `<div class="terminal-empty">Open a terminal to inspect the workspace.</div>`;
    return;
  }

  const buffer = state.terminalBuffers.get(activeId) || "";
  const session = terminalSessionById(activeId);
  const pre = document.createElement("pre");
  pre.className = "terminal-screen";
  pre.textContent = buffer || `Connected to ${session ? session.command : activeId}\n`;
  nodes.terminalViewport.innerHTML = "";
  nodes.terminalViewport.appendChild(pre);
  nodes.terminalViewport.scrollTop = nodes.terminalViewport.scrollHeight;
}

function connectTerminalSession(sessionId) {
  if (!sessionId || typeof WebSocket === "undefined") return;
  if (state.terminalSockets.has(sessionId)) return;

  const socket = new WebSocket(terminalWsUrl(sessionId));
  socket.binaryType = "arraybuffer";

  socket.addEventListener("open", () => {
    appendTerminalOutput(sessionId, "");
  });

  socket.addEventListener("message", (event) => {
    if (!(event.data instanceof ArrayBuffer)) {
      appendTerminalOutput(sessionId, String(event.data || ""));
      return;
    }
    const bytes = new Uint8Array(event.data);
    if (bytes.length > 0 && bytes[0] === 0x00) {
      try {
        const meta = JSON.parse(new TextDecoder().decode(bytes.slice(1)));
        if (meta && typeof meta.cursor === "number") {
          state.terminalCursorById.set(sessionId, meta.cursor);
        }
      } catch (_) {}
      return;
    }
    appendTerminalOutput(sessionId, new TextDecoder().decode(bytes));
  });

  socket.addEventListener("close", () => {
    state.terminalSockets.delete(sessionId);
  });

  socket.addEventListener("error", () => {
    appendTerminalOutput(sessionId, "\n[terminal connection error]\n");
  });

  state.terminalSockets.set(sessionId, socket);
}

function setActiveTerminal(sessionId) {
  state.terminalActiveId = sessionId;
  renderTerminalTabs();
  renderTerminalViewport();
  connectTerminalSession(sessionId);
}

async function loadTerminalSessions() {
  const response = await api("/pty");
  const items = await response.json();
  state.terminalSessions = Array.isArray(items) ? items : [];
  ensureTerminalSelection();
  renderTerminalTabs();
  renderTerminalViewport();
  if (state.terminalActiveId) {
    connectTerminalSession(state.terminalActiveId);
  }
}

async function createTerminalSession() {
  const current = currentSession();
  const response = await api("/pty", {
    method: "POST",
    body: JSON.stringify({
      command: defaultTerminalCommand(),
      cwd: current && current.directory ? current.directory : ".",
    }),
  });
  const session = await response.json();
  state.terminalSessions = [session, ...state.terminalSessions.filter((entry) => entry.id !== session.id)];
  state.terminalBuffers.set(session.id, "");
  setTerminalOpen(true);
  setActiveTerminal(session.id);
}

async function deleteTerminalSession(sessionId) {
  if (!sessionId) return;
  const socket = state.terminalSockets.get(sessionId);
  if (socket) {
    socket.close();
    state.terminalSockets.delete(sessionId);
  }
  await api(`/pty/${encodeURIComponent(sessionId)}`, { method: "DELETE" });
  state.terminalSessions = state.terminalSessions.filter((session) => session.id !== sessionId);
  state.terminalBuffers.delete(sessionId);
  state.terminalCursorById.delete(sessionId);
  ensureTerminalSelection();
  renderTerminalTabs();
  renderTerminalViewport();
}

async function handleTerminalInputSubmit(event) {
  event.preventDefault();
  const value = nodes.terminalInput ? String(nodes.terminalInput.value || "") : "";
  if (!value.trim() || !state.terminalActiveId) return;

  const socket = state.terminalSockets.get(state.terminalActiveId);
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(`${value}\n`);
    appendTerminalOutput(state.terminalActiveId, `$ ${value}\n`);
    nodes.terminalInput.value = "";
  }
}

function handleTerminalTabClick(event) {
  const closeTarget =
    event.target && event.target.dataset ? event.target.dataset.terminalClose : null;
  if (closeTarget) {
    void deleteTerminalSession(closeTarget).catch((error) => {
      appendTerminalOutput(closeTarget, `\n[delete failed: ${String(error)}]\n`);
    });
    return;
  }
  let node = event.target || null;
  while (node) {
    if (node.dataset && node.dataset.terminalId) {
      setActiveTerminal(node.dataset.terminalId);
      return;
    }
    node = node.parentNode || null;
  }
}

function initTerminalPanel() {
  setTerminalOpen(false);
  setWorkspaceOpen(true);

  if (nodes.terminalToggleBtn) {
    nodes.terminalToggleBtn.addEventListener("click", () => {
      const next = !state.terminalOpen;
      setTerminalOpen(next);
      if (next && state.terminalSessions.length === 0) {
        void loadTerminalSessions().catch(() => {});
      }
    });
  }

  if (nodes.workspaceToggleBtn) {
    nodes.workspaceToggleBtn.addEventListener("click", () => {
      const isHidden = nodes.workspacePanel && nodes.workspacePanel.classList.contains("hidden");
      setWorkspaceOpen(Boolean(isHidden));
    });
  }

  if (nodes.terminalNewBtn) {
    nodes.terminalNewBtn.addEventListener("click", () => {
      void createTerminalSession().catch((error) => {
        appendTerminalOutput(
          state.terminalActiveId || "terminal",
          `\n[create terminal failed: ${String(error)}]\n`,
        );
      });
    });
  }

  if (nodes.terminalCollapseBtn) {
    nodes.terminalCollapseBtn.addEventListener("click", () => setTerminalOpen(false));
  }

  if (nodes.terminalInputForm) {
    nodes.terminalInputForm.addEventListener("submit", (event) => {
      void handleTerminalInputSubmit(event);
    });
  }

  if (nodes.terminalTabList) {
    nodes.terminalTabList.addEventListener("click", handleTerminalTabClick);
  }
}
