import { type Component, Show, createSignal } from "solid-js";
import { api } from "~/api/client";
import type { PermissionInteraction } from "~/api/types";
import styles from "./PermissionPanel.module.css";

export interface PermissionPanelProps {
  interaction: PermissionInteraction;
  onClose: () => void;
}

export const PermissionPanel: Component<PermissionPanelProps> = (props) => {
  const [submitting, setSubmitting] = createSignal(false);

  const reply = async (action: "once" | "always" | "reject") => {
    setSubmitting(true);
    try {
      await api(`/permission/${props.interaction.permission_id}/reply`, {
        method: "POST",
        body: JSON.stringify({ reply: action }),
      });
      props.onClose();
    } catch (error) {
      console.error("Permission reply failed:", error);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div class={styles.overlay} onClick={(e) => e.target === e.currentTarget && props.onClose()}>
      <div class={styles.panel}>
        <div class={styles.header}>
          <span class={styles.title}>Permission Request</span>
          <button class={styles.closeBtn} onClick={props.onClose}>✕</button>
        </div>
        <div class={styles.body}>
          <Show when={props.interaction.message}>
            <div class={styles.message}>{props.interaction.message}</div>
          </Show>
          <div class={styles.detail}>
            <Show when={props.interaction.permission}>
              <div class={styles.detailRow}>
                <span class={styles.detailLabel}>Permission</span>
                <span class={styles.detailValue}>{props.interaction.permission}</span>
              </div>
            </Show>
            <Show when={props.interaction.command}>
              <div class={styles.detailRow}>
                <span class={styles.detailLabel}>Command</span>
                <span class={styles.detailValue}>{props.interaction.command}</span>
              </div>
            </Show>
            <Show when={props.interaction.filepath}>
              <div class={styles.detailRow}>
                <span class={styles.detailLabel}>File</span>
                <span class={styles.detailValue}>{props.interaction.filepath}</span>
              </div>
            </Show>
          </div>
        </div>
        <div class={styles.footer}>
          <button
            class={styles.btnReject}
            onClick={() => reply("reject")}
            disabled={submitting()}
          >
            Reject
          </button>
          <button
            class={styles.btnAlways}
            onClick={() => reply("always")}
            disabled={submitting()}
          >
            Allow Always
          </button>
          <button
            class={styles.btnAllow}
            onClick={() => reply("once")}
            disabled={submitting()}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
};
