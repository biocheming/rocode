// ── Sidebar & Session List ──────────────────────────────────────────────────

function updateSessionControls() {
  const current = currentSession();
  const disabled = !current || interactionLocked();
  const canCancel = canAbortCurrentExecution();

  nodes.composerInput.disabled = Boolean(state.busyAction);
  nodes.sendButton.disabled = interactionLocked();
  nodes.cancelRunBtn.disabled = !canCancel;
  nodes.cancelRunBtn.classList.toggle('hidden', !canCancel);

  nodes.refreshSession.disabled = interactionLocked();
  nodes.newSessionBtn.disabled = interactionLocked();

  if (nodes.forkSessionBtn) nodes.forkSessionBtn.disabled = disabled;
  if (nodes.compactSessionBtn) nodes.compactSessionBtn.disabled = disabled;
  if (nodes.renameSessionBtn) nodes.renameSessionBtn.disabled = disabled;
  if (nodes.shareSessionBtn) nodes.shareSessionBtn.disabled = disabled;
  if (nodes.deleteSessionBtn) nodes.deleteSessionBtn.disabled = disabled;

  updateRuntimeChrome();

  if (disabled) {
    if (nodes.shareSessionBtn) {
      nodes.shareSessionBtn.textContent = "Share";
    }
    updateComposerMeta();
    return;
  }

  if (nodes.shareSessionBtn) {
    nodes.shareSessionBtn.textContent = current.share_url ? "Unshare" : "Share";
  }
  updateComposerMeta();
}

function updateCommandActionControls() {
  const current = currentSession();
  const locked = interactionLocked();
  const disabled = !current || locked;

  nodes.modelSelect.disabled = locked;
  nodes.themeSelect.disabled = locked;
  nodes.agentSelect.disabled = locked;

  if (nodes.commandAbortBtn) {
    nodes.commandAbortBtn.disabled = !canAbortCurrentExecution();
    nodes.commandAbortBtn.classList.toggle('hidden', !canAbortCurrentExecution());
  }

  if (nodes.commandSessionNewBtn) nodes.commandSessionNewBtn.disabled = locked;
  if (nodes.commandSessionForkBtn) nodes.commandSessionForkBtn.disabled = disabled;
  if (nodes.commandSessionCompactBtn) nodes.commandSessionCompactBtn.disabled = disabled;
  if (nodes.commandSessionRenameBtn) nodes.commandSessionRenameBtn.disabled = disabled;
  if (nodes.commandSessionShareBtn) nodes.commandSessionShareBtn.disabled = disabled;
  if (nodes.commandSessionDeleteBtn) nodes.commandSessionDeleteBtn.disabled = disabled;

  if (nodes.commandSessionShareBtn) {
    nodes.commandSessionShareBtn.textContent = current && current.share_url ? "Unshare" : "Share";
  }

  updateRuntimeChrome();
}

function renderProjects() {
  nodes.projectTree.innerHTML = "";

  if (state.projects.length === 0) {
    const empty = document.createElement("div");
    empty.className = "session-list-empty";
    empty.textContent = "No sessions yet. Send your first prompt.";
    nodes.projectTree.appendChild(empty);
    return;
  }

  for (const project of state.projects) {
    const card = document.createElement("div");
    card.style.marginBottom = "var(--space-3)";

    const trigger = document.createElement("button");
    trigger.className = "session-item";
    if (project.key === state.selectedProject) trigger.classList.add("active");

    const title = document.createElement("span");
    title.className = "session-item-title";
    title.textContent = project.label;

    const meta = document.createElement("span");
    meta.className = "session-item-meta";
    meta.textContent = `${project.sessions.length} sessions`;

    trigger.appendChild(title);
    trigger.appendChild(meta);

    trigger.addEventListener("click", () => {
      state.selectedProject = project.key;
      if (!state.selectedSession && project.sessions.length > 0) {
        state.selectedSession = project.sessions[0].id;
      }
      renderProjects();
      void loadMessages();
      if (window.innerWidth <= 980) {
        nodes.shell.classList.remove("sidebar-open");
      }
    });

    card.appendChild(trigger);

    if (project.key === state.selectedProject) {
      for (const session of project.sessions) {
        const button = document.createElement("button");
        button.className = "session-item";
        button.style.marginLeft = "var(--space-3)";
        button.style.paddingLeft = "var(--space-3)";

        if (session.id === state.selectedSession) {
          button.classList.add("active");
        }

        const title = document.createElement("span");
        title.className = "session-item-title";
        title.textContent = short(session.title, 32);

        const meta = document.createElement("span");
        meta.className = "session-item-meta";
        meta.textContent = formatTime(session.updated);

        button.appendChild(title);
        button.appendChild(meta);

        button.addEventListener("click", () => {
          state.selectedSession = session.id;
          renderProjects();
          void loadMessages();
          renderCommandSessionList();
          if (window.innerWidth <= 980) {
            nodes.shell.classList.remove("sidebar-open");
          }
        });

        card.appendChild(button);
      }
    }

    nodes.projectTree.appendChild(card);
  }

  updateSessionControls();
  updateCommandActionControls();
}

function renderCommandSessionList() {
  const container = nodes.commandSessionList;
  if (!container) return;

  container.innerHTML = "";

  for (const project of state.projects) {
    for (const session of project.sessions) {
      const btn = document.createElement("button");
      btn.className = "btn btn-ghost";
      btn.style.justifyContent = "flex-start";
      btn.style.textAlign = "left";

      if (session.id === state.selectedSession) {
        btn.style.background = "var(--accent-primary-soft)";
        btn.style.color = "var(--accent-primary)";
      }

      const title = short(session.title, 40);
      const projectLabel = project.label;
      btn.textContent = `${title} · ${projectLabel}`;

      btn.addEventListener("click", () => {
        state.selectedProject = project.key;
        state.selectedSession = session.id;
        renderProjects();
        renderCommandSessionList();
        void loadMessages();
      });

      container.appendChild(btn);
    }
  }
}
