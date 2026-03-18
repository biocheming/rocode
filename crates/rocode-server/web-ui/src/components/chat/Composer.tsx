import { type Component, createSignal, Show } from "solid-js";
import { state, interactionLocked, abortCurrentExecution, selectedModeLabel } from "~/stores/app";
import { compactPath } from "~/utils/format";
import styles from "./Composer.module.css";

export interface ComposerProps {
  onSend: (content: string) => void;
}

export const Composer: Component<ComposerProps> = (props) => {
  const [input, setInput] = createSignal("");
  let textareaRef: HTMLTextAreaElement | undefined;

  const autoSize = () => {
    if (!textareaRef) return;
    textareaRef.style.height = "auto";
    textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 140)}px`;
  };

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    const content = input().trim();
    if (!content || interactionLocked()) return;
    props.onSend(content);
    setInput("");
    if (textareaRef) {
      textareaRef.style.height = "auto";
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  };

  return (
    <div>
      <form class={styles.composer} onSubmit={handleSubmit}>
        <div class={styles.inputWrap}>
          <textarea
            ref={textareaRef}
            class={styles.input}
            placeholder="Send a message..."
            value={input()}
            onInput={(e) => {
              setInput(e.currentTarget.value);
              autoSize();
            }}
            onKeyDown={handleKeyDown}
            disabled={interactionLocked()}
            rows={1}
          />
        </div>
        <Show
          when={state.streaming}
          fallback={
            <button
              type="submit"
              class={styles.sendBtn}
              disabled={interactionLocked() || !input().trim()}
              title="Send"
            >
              ↑
            </button>
          }
        >
          <button
            type="button"
            class={styles.cancelBtn}
            title="Cancel"
            onClick={() => {
              void abortCurrentExecution().catch(() => {});
            }}
          >
            ✕
          </button>
        </Show>
      </form>
      <div class={styles.meta}>
        <span class={styles.metaPill}>
          <span class={styles.metaLabel}>mode</span>
          {selectedModeLabel()}
        </span>
        <span class={styles.metaPill}>
          <span class={styles.metaLabel}>model</span>
          {state.selectedModel || "auto"}
        </span>
      </div>
    </div>
  );
};
