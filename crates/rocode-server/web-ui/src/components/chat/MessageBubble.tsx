import { type Component, Show, createMemo, createSignal } from "solid-js";
import { renderMarkdownToHtml } from "~/utils/markdown";
import type { FeedMessage } from "./MessageFeed";
import styles from "./MessageBubble.module.css";

export interface MessageBubbleProps {
  message: FeedMessage;
}

export const MessageBubble: Component<MessageBubbleProps> = (props) => {
  const [reasoningOpen, setReasoningOpen] = createSignal(false);

  const isStatus = () => props.message.kind === "status";
  const isUser = () => props.message.role === "user";
  const isReasoning = () => props.message.kind === "reasoning";
  const isAssistant = () =>
    props.message.role === "assistant" || props.message.kind === "message";

  const bubbleClass = createMemo(() => {
    if (isStatus()) {
      const tone = props.message.tone ?? "normal";
      return `${styles.bubble} ${styles.status} ${styles[tone] ?? ""}`;
    }
    if (isReasoning()) return `${styles.bubble} ${styles.reasoning}`;
    if (isUser()) return `${styles.bubble} ${styles.user}`;
    if (isAssistant()) return `${styles.bubble} ${styles.assistant}`;
    return `${styles.bubble} ${styles.system}`;
  });

  const renderedHtml = createMemo(() => {
    if (isStatus()) return props.message.text;
    if (isReasoning()) return props.message.text; // rendered as pre-wrap, not markdown
    return renderMarkdownToHtml(props.message.text);
  });

  return (
    <article class={bubbleClass()}>
      <Show when={isReasoning()}>
        <div
          class={styles.reasoningToggle}
          onClick={() => setReasoningOpen((v) => !v)}
        >
          <span>{reasoningOpen() ? "▾" : "▸"}</span>
          <span>Thinking{props.message.text ? ` (${props.message.text.length} chars)` : ""}</span>
        </div>
        <Show when={reasoningOpen()}>
          <div class={styles.reasoningBody}>{props.message.text}</div>
        </Show>
      </Show>
      <Show when={!isReasoning()}>
        <Show when={props.message.title && !isStatus()}>
          <div class={styles.title}>{props.message.title}</div>
        </Show>
        <div class={styles.content} innerHTML={renderedHtml()} />
      </Show>
    </article>
  );
};
