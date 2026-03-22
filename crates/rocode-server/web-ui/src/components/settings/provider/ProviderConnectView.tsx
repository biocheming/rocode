import { type Component, For, Show } from "solid-js";
import type {
  ConnectProtocolOption,
  KnownProviderEntry,
} from "~/api/types";
import styles from "../SettingsDrawer.module.css";

type ConnectMode = "known" | "custom";

interface ProviderConnectViewProps {
  loading: boolean;
  hasError: boolean;
  connectMode: ConnectMode;
  knownProviderId: string;
  customProviderId: string;
  customBaseUrl: string;
  customProtocol: string;
  apiKey: string;
  connectSubmitting: boolean;
  knownProviders: KnownProviderEntry[];
  protocols: ConnectProtocolOption[];
  selectedKnownProvider: KnownProviderEntry | null;
  selectedCustomProtocol: ConnectProtocolOption | null;
  onConnectModeChange: (value: ConnectMode) => void;
  onKnownProviderIdChange: (value: string) => void;
  onCustomProviderIdChange: (value: string) => void;
  onCustomBaseUrlChange: (value: string) => void;
  onCustomProtocolChange: (value: string) => void;
  onApiKeyChange: (value: string) => void;
  onSubmitConnect: () => void;
  onRefresh: () => void;
}

export const ProviderConnectView: Component<ProviderConnectViewProps> = (props) => {
  return (
    <div class={styles.section}>
      <div class={styles.sectionTitle}>Connect Provider</div>
      <Show
        when={!props.loading}
        fallback={<div class={styles.empty}>Loading provider schema…</div>}
      >
        <Show
          when={!props.hasError}
          fallback={<div class={styles.empty}>Failed to load connect schema</div>}
        >
          <div class={styles.connectLayout}>
            <div class={styles.connectStepCard}>
              <div class={styles.connectStepHeader}>
                <span class={styles.stepBadge}>1</span>
                <div>
                  <div class={styles.itemName}>Choose Provider Source</div>
                  <div class={styles.itemMeta}>
                    Start from a known provider or enter a custom endpoint.
                  </div>
                </div>
              </div>
              <div class={styles.segmentedControl}>
                <button
                  class={styles.segmentedButton}
                  classList={{ [styles.active]: props.connectMode === "known" }}
                  onClick={() => props.onConnectModeChange("known")}
                >
                  Known
                </button>
                <button
                  class={styles.segmentedButton}
                  classList={{ [styles.active]: props.connectMode === "custom" }}
                  onClick={() => props.onConnectModeChange("custom")}
                >
                  Custom
                </button>
              </div>
              <Show
                when={props.connectMode === "known"}
                fallback={
                  <>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Provider ID</label>
                      <input
                        class={styles.fieldInput}
                        type="text"
                        value={props.customProviderId}
                        onInput={(e) => props.onCustomProviderIdChange(e.currentTarget.value)}
                        placeholder="provider-id"
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Base URL</label>
                      <input
                        class={styles.fieldInput}
                        type="url"
                        value={props.customBaseUrl}
                        onInput={(e) => props.onCustomBaseUrlChange(e.currentTarget.value)}
                        placeholder="https://api.example.com/v1"
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Protocol</label>
                      <select
                        class={styles.fieldSelect}
                        value={props.customProtocol}
                        onChange={(e) => props.onCustomProtocolChange(e.currentTarget.value)}
                      >
                        <For each={props.protocols}>
                          {(protocol) => (
                            <option value={protocol.id}>{protocol.name}</option>
                          )}
                        </For>
                      </select>
                    </div>
                    <div class={styles.metaCard}>
                      <div class={styles.metaRow}>
                        <span class={styles.itemName}>
                          {props.customProviderId.trim() || "Custom provider"}
                        </span>
                        <span class={styles.itemMeta}>
                          {props.selectedCustomProtocol?.name || "Select protocol"}
                        </span>
                      </div>
                      <div class={styles.metaText}>
                        {props.customBaseUrl.trim()
                          || "Enter a base URL to create a custom provider connection."}
                      </div>
                    </div>
                  </>
                }
              >
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Known Provider</label>
                  <select
                    class={styles.fieldSelect}
                    value={props.knownProviderId}
                    onChange={(e) => props.onKnownProviderIdChange(e.currentTarget.value)}
                  >
                    <For each={props.knownProviders}>
                      {(provider) => (
                        <option value={provider.id}>
                          {provider.name} ({provider.id})
                        </option>
                      )}
                    </For>
                  </select>
                </div>
                <Show when={props.selectedKnownProvider}>
                  {(provider) => (
                    <div class={styles.metaCard}>
                      <div class={styles.metaRow}>
                        <span class={styles.itemName}>{provider().name}</span>
                        <span class={styles.itemMeta}>
                          {provider().connected ? "Connected" : "Not connected"}
                        </span>
                      </div>
                      <div class={styles.metaText}>
                        {provider().model_count} models
                        <Show when={provider().env.length > 0}>
                          {` · ${provider().env.join(", ")}`}
                        </Show>
                      </div>
                    </div>
                  )}
                </Show>
              </Show>
            </div>
            <div class={styles.connectStepCard}>
              <div class={styles.connectStepHeader}>
                <span class={styles.stepBadge}>2</span>
                <div>
                  <div class={styles.itemName}>Add Credential</div>
                  <div class={styles.itemMeta}>
                    Store the API key and let the server connect this provider.
                  </div>
                </div>
              </div>
              <div class={styles.metaCard}>
                <div class={styles.metaRow}>
                  <span class={styles.itemName}>Target</span>
                  <span class={styles.itemMeta}>
                    {props.connectMode === "known" ? "Known provider" : "Custom endpoint"}
                  </span>
                </div>
                <div class={styles.metaText}>
                  {props.connectMode === "known"
                    ? `${props.selectedKnownProvider?.name || "Select a provider"} · ${props.selectedKnownProvider?.id || "No provider selected"}`
                    : `${props.customProviderId.trim() || "provider-id"} · ${props.selectedCustomProtocol?.name || "Protocol not selected"}`}
                </div>
              </div>
              <div class={styles.field}>
                <label class={styles.fieldLabel}>API Key</label>
                <input
                  class={styles.fieldInput}
                  type="password"
                  value={props.apiKey}
                  onInput={(e) => props.onApiKeyChange(e.currentTarget.value)}
                  placeholder="Paste API key"
                />
                <div class={styles.fieldHint}>
                  The credential is stored by the server and used only for this provider connection.
                </div>
              </div>
              <div class={styles.btnRow}>
                <button
                  class={styles.btnPrimary}
                  disabled={props.connectSubmitting}
                  onClick={() => props.onSubmitConnect()}
                >
                  {props.connectSubmitting ? "Connecting…" : "Connect"}
                </button>
                <button class={styles.btnSecondary} onClick={() => props.onRefresh()}>
                  Refresh
                </button>
              </div>
            </div>
          </div>
        </Show>
      </Show>
    </div>
  );
};
