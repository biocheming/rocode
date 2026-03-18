import { type Component, Show, Switch, Match, onMount, createSignal } from "solid-js";
import {
  state,
  createAndSelectSession,
  refreshSessionsIndex,
  loadProviders,
  loadModes,
  loadUiCommands,
  loadWebUiPreferences,
  sendPrompt,
  setTheme,
  selectSession,
  setActiveDomain,
  handleSSEEvent,
} from "~/stores/app";
import { loadTerminalSessions } from "~/stores/terminal";
import { NavRail } from "~/components/nav/NavRail";
import { ContextHeader } from "~/components/header/ContextHeader";
import { DomainTabs, type DomainId } from "~/components/header/DomainTabs";
import { ChatDomain } from "~/components/chat/ChatDomain";
import { SchedulerDomain } from "~/components/scheduler/SchedulerDomain";
import { TerminalDomain } from "~/components/terminal/TerminalDomain";
import { SettingsDrawer } from "~/components/settings/SettingsDrawer";
import { CommandPalette } from "~/components/command/CommandPalette";
import { QuestionPanel } from "~/components/interaction/QuestionPanel";
import { PermissionPanel } from "~/components/interaction/PermissionPanel";
import { useSSE } from "~/hooks/useSSE";
import { timeGreeting } from "~/utils/format";
import styles from "./App.module.css";

const App: Component = () => {
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [commandOpen, setCommandOpen] = createSignal(false);

  // Global SSE event stream
  const sse = useSSE({
    url: "/event",
    onEvent: handleSSEEvent,
  });

  onMount(async () => {
    setTheme(state.selectedTheme, { persist: false });

    await Promise.all([
      loadProviders().catch(() => {}),
      loadModes().catch(() => {}),
      refreshSessionsIndex().catch(() => {}),
      loadUiCommands().catch(() => {}),
    ]);

    await loadWebUiPreferences().catch(() => {});
    void loadTerminalSessions().catch(() => {});

    // Start global event stream
    sse.start();

    // Global keyboard shortcut: Ctrl+K for command palette
    document.addEventListener("keydown", (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setCommandOpen((open) => !open);
      }
    });
  });

  const handleSend = async (content: string) => {
    await sendPrompt(content);
  };

  const handleCommand = (command: string) => {
    // Slash command execution — will be fully wired
    console.log("Execute command:", command);
  };

  return (
    <div class={styles.app}>
      <NavRail
        onNewSession={() => createAndSelectSession()}
        onSelectSession={(id) => selectSession(id)}
        onOpenSettings={() => setSettingsOpen(true)}
      />
      <main class={styles.mainStage}>
        <Show
          when={state.selectedSession}
          fallback={
            <section class={styles.emptyState}>
              <div class={styles.emptyContent}>
                <h1>{timeGreeting()}</h1>
                <p>Select a session or create a new one to get started.</p>
              </div>
            </section>
          }
        >
          <ContextHeader />
          <DomainTabs active={state.activeDomain} onSelect={(id) => setActiveDomain(id)} />
          <div class={styles.domainContent}>
            <Switch>
              <Match when={state.activeDomain === "chat"}>
                <ChatDomain onSend={handleSend} />
              </Match>
              <Match when={state.activeDomain === "scheduler"}>
                <SchedulerDomain />
              </Match>
              <Match when={state.activeDomain === "terminal"}>
                <TerminalDomain />
              </Match>
              <Match when={state.activeDomain === "workspace"}>
                <div class={styles.placeholder}>Workspace</div>
              </Match>
            </Switch>
          </div>
        </Show>
      </main>

      {/* Overlays */}
      <Show when={settingsOpen()}>
        <SettingsDrawer onClose={() => setSettingsOpen(false)} />
      </Show>

      <Show when={commandOpen()}>
        <CommandPalette
          onClose={() => setCommandOpen(false)}
          onExecute={handleCommand}
        />
      </Show>

      <Show when={state.activeQuestionInteraction}>
        {(interaction) => (
          <QuestionPanel
            interaction={interaction()}
            onClose={() => {}}
          />
        )}
      </Show>

      <Show when={state.activePermissionInteraction}>
        {(interaction) => (
          <PermissionPanel
            interaction={interaction()}
            onClose={() => {}}
          />
        )}
      </Show>
    </div>
  );
};

export default App;
