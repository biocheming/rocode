import { type Component, For, Show } from "solid-js";
import type { ManagedProvider } from "~/api/types";
import styles from "../SettingsDrawer.module.css";
import {
  authTypeLabel,
  protocolLabel,
  statusLabel,
  type ManagedProviderGroup,
} from "./shared";

interface ProviderManagedListViewProps {
  loading: boolean;
  managedList: ManagedProvider[];
  managedProviderCounts: {
    all: number;
    connected: number;
    needsAuth: number;
    configured: number;
  };
  providerFilterQuery: string;
  providerStatusFilter: string;
  filteredManagedListCount: number;
  groupedManagedProviders: ManagedProviderGroup[];
  selectedProviderId: string | null;
  onProviderFilterQueryChange: (value: string) => void;
  onProviderStatusFilterChange: (value: string) => void;
  onSelectProvider: (providerId: string) => void;
}

export const ProviderManagedListView: Component<ProviderManagedListViewProps> = (
  props,
) => {
  return (
    <div class={styles.section}>
      <div class={styles.sectionTitle}>Managed Providers</div>
      <Show
        when={!props.loading}
        fallback={<div class={styles.empty}>Loading managed providers…</div>}
      >
        <Show
          when={props.managedList.length > 0}
          fallback={<div class={styles.empty}>No providers connected or configured</div>}
        >
          <div class={styles.summaryGrid}>
            <div class={styles.summaryCard}>
              <span class={styles.summaryLabel}>All</span>
              <span class={styles.summaryValue}>{props.managedProviderCounts.all}</span>
              <span class={styles.summaryMeta}>Connected or configured providers</span>
            </div>
            <div class={styles.summaryCard}>
              <span class={styles.summaryLabel}>Connected</span>
              <span class={styles.summaryValue}>
                {props.managedProviderCounts.connected}
              </span>
              <span class={styles.summaryMeta}>Ready to use right now</span>
            </div>
            <div class={styles.summaryCard}>
              <span class={styles.summaryLabel}>Needs Auth</span>
              <span class={styles.summaryValue}>
                {props.managedProviderCounts.needsAuth}
              </span>
              <span class={styles.summaryMeta}>
                Configured but missing valid credential
              </span>
            </div>
            <div class={styles.summaryCard}>
              <span class={styles.summaryLabel}>Saved</span>
              <span class={styles.summaryValue}>
                {props.managedProviderCounts.configured}
              </span>
              <span class={styles.summaryMeta}>
                Configured or credentialed, not currently connected
              </span>
            </div>
          </div>
          <div class={styles.filterBar}>
            <input
              class={styles.fieldInput}
              type="text"
              value={props.providerFilterQuery}
              onInput={(e) => props.onProviderFilterQueryChange(e.currentTarget.value)}
              placeholder="Search by id, name, protocol or URL"
            />
            <select
              class={styles.fieldSelect}
              value={props.providerStatusFilter}
              onChange={(e) => props.onProviderStatusFilterChange(e.currentTarget.value)}
            >
              <option value="all">All statuses</option>
              <option value="connected">Connected</option>
              <option value="needs-auth">Needs auth</option>
              <option value="configured">Configured / saved</option>
              <option value="known">Known providers</option>
              <option value="custom">Custom providers</option>
            </select>
          </div>
          <Show
            when={props.filteredManagedListCount > 0}
            fallback={<div class={styles.empty}>No providers match the current filters</div>}
          >
            <div class={styles.groupList}>
              <For each={props.groupedManagedProviders}>
                {(group) => (
                  <div class={styles.providerGroup}>
                    <div class={styles.groupHeader}>
                      <span class={styles.groupTitle}>{group.label}</span>
                      <span class={styles.groupMeta}>{group.providers.length}</span>
                    </div>
                    <div class={styles.itemList}>
                      <For each={group.providers}>
                        {(provider) => (
                          <button
                            class={styles.item}
                            classList={{
                              [styles.active]: props.selectedProviderId === provider.id,
                            }}
                            onClick={() => props.onSelectProvider(provider.id)}
                          >
                            <div class={styles.itemBody}>
                              <span class={styles.itemName}>
                                {provider.name || provider.id}
                              </span>
                              <span class={styles.itemMeta}>
                                {provider.id}
                                <Show when={provider.auth_type}>
                                  {` · ${authTypeLabel(provider.auth_type)}`}
                                </Show>
                                <Show when={provider.models.length > 0}>
                                  {` · ${provider.models.length} models`}
                                </Show>
                                <Show when={provider.protocol}>
                                  {` · ${protocolLabel(provider.protocol)}`}
                                </Show>
                              </span>
                            </div>
                            <span
                              class={styles.statusBadge}
                              classList={{
                                [styles.statusConnected]: provider.status === "connected",
                                [styles.statusWarn]: provider.status === "needs-auth",
                                [styles.statusNeutral]:
                                  provider.status !== "connected"
                                  && provider.status !== "needs-auth",
                              }}
                            >
                              {statusLabel(provider.status)}
                            </span>
                          </button>
                        )}
                      </For>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </Show>
      </Show>
    </div>
  );
};
