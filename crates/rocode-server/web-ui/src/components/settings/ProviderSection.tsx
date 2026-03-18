import { type Component, For, Show, createSignal } from "solid-js";
import { state } from "~/stores/app";
import styles from "./SettingsDrawer.module.css";

export const ProviderSection: Component = () => {
  const [selectedProvider, setSelectedProvider] = createSignal<string | null>(null);

  const providers = () => state.providers;

  return (
    <div>
      <div class={styles.section}>
        <div class={styles.sectionTitle}>Providers</div>
        <Show
          when={providers().length > 0}
          fallback={<div class={styles.empty}>No providers configured</div>}
        >
          <div class={styles.itemList}>
            <For each={providers()}>
              {(provider) => (
                <button
                  class={styles.item}
                  classList={{ [styles.active]: selectedProvider() === provider.id }}
                  onClick={() => setSelectedProvider(provider.id)}
                >
                  <span class={styles.itemName}>{provider.name || provider.id}</span>
                  <span class={styles.itemMeta}>
                    {(provider.models ?? []).length} models
                  </span>
                </button>
              )}
            </For>
          </div>
        </Show>
      </div>

      <Show when={selectedProvider()}>
        {(providerId) => {
          const provider = () => providers().find((p) => p.id === providerId());
          return (
            <div class={styles.section}>
              <div class={styles.sectionTitle}>{provider()?.name ?? providerId()}</div>
              <div class={styles.field}>
                <label class={styles.fieldLabel}>Provider ID</label>
                <input
                  class={styles.fieldInput}
                  type="text"
                  value={provider()?.id ?? ""}
                  readOnly
                />
              </div>
              <Show when={provider()?.base}>
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Base Provider</label>
                  <input
                    class={styles.fieldInput}
                    type="text"
                    value={String(provider()?.base ?? "")}
                    readOnly
                  />
                </div>
              </Show>
              <Show when={provider()?.base_url}>
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Base URL</label>
                  <input
                    class={styles.fieldInput}
                    type="text"
                    value={String(provider()?.base_url ?? "")}
                    readOnly
                  />
                </div>
              </Show>
              <Show when={(provider()?.models ?? []).length > 0}>
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Models</label>
                  <div class={styles.itemList}>
                    <For each={provider()?.models ?? []}>
                      {(model) => (
                        <div class={styles.item}>
                          <span class={styles.itemName}>{model.name || model.id}</span>
                          <span class={styles.itemMeta}>{model.family ?? ""}</span>
                        </div>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </div>
          );
        }}
      </Show>
    </div>
  );
};
