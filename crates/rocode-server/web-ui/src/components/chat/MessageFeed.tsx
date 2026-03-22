import { type Component, For, Show, createSignal, onCleanup } from "solid-js";
import { createStore } from "solid-js/store";
import type { OutputBlock } from "~/api/types";
import { state } from "~/stores/app";
import { MessageBubble } from "./MessageBubble";
import styles from "./MessageFeed.module.css";

// ── Feed state (local to chat domain) ─────────────────────────────────────

export interface FeedMessage {
  id: string;
  kind: string;
  role?: string;
  title?: string;
  text: string;
  tone?: string;
  phase?: string;
  ts?: number;
}

const [messages, setMessages] = createStore<FeedMessage[]>([]);

let nextId = 0;

export function clearFeed() {
  setMessages([]);
  nextId = 0;
}

export function appendFeedMessage(block: OutputBlock): string {
  const id = `msg-${nextId++}`;
  const msg: FeedMessage = {
    id,
    kind: block.kind,
    role: block.role,
    title: block.title,
    text: block.text ?? "",
    tone: block.tone,
    phase: block.phase,
    ts: block.ts as number | undefined,
  };
  setMessages((prev) => [...prev, msg]);
  return id;
}

export function updateFeedMessage(id: string, text: string) {
  setMessages(
    (msg) => msg.id === id,
    "text",
    (prev) => prev + text,
  );
}

// Insert a message before an existing message with given id
function insertBeforeMessageId(id: string, msg: FeedMessage) {
  setMessages((prev) => {
    const idx = prev.findIndex((m) => m.id === id);
    if (idx === -1) return [...prev, msg];
    return [...prev.slice(0, idx), msg, ...prev.slice(idx)];
  });
}

export function applyOutputBlockToFeed(block: OutputBlock) {
  if (block.kind === "reasoning" && !state.showThinking) {
    return;
  }

  if (block.kind === "status") {
    if (!block.silent) {
      appendFeedMessage(block);
    }
    return;
  }

  if (block.kind === "reasoning") {
    if (block.phase === "start") {
      // Insert reasoning BEFORE the assistant message it belongs to.
      // This fixes ordering when message delta arrives before reasoning start.
      const lastAssistant = messages.findLast(
        (m) => m.kind === "message" && m.role === "assistant",
      );
      if (lastAssistant) {
        const id = `msg-${nextId++}`;
        const msg: FeedMessage = {
          id,
          kind: block.kind,
          role: block.role,
          title: block.title,
          text: block.text ?? "",
          tone: block.tone,
          phase: block.phase,
          ts: block.ts as number | undefined,
        };
        insertBeforeMessageId(lastAssistant.id, msg);
        return;
      }
      // No assistant message yet, fall through to append
    } else if (block.phase === "delta") {
      const last = messages.findLast((m) => m.kind === "reasoning");
      if (last) {
        updateFeedMessage(last.id, block.text ?? "");
      }
      return;
    } else if (block.phase === "full") {
      appendFeedMessage(block);
      return;
    }
    appendFeedMessage(block);
    return;
  }

  if (block.kind === "message") {
    if (block.phase === "start") {
      appendFeedMessage(block);
    } else if (block.phase === "delta") {
      // Find the last message with matching role and update it
      const last = messages.findLast(
        (m) => m.kind === "message" && m.role === block.role,
      );
      if (last) {
        updateFeedMessage(last.id, block.text ?? "");
      }
    } else if (block.phase === "full") {
      appendFeedMessage(block);
    }
    return;
  }

  // For tool, session_event, etc. — just append
  appendFeedMessage(block);
}

// ── Component ──────────────────────────────────────────────────────────────

export const MessageFeed: Component = () => {
  let feedRef: HTMLDivElement | undefined;

  // Auto-scroll to bottom when new messages arrive
  const scrollToBottom = () => {
    if (feedRef) {
      feedRef.scrollTop = feedRef.scrollHeight;
    }
  };

  // Use MutationObserver for auto-scroll
  let observer: MutationObserver | undefined;
  const setupObserver = (el: HTMLDivElement) => {
    feedRef = el;
    observer = new MutationObserver(scrollToBottom);
    observer.observe(el, { childList: true, subtree: true });
  };

  onCleanup(() => observer?.disconnect());

  return (
    <div class={styles.feed} ref={setupObserver}>
      <Show
        when={messages.length > 0}
        fallback={
          <div class={styles.empty}>
            Send a message to start the conversation.
          </div>
        }
      >
        <For each={messages}>
          {(msg) => <MessageBubble message={msg} />}
        </For>
      </Show>
    </div>
  );
};
