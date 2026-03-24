import { type Component, createSignal, For, Show } from "solid-js";
import { state, interactionLocked, abortCurrentExecution, selectedModeLabel } from "~/stores/app";
import { compactPath } from "~/utils/format";
import type { PromptPart } from "~/api/types";
import styles from "./Composer.module.css";

export interface ComposerProps {
  onSend: (content: string, parts?: PromptPart[]) => void;
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("Failed to read file"));
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.readAsDataURL(file);
  });
}

export const Composer: Component<ComposerProps> = (props) => {
  const [input, setInput] = createSignal("");
  const [attachments, setAttachments] = createSignal<PromptPart[]>([]);
  let textareaRef: HTMLTextAreaElement | undefined;
  let fileInputRef: HTMLInputElement | undefined;

  const autoSize = () => {
    if (!textareaRef) return;
    textareaRef.style.height = "auto";
    textareaRef.style.height = `${Math.min(textareaRef.scrollHeight, 140)}px`;
  };

  const handleSubmit = (e: Event) => {
    e.preventDefault();
    const content = input().trim();
    const promptParts = attachments();
    if ((!content && promptParts.length === 0) || interactionLocked()) return;
    props.onSend(content, promptParts);
    setInput("");
    setAttachments([]);
    if (fileInputRef) {
      fileInputRef.value = "";
    }
    if (textareaRef) {
      textareaRef.style.height = "auto";
    }
  };

  const handleFileChange = async (event: Event) => {
    const files = Array.from(event.currentTarget instanceof HTMLInputElement ? event.currentTarget.files ?? [] : []);
    if (files.length === 0) return;
    const nextParts = await Promise.all(
      files.map(async (file) => ({
        type: "file" as const,
        url: await readFileAsDataUrl(file),
        filename: file.name,
        mime: file.type || undefined,
      })),
    );
    setAttachments((current) => [...current, ...nextParts]);
    if (fileInputRef) {
      fileInputRef.value = "";
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
      <Show when={attachments().length > 0}>
        <div class={styles.attachmentList}>
          <For each={attachments()}>
            {(attachment, index) => (
              <button
                type="button"
                class={styles.attachmentPill}
                onClick={() => {
                  setAttachments((current) => current.filter((_, itemIndex) => itemIndex !== index()));
                }}
                title="Remove attachment"
              >
                {attachment.type === "file" ? compactPath(attachment.filename || "attachment", 28) : attachment.type}
                <span class={styles.attachmentRemove}>×</span>
              </button>
            )}
          </For>
        </div>
      </Show>
      <form class={styles.composer} onSubmit={handleSubmit}>
        <input
          ref={fileInputRef}
          class={styles.fileInput}
          type="file"
          multiple
          onChange={(event) => {
            void handleFileChange(event);
          }}
        />
        <button
          type="button"
          class={styles.attachBtn}
          disabled={interactionLocked()}
          onClick={() => fileInputRef?.click()}
          title="Attach files"
        >
          +
        </button>
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
              disabled={interactionLocked() || (!input().trim() && attachments().length === 0)}
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
