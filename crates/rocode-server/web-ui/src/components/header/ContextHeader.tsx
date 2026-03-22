import { type Component, Show, For, createMemo } from "solid-js";
import {
  state,
  currentSession,
  selectedModeLabel,
} from "~/stores/app";
import { short, compactPath } from "~/utils/format";
import styles from "./ContextHeader.module.css";

export const ContextHeader: Component = () => {
  const session = currentSession;

  const metaPills = createMemo(() => {
    const entries: { label: string; value: string }[] = [];
    entries.push({ label: "mode", value: selectedModeLabel() });
    entries.push({ label: "model", value: state.selectedModel || "auto" });
    entries.push({
      label: "directory",
      value: compactPath(session()?.directory),
    });
    return entries;
  });

  return (
    <div class={styles.header}>
      <span class={styles.title}>
        {session() ? short(session()!.title, 56) : "ROCode"}
      </span>
      <div class={styles.meta}>
        <For each={metaPills()}>
          {(pill) => (
            <span class={styles.pill}>
              <span class={styles.pillLabel}>{pill.label}</span>
              <span>{pill.value}</span>
            </span>
          )}
        </For>
        <Show when={state.modeLoadError}>
          <span class={`${styles.pill} ${styles.errorPill}`} title={state.modeLoadError ?? ""}>
            <span class={styles.pillLabel}>mode error</span>
            <span>{state.modeLoadError}</span>
          </span>
        </Show>
      </div>
    </div>
  );
};
