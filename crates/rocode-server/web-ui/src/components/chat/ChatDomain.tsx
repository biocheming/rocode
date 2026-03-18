import { type Component, onMount, onCleanup, createEffect } from "solid-js";
import { state, setOutputBlockListener, loadSessionMessages } from "~/stores/app";
import { MessageFeed, clearFeed, appendFeedMessage, applyOutputBlockToFeed } from "./MessageFeed";
import { Composer } from "./Composer";
import styles from "./ChatDomain.module.css";

export interface ChatDomainProps {
  onSend: (content: string) => void;
}

export const ChatDomain: Component<ChatDomainProps> = (props) => {
  // Register output block listener so SSE events flow into the feed
  onMount(() => {
    setOutputBlockListener((block, _id) => {
      applyOutputBlockToFeed(block);
    });
  });

  onCleanup(() => {
    setOutputBlockListener(null);
  });

  // Load messages when selected session changes
  createEffect(() => {
    const sessionId = state.selectedSession;
    clearFeed();
    if (!sessionId) return;

    void loadSessionMessages(sessionId).then((blocks) => {
      for (const block of blocks) {
        if (block.text) {
          appendFeedMessage(block);
        }
      }
    }).catch(() => {});
  });

  return (
    <div class={styles.domain}>
      <MessageFeed />
      <div class={styles.composerWrap}>
        <Composer onSend={props.onSend} />
      </div>
    </div>
  );
};
