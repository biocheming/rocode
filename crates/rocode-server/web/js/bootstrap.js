// ── Event Wiring & Bootstrap ───────────────────────────────────────────────

function autoSizeInput() {
  nodes.composerInput.style.height = "auto";
  nodes.composerInput.style.height = `${Math.min(nodes.composerInput.scrollHeight, 140)}px`;
}

function wireEvents() {
  nodes.sidebarToggle.addEventListener("click", () => {
    nodes.shell.classList.toggle("sidebar-open");
  });

  nodes.projectSearch.addEventListener("input", () => {
    buildProjects();
    renderProjects();
  });

  nodes.refreshSession.addEventListener("click", () => {
    void loadMessages();
  });

  nodes.newSessionBtn.addEventListener("click", () => {
    void runUiAction("creating session", async () => {
      await createAndSelectSession();
    });
  });

  nodes.forkSessionBtn.addEventListener("click", () => {
    void runUiAction("forking session", async () => {
      await forkCurrentSession();
    });
  });

  nodes.compactSessionBtn.addEventListener("click", () => {
    void runUiAction("compacting session", async () => {
      await compactCurrentSession();
    });
  });

  nodes.renameSessionBtn.addEventListener("click", () => {
    void runUiAction("renaming session", async () => {
      await renameCurrentSession();
    });
  });

  nodes.shareSessionBtn.addEventListener("click", () => {
    void runUiAction("sharing session", async () => {
      await toggleShareCurrentSession();
    });
  });

  nodes.deleteSessionBtn.addEventListener("click", () => {
    void runUiAction("deleting session", async () => {
      await deleteCurrentSession();
    });
  });

  nodes.cancelRunBtn.addEventListener("click", () => {
    void abortCurrentExecution();
  });

  nodes.commandAbortBtn.addEventListener("click", () => {
    void abortCurrentExecution();
  });

  nodes.commandBtn.addEventListener("click", () => {
    openCommandPanel("model");
  });

  nodes.commandClose.addEventListener("click", closeCommandPanel);
  nodes.commandPanel.addEventListener("click", (event) => {
    if (event.target === nodes.commandPanel) {
      closeCommandPanel();
    }
  });
  nodes.recoveryPanel.addEventListener("click", (event) => {
    void handleRecoveryPanelClick(event);
  });

  nodes.questionClose.addEventListener("click", closeQuestionPanel);
  nodes.questionPanel.addEventListener("click", (event) => {
    if (event.target === nodes.questionPanel) {
      closeQuestionPanel();
    }
  });
  nodes.questionRejectBtn.addEventListener("click", async () => {
    const interaction = state.activeQuestionInteraction;
    if (!interaction || state.questionSubmitting) return;
    state.questionSubmitting = true;
    renderQuestionPanel();
    try {
      await rejectQuestionInteraction(interaction);
      closeQuestionPanel();
      await loadMessages();
    } catch (error) {
      applyOutputBlock({
        kind: "status",
        tone: "error",
        text: `Failed to reject question: ${String(error)}`,
      });
      state.questionSubmitting = false;
      renderQuestionPanel();
    }
  });
  nodes.questionForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const interaction = state.activeQuestionInteraction;
    if (!interaction || state.questionSubmitting) return;
    state.questionSubmitting = true;
    renderQuestionPanel();
    try {
      const answers = collectQuestionPanelAnswers();
      await api(`/question/${interaction.request_id}/reply`, {
        method: "POST",
        body: JSON.stringify({ answers }),
      });
      closeQuestionPanel();
      await loadMessages();
    } catch (error) {
      applyOutputBlock({
        kind: "status",
        tone: "error",
        text: `Failed to answer question: ${String(error)}`,
      });
      state.questionSubmitting = false;
      renderQuestionPanel();
    }
  });

  nodes.modelSelect.addEventListener("change", () => {
    state.selectedModel = nodes.modelSelect.value;
    updateComposerMeta();
    updateSessionRuntimeMeta(currentSession());
    applyOutputBlock({ kind: "status", tone: "success", text: `Model set to ${state.selectedModel}`, silent: true });
  });

  nodes.agentSelect.addEventListener("change", () => {
    setSelectedMode(nodes.agentSelect.value || null);
    applyOutputBlock({ kind: "status", tone: "success", text: `Mode set to ${selectedModeLabel()}`, silent: true });
  });

  nodes.themeSelect.addEventListener("change", () => {
    applyTheme(nodes.themeSelect.value);
  });

  nodes.commandSessionNewBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("creating session", async () => {
      await createAndSelectSession();
    });
  });

  nodes.commandSessionForkBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("forking session", async () => {
      await forkCurrentSession();
    });
  });

  nodes.commandSessionCompactBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("compacting session", async () => {
      await compactCurrentSession();
    });
  });

  nodes.commandSessionRenameBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("renaming session", async () => {
      await renameCurrentSession();
    });
  });

  nodes.commandSessionShareBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("sharing session", async () => {
      await toggleShareCurrentSession();
    });
  });

  nodes.commandSessionDeleteBtn.addEventListener("click", () => {
    closeCommandPanel();
    void runUiAction("deleting session", async () => {
      await deleteCurrentSession();
    });
  });

  nodes.composerForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const content = nodes.composerInput.value.trim();
    if (!content) return;

    if (interactionLocked() && !content.startsWith("/")) {
      applyOutputBlock({
        kind: "status",
        tone: "warning",
        text: state.streaming
          ? "A response is running. Use /abort to cancel or wait until it finishes."
          : "Another action is running. Wait until it finishes.",
      });
      return;
    }

    if (content.startsWith("/")) {
      const handled = await handleSlashCommand(content);
      if (handled) {
        nodes.composerInput.value = "";
        autoSizeInput();
        return;
      }
    }

    nodes.composerInput.value = "";
    autoSizeInput();
    await sendPrompt(content);
  });

  nodes.composerInput.addEventListener("input", autoSizeInput);
  nodes.composerInput.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !nodes.questionPanel.classList.contains("hidden")) {
      closeQuestionPanel();
      return;
    }
    if (event.key === "Escape" && !nodes.commandPanel.classList.contains("hidden")) {
      closeCommandPanel();
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      nodes.composerForm.requestSubmit();
    }
  });

  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      openCommandPanel("model");
    }
    if (event.key === "Escape" && !nodes.questionPanel.classList.contains("hidden")) {
      closeQuestionPanel();
      return;
    }
    if (event.key === "Escape" && !nodes.commandPanel.classList.contains("hidden")) {
      closeCommandPanel();
    }
  });

  for (const chip of nodes.chipActions) {
    chip.addEventListener("click", () => {
      nodes.composerInput.value = chip.dataset.template || "";
      autoSizeInput();
      nodes.composerInput.focus();
    });
  }

  if (nodes.overflowToggle && nodes.overflowMenu) {
    nodes.overflowToggle.addEventListener("click", (event) => {
      event.stopPropagation();
      nodes.overflowMenu.classList.toggle("hidden");
    });
    document.addEventListener("click", () => {
      nodes.overflowMenu.classList.add("hidden");
    });
  }

  if (nodes.settingsToggleBtn) {
    nodes.settingsToggleBtn.addEventListener("click", () => {
      openCommandPanel("model");
    });
  }
}

async function bootstrap() {
  nodes.heroGreeting.textContent = timeGreeting();
  applyTheme(state.selectedTheme);
  updateTokenUsage();
  setBadge("loading", "warn");
  wireEvents();
  autoSizeInput();
  renderThemeOptions();
  updateComposerMeta();
  syncInteractionState();

  await Promise.all([loadProviders(), loadModes(), loadSessions()]);

  if (!state.streaming) {
    setBadge("ready", "ok");
  }
}

function installTestApi() {
  const target = globalThis.__ROCODE_WEB_TEST_API__;
  if (!target) return;
  Object.assign(target, {
    state,
    nodes,
    applyOutputBlock,
    applyStreamUsage,
    appendSchedulerStage,
    updateSchedulerStage,
    schedulerStageBlockFromMessage,
    loadMessages,
    openQuestionPanel,
    closeQuestionPanel,
    interactionFromLiveQuestionEvent,
    renderDecisionBlock,
    clearFeed,
  });
}

installTestApi();

if (!globalThis.__ROCODE_WEB_DISABLE_BOOTSTRAP__) {
  void bootstrap();
}
