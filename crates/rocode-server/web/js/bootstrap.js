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

  if (nodes.heroNewSessionBtn) {
    nodes.heroNewSessionBtn.addEventListener("click", () => {
      void runUiAction("creating session", async () => {
        await createAndSelectSession();
      });
    });
  }

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
  if (nodes.settingsTabList) {
    nodes.settingsTabList.addEventListener("click", (event) => {
      const button =
        event.target && event.target.dataset && event.target.dataset.settingsTab ? event.target : null;
      if (!button) return;
      setSettingsTab(button.dataset.settingsTab || "general");
    });
  }
  if (nodes.settingsProviderReloadBtn) {
    nodes.settingsProviderReloadBtn.addEventListener("click", () => {
      void reloadProviderSettings().catch((error) => {
        setProviderStatus(`Reload failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsProviderNewBtn) {
    nodes.settingsProviderNewBtn.addEventListener("click", createBlankProvider);
  }
  if (nodes.settingsModelNewBtn) {
    nodes.settingsModelNewBtn.addEventListener("click", createBlankModel);
  }
  if (nodes.settingsModelRenameBtn) {
    nodes.settingsModelRenameBtn.addEventListener("click", () => {
      void renameSelectedModel().catch((error) => {
        setProviderStatus(`Rename failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsModelDeleteBtn) {
    nodes.settingsModelDeleteBtn.addEventListener("click", () => {
      void deleteSelectedModel().catch((error) => {
        setProviderStatus(`Delete failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsProviderSaveBtn) {
    nodes.settingsProviderSaveBtn.addEventListener("click", () => {
      void saveSelectedProvider().catch((error) => {
        setProviderStatus(`Save failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsProviderRenameBtn) {
    nodes.settingsProviderRenameBtn.addEventListener("click", () => {
      void renameSelectedProvider().catch((error) => {
        setProviderStatus(`Rename failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsProviderDeleteBtn) {
    nodes.settingsProviderDeleteBtn.addEventListener("click", () => {
      void deleteSelectedProvider().catch((error) => {
        setProviderStatus(`Delete failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsSchedulerReloadBtn) {
    nodes.settingsSchedulerReloadBtn.addEventListener("click", () => {
      void loadSettingsWorkspace({ force: true }).catch((error) => {
        setSchedulerStatus(`Reload failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsSchedulerTemplateBtn) {
    nodes.settingsSchedulerTemplateBtn.addEventListener("click", seedSchedulerTemplate);
  }
  if (nodes.settingsSchedulerSaveBtn) {
    nodes.settingsSchedulerSaveBtn.addEventListener("click", () => {
      void saveSchedulerSettings().catch((error) => {
        setSchedulerStatus(`Save failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpReloadBtn) {
    nodes.settingsMcpReloadBtn.addEventListener("click", () => {
      void reloadMcpSettings().catch((error) => {
        setMcpStatus(`Reload failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpNewBtn) {
    nodes.settingsMcpNewBtn.addEventListener("click", createBlankMcp);
  }
  if (nodes.settingsMcpSaveBtn) {
    nodes.settingsMcpSaveBtn.addEventListener("click", () => {
      void saveSelectedMcp().catch((error) => {
        setMcpStatus(`Save failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpConnectBtn) {
    nodes.settingsMcpConnectBtn.addEventListener("click", () => {
      void connectSelectedMcp().catch((error) => {
        setMcpStatus(`Connect failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpRestartBtn) {
    nodes.settingsMcpRestartBtn.addEventListener("click", () => {
      void restartSelectedMcp().catch((error) => {
        setMcpStatus(`Restart failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpRenameBtn) {
    nodes.settingsMcpRenameBtn.addEventListener("click", () => {
      void renameSelectedMcp().catch((error) => {
        setMcpStatus(`Rename failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsMcpDeleteBtn) {
    nodes.settingsMcpDeleteBtn.addEventListener("click", () => {
      void deleteSelectedMcp().catch((error) => {
        setMcpStatus(`Delete failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsPluginReloadBtn) {
    nodes.settingsPluginReloadBtn.addEventListener("click", () => {
      void reloadPluginSettings().catch((error) => {
        setPluginStatus(`Reload failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsPluginNewBtn) {
    nodes.settingsPluginNewBtn.addEventListener("click", createBlankPlugin);
  }
  if (nodes.settingsPluginSaveBtn) {
    nodes.settingsPluginSaveBtn.addEventListener("click", () => {
      void saveSelectedPlugin().catch((error) => {
        setPluginStatus(`Save failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsPluginRenameBtn) {
    nodes.settingsPluginRenameBtn.addEventListener("click", () => {
      void renameSelectedPlugin().catch((error) => {
        setPluginStatus(`Rename failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsPluginDeleteBtn) {
    nodes.settingsPluginDeleteBtn.addEventListener("click", () => {
      void deleteSelectedPlugin().catch((error) => {
        setPluginStatus(`Delete failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsLspReloadBtn) {
    nodes.settingsLspReloadBtn.addEventListener("click", () => {
      void reloadLspSettings().catch((error) => {
        setLspStatus(`Reload failed: ${String(error)}`, "error");
      });
    });
  }
  if (nodes.settingsLspSaveBtn) {
    nodes.settingsLspSaveBtn.addEventListener("click", () => {
      void saveLspSettings().catch((error) => {
        setLspStatus(`Save failed: ${String(error)}`, "error");
      });
    });
  }
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

  nodes.permissionClose.addEventListener("click", closePermissionPanel);
  nodes.permissionPanel.addEventListener("click", (event) => {
    if (event.target === nodes.permissionPanel) {
      closePermissionPanel();
    }
  });
  nodes.permissionRejectBtn.addEventListener("click", async () => {
    try {
      await submitPermissionInteractionReply("reject");
      closePermissionPanel();
      await loadMessages();
    } catch (error) {
      applyOutputBlock({
        kind: "status",
        tone: "error",
        text: `Failed to reject permission: ${String(error)}`,
      });
      renderPermissionPanel();
    }
  });
  nodes.permissionAllowBtn.addEventListener("click", async () => {
    try {
      await submitPermissionInteractionReply("once");
      closePermissionPanel();
      await loadMessages();
    } catch (error) {
      applyOutputBlock({
        kind: "status",
        tone: "error",
        text: `Failed to allow permission: ${String(error)}`,
      });
      renderPermissionPanel();
    }
  });
  nodes.permissionAlwaysBtn.addEventListener("click", async () => {
    try {
      await submitPermissionInteractionReply("always");
      closePermissionPanel();
      await loadMessages();
    } catch (error) {
      applyOutputBlock({
        kind: "status",
        tone: "error",
        text: `Failed to allow permission: ${String(error)}`,
      });
      renderPermissionPanel();
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
    if (event.key === "Escape" && !nodes.permissionPanel.classList.contains("hidden")) {
      closePermissionPanel();
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
    if (event.key === "Escape" && !nodes.permissionPanel.classList.contains("hidden")) {
      closePermissionPanel();
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
  applyTheme(state.selectedTheme, { persist: false, announce: false });
  updateTokenUsage();
  setBadge("loading", "warn");
  wireEvents();
  initTerminalPanel();
  autoSizeInput();
  renderThemeOptions();
  updateComposerMeta();
  syncInteractionState();

  await Promise.all([loadProviders(), loadModes(), loadSessions(), loadUiCommands()]);
  await loadWebUiPreferences();
  await loadTerminalSessions().catch(() => {});
  startGlobalServerEventStream();

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
    applyOutputBlockEvent,
    applyWebUiPreferences,
    loadWebUiPreferences,
    handleSlashCommand,
    openCommandPanel,
    loadSettingsWorkspace,
    createBlankProvider,
    renameSelectedProvider,
    loadUiCommands,
    refreshSessionsIndex,
    handleGlobalServerEvent,
    startGlobalServerEventStream,
  });
}

installTestApi();

if (!globalThis.__ROCODE_WEB_DISABLE_BOOTSTRAP__) {
  void bootstrap();
}
