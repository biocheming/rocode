import { type Component, For, Show } from "solid-js";
import type {
  ConnectProtocolOption,
  ManagedModelOverride,
  ManagedProvider,
  Model,
  ProviderAuthMethod,
} from "~/api/types";
import styles from "../SettingsDrawer.module.css";
import {
  authStateDescription,
  authTypeLabel,
  boolSummary,
  jsonFieldSummary,
  NEW_MODEL_OVERRIDE_KEY,
  oauthStepLabel,
  protocolLabel,
  statusLabel,
  type PendingOAuthFlow,
} from "./shared";

interface ProviderDetailViewProps {
  provider: ManagedProvider;
  connectProtocols: ConnectProtocolOption[];
  canEditConnection: boolean;
  editName: string;
  editBaseUrl: string;
  editProtocol: string;
  saveSubmitting: boolean;
  clearAuthSubmitting: boolean;
  deleteSubmitting: boolean;
  selectedAuthMethods: ProviderAuthMethod[];
  oauthSubmitting: boolean;
  pendingOAuth: PendingOAuthFlow | null;
  oauthCode: string;
  filteredSelectedModels: Model[];
  modelFilterQuery: string;
  selectedModelOverrideKey: string | null;
  selectedModelOverride: ManagedModelOverride | null;
  modelOverrideSubmitting: boolean;
  modelOverrideDeleting: boolean;
  modelOverrideKeyInput: string;
  modelOverrideModelInput: string;
  modelOverrideNameInput: string;
  modelOverrideBaseUrlInput: string;
  modelOverrideFamilyInput: string;
  modelOverrideReasoningInput: string;
  modelOverrideToolCallInput: string;
  modelOverrideStatusInput: string;
  modelOverrideReleaseDateInput: string;
  modelOverrideHeadersInput: string;
  modelOverrideOptionsInput: string;
  modelOverrideVariantsInput: string;
  modelOverrideModalitiesInput: string;
  modelOverrideInterleavedInput: string;
  modelOverrideCostInput: string;
  modelOverrideLimitInput: string;
  modelOverrideAttachmentInput: string;
  modelOverrideTemperatureInput: string;
  modelOverrideExperimentalInput: string;
  showAdvancedOverrideFields: boolean;
  onEditNameChange: (value: string) => void;
  onEditBaseUrlChange: (value: string) => void;
  onEditProtocolChange: (value: string) => void;
  onSaveSelectedProvider: () => void;
  onClearSelectedProviderAuth: () => void;
  onDeleteProvider: () => void;
  onStartOAuth: (methodIndex: number, method: ProviderAuthMethod) => void;
  onFinishOAuth: () => void;
  onOpenOAuthAgain: () => void;
  onCancelOAuth: () => void;
  onOAuthCodeChange: (value: string) => void;
  onModelFilterQueryChange: (value: string) => void;
  onSelectModelOverrideKey: (value: string | null) => void;
  onModelOverrideKeyInputChange: (value: string) => void;
  onModelOverrideModelInputChange: (value: string) => void;
  onModelOverrideNameInputChange: (value: string) => void;
  onModelOverrideBaseUrlInputChange: (value: string) => void;
  onModelOverrideFamilyInputChange: (value: string) => void;
  onModelOverrideReasoningInputChange: (value: string) => void;
  onModelOverrideToolCallInputChange: (value: string) => void;
  onModelOverrideStatusInputChange: (value: string) => void;
  onModelOverrideReleaseDateInputChange: (value: string) => void;
  onModelOverrideHeadersInputChange: (value: string) => void;
  onModelOverrideOptionsInputChange: (value: string) => void;
  onModelOverrideVariantsInputChange: (value: string) => void;
  onModelOverrideModalitiesInputChange: (value: string) => void;
  onModelOverrideInterleavedInputChange: (value: string) => void;
  onModelOverrideCostInputChange: (value: string) => void;
  onModelOverrideLimitInputChange: (value: string) => void;
  onModelOverrideAttachmentInputChange: (value: string) => void;
  onModelOverrideTemperatureInputChange: (value: string) => void;
  onModelOverrideExperimentalInputChange: (value: string) => void;
  onToggleAdvancedOverrideFields: () => void;
  onSaveModelOverride: () => void;
  onDeleteModelOverride: () => void;
}

export const ProviderDetailView: Component<ProviderDetailViewProps> = (props) => {
  return (
    <div class={styles.section}>
      <div class={styles.sectionTitle}>{props.provider.name}</div>
      <div class={styles.detailGrid}>
        <div class={styles.metaCard}>
          <div class={styles.metaRow}>
            <span class={styles.itemName}>Status</span>
            <span
              class={styles.statusBadge}
              classList={{
                [styles.statusConnected]: props.provider.status === "connected",
                [styles.statusWarn]: props.provider.status === "needs-auth",
                [styles.statusNeutral]:
                  props.provider.status !== "connected"
                  && props.provider.status !== "needs-auth",
              }}
            >
              {statusLabel(props.provider.status)}
            </span>
          </div>
          <div class={styles.metaText}>
            {props.provider.known ? "Known provider" : "Custom provider"}
            {` · Auth: ${authTypeLabel(props.provider.auth_type)}`}
            {props.provider.configured ? " · Configured" : " · No custom config"}
          </div>
        </div>
        <div class={styles.summaryGrid}>
          <div class={styles.summaryCard}>
            <span class={styles.summaryLabel}>Endpoint</span>
            <span class={styles.summaryValue}>
              {props.provider.base_url || "Built-in endpoint"}
            </span>
            <span class={styles.summaryMeta}>
              {props.canEditConnection
                ? "Editable from this panel"
                : "Managed by built-in provider defaults"}
            </span>
          </div>
          <div class={styles.summaryCard}>
            <span class={styles.summaryLabel}>Protocol</span>
            <span class={styles.summaryValue}>
              {protocolLabel(props.provider.protocol)}
            </span>
            <span class={styles.summaryMeta}>
              {props.provider.protocol || "No custom protocol override"}
            </span>
          </div>
          <div class={styles.summaryCard}>
            <span class={styles.summaryLabel}>Models</span>
            <span class={styles.summaryValue}>{props.provider.models.length}</span>
            <span class={styles.summaryMeta}>
              {props.provider.known_model_count > 0
                ? `${props.provider.known_model_count} known in registry`
                : "Runtime-discovered models only"}
            </span>
          </div>
          <div class={styles.summaryCard}>
            <span class={styles.summaryLabel}>Overrides</span>
            <span class={styles.summaryValue}>
              {props.provider.model_overrides.length}
            </span>
            <span class={styles.summaryMeta}>
              {props.provider.model_overrides.length > 0
                ? "Provider-local model aliases are configured"
                : "No model overrides yet"}
            </span>
          </div>
        </div>
      </div>
      <div class={styles.detailSections}>
        <div class={styles.sectionCard}>
          <div class={styles.sectionCardHeader}>
            <div>
              <div class={styles.itemName}>Connection</div>
              <div class={styles.itemMeta}>
                Provider identity and endpoint settings
              </div>
            </div>
          </div>
          <div class={styles.fieldGrid}>
            <div class={styles.field}>
              <label class={styles.fieldLabel}>Provider ID</label>
              <input
                class={styles.fieldInput}
                type="text"
                value={props.provider.id}
                readOnly
              />
            </div>
            <div class={styles.field}>
              <label class={styles.fieldLabel}>Display Name</label>
              <input
                class={styles.fieldInput}
                type="text"
                value={props.editName}
                onInput={(e) => props.onEditNameChange(e.currentTarget.value)}
              />
            </div>
            <Show when={props.canEditConnection}>
              <>
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Base URL</label>
                  <input
                    class={styles.fieldInput}
                    type="url"
                    value={props.editBaseUrl}
                    onInput={(e) => props.onEditBaseUrlChange(e.currentTarget.value)}
                  />
                </div>
                <div class={styles.field}>
                  <label class={styles.fieldLabel}>Protocol</label>
                  <select
                    class={styles.fieldSelect}
                    value={props.editProtocol}
                    onChange={(e) => props.onEditProtocolChange(e.currentTarget.value)}
                  >
                    <For each={props.connectProtocols}>
                      {(protocol) => (
                        <option value={protocol.id}>{protocol.name}</option>
                      )}
                    </For>
                  </select>
                </div>
              </>
            </Show>
          </div>
          <Show when={!props.canEditConnection}>
            <div class={styles.metaCard}>
              <div class={styles.metaText}>
                This provider uses the built-in endpoint. Update its API key from
                the connect panel above.
              </div>
            </div>
          </Show>
          <Show when={props.provider.env.length > 0}>
            <div class={styles.metaCard}>
              <div class={styles.metaText}>
                Env hints: {props.provider.env.join(", ")}
              </div>
            </div>
          </Show>
          <div class={styles.btnRow}>
            <button
              class={styles.btnPrimary}
              disabled={props.saveSubmitting}
              onClick={() => props.onSaveSelectedProvider()}
            >
              {props.saveSubmitting ? "Saving…" : "Save Connection"}
            </button>
          </div>
        </div>

        <div class={styles.sectionCard}>
          <div class={styles.sectionCardHeader}>
            <div>
              <div class={styles.itemName}>Authentication</div>
              <div class={styles.itemMeta}>
                Credential status, auth methods and OAuth flow
              </div>
            </div>
          </div>
          <div class={styles.metaCard}>
            <div class={styles.metaRow}>
              <span class={styles.itemName}>
                {authTypeLabel(props.provider.auth_type)}
              </span>
              <span
                class={styles.statusBadge}
                classList={{
                  [styles.statusConnected]: props.provider.status === "connected",
                  [styles.statusWarn]: props.provider.status === "needs-auth",
                  [styles.statusNeutral]:
                    props.provider.status !== "connected"
                    && props.provider.status !== "needs-auth",
                }}
              >
                {statusLabel(props.provider.status)}
              </span>
            </div>
            <div class={styles.metaText}>
              {authStateDescription(props.provider)}
            </div>
            <Show when={props.provider.has_auth}>
              <div class={styles.btnRow}>
                <button
                  class={styles.btnSecondary}
                  disabled={props.clearAuthSubmitting}
                  onClick={() => props.onClearSelectedProviderAuth()}
                >
                  {props.clearAuthSubmitting ? "Clearing…" : "Clear Credential"}
                </button>
              </div>
            </Show>
          </div>
          <Show when={props.selectedAuthMethods.length > 0}>
            <div class={styles.itemList}>
              <For each={props.selectedAuthMethods}>
                {(method, index) => (
                  <div class={styles.authMethodCard}>
                    <div class={styles.metaRow}>
                      <span class={styles.itemName}>{method.name}</span>
                      <span class={styles.itemMeta}>{method.description}</span>
                    </div>
                    <div class={styles.metaText}>
                      {method.description === "code"
                        ? "Opens an authorization page, then expects a pasted code."
                        : "Opens an authorization page and finishes through the browser callback."}
                    </div>
                    <div class={styles.btnRow}>
                      <button
                        class={styles.btnSecondary}
                        disabled={props.oauthSubmitting}
                        onClick={() => props.onStartOAuth(index(), method)}
                      >
                        {props.oauthSubmitting
                          ? "Starting…"
                          : props.provider.auth_type === "oauth"
                            ? "Reauthorize"
                            : "Authorize"}
                      </button>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
          <Show when={props.pendingOAuth}>
            {(flow) => (
              <div class={styles.oauthFlowCard}>
                <div class={styles.metaRow}>
                  <span class={styles.itemName}>OAuth In Progress</span>
                  <span class={styles.itemMeta}>
                    {flow().methodType === "code"
                      ? "Manual code"
                      : "Browser callback"}
                  </span>
                </div>
                <div class={styles.oauthStep}>
                  <span class={styles.stepBadge}>1</span>
                  <div class={styles.stepBody}>
                    <div class={styles.itemName}>{flow().methodLabel}</div>
                    <div class={styles.metaText}>
                      {flow().popupBlocked
                        ? "Open the authorization page manually."
                        : "Complete the provider sign-in flow in the opened browser tab."}
                    </div>
                  </div>
                </div>
                <Show when={flow().instructions}>
                  <div class={styles.metaText}>{flow().instructions}</div>
                </Show>
                <Show when={flow().popupBlocked}>
                  <div class={styles.popupHint}>
                    Browser popup was blocked. Open the authorization page manually.
                  </div>
                </Show>
                <Show when={flow().url}>
                  <a
                    class={styles.inlineLink}
                    href={flow().url}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Open authorization page
                  </a>
                </Show>
                <div class={styles.oauthStep}>
                  <span class={styles.stepBadge}>2</span>
                  <div class={styles.stepBody}>
                    <div class={styles.itemName}>
                      {oauthStepLabel(flow().methodType)}
                    </div>
                    <div class={styles.metaText}>
                      {flow().methodType === "code"
                        ? "After the provider shows a code, paste it here and complete auth."
                        : "When the browser flow finishes, confirm here so the server stores the credential."}
                    </div>
                  </div>
                </div>
                <Show when={flow().methodType === "code"}>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Authorization Code</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.oauthCode}
                      onInput={(e) => props.onOAuthCodeChange(e.currentTarget.value)}
                      placeholder="Paste the returned code"
                    />
                  </div>
                </Show>
                <div class={styles.btnRow}>
                  <button
                    class={styles.btnPrimary}
                    disabled={props.oauthSubmitting}
                    onClick={() => props.onFinishOAuth()}
                  >
                    {props.oauthSubmitting ? "Completing…" : "Complete Auth"}
                  </button>
                  <Show when={flow().url}>
                    <button
                      class={styles.btnSecondary}
                      onClick={() => props.onOpenOAuthAgain()}
                    >
                      Open Again
                    </button>
                  </Show>
                  <button
                    class={styles.btnSecondary}
                    onClick={() => props.onCancelOAuth()}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </Show>
        </div>

        <Show when={props.provider.models.length > 0}>
          <div class={styles.sectionCard}>
            <div class={styles.sectionCardHeader}>
              <div>
                <div class={styles.itemName}>Available Models</div>
                <div class={styles.itemMeta}>
                  Runtime models currently exposed by this provider
                </div>
              </div>
            </div>
            <div class={styles.summaryGrid}>
              <div class={styles.summaryCard}>
                <span class={styles.summaryLabel}>Visible</span>
                <span class={styles.summaryValue}>
                  {props.filteredSelectedModels.length}
                </span>
                <span class={styles.summaryMeta}>
                  Models matching current search
                </span>
              </div>
              <div class={styles.summaryCard}>
                <span class={styles.summaryLabel}>Total</span>
                <span class={styles.summaryValue}>
                  {props.provider.models.length}
                </span>
                <span class={styles.summaryMeta}>
                  Runtime models exposed by provider
                </span>
              </div>
            </div>
            <div class={styles.filterBar}>
              <input
                class={styles.fieldInput}
                type="text"
                value={props.modelFilterQuery}
                onInput={(e) => props.onModelFilterQueryChange(e.currentTarget.value)}
                placeholder="Search by model id, name or family"
              />
            </div>
            <Show
              when={props.filteredSelectedModels.length > 0}
              fallback={<div class={styles.empty}>No models match the current search</div>}
            >
              <div class={styles.compactItemList}>
                <For each={props.filteredSelectedModels}>
                  {(model) => (
                    <div class={styles.compactItem}>
                      <div class={styles.itemBody}>
                        <span class={styles.itemName}>{model.name || model.id}</span>
                        <span class={styles.itemMeta}>
                          {model.id}
                          <Show when={model.family}>{` · ${model.family}`}</Show>
                          <Show when={model.reasoning !== undefined}>
                            {` · reasoning ${model.reasoning ? "on" : "off"}`}
                          </Show>
                          <Show when={model.tool_call !== undefined}>
                            {` · tools ${model.tool_call ? "on" : "off"}`}
                          </Show>
                        </span>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </div>
        </Show>

        <div class={styles.sectionCard}>
          <div class={styles.sectionCardHeader}>
            <div>
              <div class={styles.itemName}>Model Overrides</div>
              <div class={styles.itemMeta}>
                Provider-local aliases and capability overrides
              </div>
            </div>
          </div>
          <div class={styles.overrideWorkspace}>
            <div class={styles.overrideListPanel}>
              <div class={styles.overridePanelHeader}>
                <div>
                  <div class={styles.itemName}>Override List</div>
                  <div class={styles.itemMeta}>
                    {props.provider.model_overrides.length} configured override
                    {props.provider.model_overrides.length === 1 ? "" : "s"}
                  </div>
                </div>
              </div>
              <div class={styles.itemList}>
                <For each={props.provider.model_overrides}>
                  {(override) => (
                    <button
                      class={styles.item}
                      classList={{
                        [styles.active]:
                          props.selectedModelOverrideKey === override.key,
                      }}
                      onClick={() => props.onSelectModelOverrideKey(override.key)}
                    >
                      <div class={styles.itemBody}>
                        <span class={styles.itemName}>{override.name || override.key}</span>
                        <span class={styles.itemMeta}>
                          {override.key}
                          <Show when={override.model}>{` · ${override.model}`}</Show>
                          <Show when={override.family}>{` · ${override.family}`}</Show>
                        </span>
                      </div>
                    </button>
                  )}
                </For>
                <button
                  class={styles.item}
                  classList={{
                    [styles.active]:
                      props.selectedModelOverrideKey === NEW_MODEL_OVERRIDE_KEY,
                  }}
                  onClick={() => props.onSelectModelOverrideKey(NEW_MODEL_OVERRIDE_KEY)}
                >
                  <div class={styles.itemBody}>
                    <span class={styles.itemName}>Add Model Override</span>
                    <span class={styles.itemMeta}>
                      Create a provider-local model alias
                    </span>
                  </div>
                </button>
              </div>
            </div>

            <div class={styles.overrideEditorPanel}>
              <div class={styles.overridePanelHeader}>
                <div>
                  <div class={styles.itemName}>
                    {props.selectedModelOverride ? "Edit Override" : "New Override"}
                  </div>
                  <div class={styles.itemMeta}>
                    {props.selectedModelOverride
                      ? "Adjust alias mapping and per-model capabilities"
                      : "Create a new alias for a provider model"}
                  </div>
                </div>
              </div>
              <div class={styles.metaCard}>
                <Show when={props.selectedModelOverride}>
                  {(override) => (
                    <div class={styles.overrideSummary}>
                      <div class={styles.metaRow}>
                        <div>
                          <div class={styles.itemName}>
                            {override().name || override().key}
                          </div>
                          <div class={styles.itemMeta}>
                            {override().key}
                            <Show when={override().model}>
                              {` · ${override().model}`}
                            </Show>
                            <Show when={override().family}>
                              {` · ${override().family}`}
                            </Show>
                            <Show when={override().status}>
                              {` · ${override().status}`}
                            </Show>
                          </div>
                        </div>
                        <Show when={override().release_date}>
                          <span class={styles.statusBadge}>
                            {override().release_date}
                          </span>
                        </Show>
                      </div>
                      <div class={styles.chipRow}>
                        <span class={styles.summaryChip}>
                          Reasoning: {boolSummary(override().reasoning)}
                        </span>
                        <span class={styles.summaryChip}>
                          Tool Call: {boolSummary(override().tool_call)}
                        </span>
                        <span class={styles.summaryChip}>
                          Attachment: {boolSummary(override().attachment)}
                        </span>
                        <span class={styles.summaryChip}>
                          Temperature: {boolSummary(override().temperature)}
                        </span>
                        <span class={styles.summaryChip}>
                          Experimental: {boolSummary(override().experimental, "Yes", "No")}
                        </span>
                      </div>
                      <div class={styles.summaryList}>
                        <For
                          each={[
                            jsonFieldSummary("Headers", override().headers),
                            jsonFieldSummary("Options", override().options),
                            jsonFieldSummary("Variants", override().variants),
                            jsonFieldSummary("Modalities", override().modalities),
                            jsonFieldSummary("Interleaved", override().interleaved),
                            jsonFieldSummary("Cost", override().cost),
                            jsonFieldSummary("Limit", override().limit),
                          ].filter((value): value is string => Boolean(value))}
                        >
                          {(item) => (
                            <span class={styles.summaryListItem}>{item}</span>
                          )}
                        </For>
                      </div>
                    </div>
                  )}
                </Show>
                <div class={styles.editorFieldGrid}>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Override Key</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideKeyInput}
                      onInput={(e) =>
                        props.onModelOverrideKeyInputChange(e.currentTarget.value)}
                      placeholder="glm-5-fast"
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Target Model ID</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideModelInput}
                      onInput={(e) =>
                        props.onModelOverrideModelInputChange(e.currentTarget.value)}
                      placeholder="provider-native-model-id"
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Display Name</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideNameInput}
                      onInput={(e) =>
                        props.onModelOverrideNameInputChange(e.currentTarget.value)}
                      placeholder="Optional display name"
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Model Base URL</label>
                    <input
                      class={styles.fieldInput}
                      type="url"
                      value={props.modelOverrideBaseUrlInput}
                      onInput={(e) =>
                        props.onModelOverrideBaseUrlInputChange(e.currentTarget.value)}
                      placeholder="Optional model-specific endpoint"
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Family</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideFamilyInput}
                      onInput={(e) =>
                        props.onModelOverrideFamilyInputChange(e.currentTarget.value)}
                      placeholder="gpt, qwen, glm, gemini..."
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Reasoning</label>
                    <select
                      class={styles.fieldSelect}
                      value={props.modelOverrideReasoningInput}
                      onChange={(e) =>
                        props.onModelOverrideReasoningInputChange(
                          e.currentTarget.value,
                        )}
                    >
                      <option value="">inherit</option>
                      <option value="true">enabled</option>
                      <option value="false">disabled</option>
                    </select>
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Tool Call</label>
                    <select
                      class={styles.fieldSelect}
                      value={props.modelOverrideToolCallInput}
                      onChange={(e) =>
                        props.onModelOverrideToolCallInputChange(
                          e.currentTarget.value,
                        )}
                    >
                      <option value="">inherit</option>
                      <option value="true">enabled</option>
                      <option value="false">disabled</option>
                    </select>
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Status</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideStatusInput}
                      onInput={(e) =>
                        props.onModelOverrideStatusInputChange(
                          e.currentTarget.value,
                        )}
                      placeholder="stable, preview, deprecated..."
                    />
                  </div>
                  <div class={styles.field}>
                    <label class={styles.fieldLabel}>Release Date</label>
                    <input
                      class={styles.fieldInput}
                      type="text"
                      value={props.modelOverrideReleaseDateInput}
                      onInput={(e) =>
                        props.onModelOverrideReleaseDateInputChange(
                          e.currentTarget.value,
                        )}
                      placeholder="2026-03-21"
                    />
                  </div>
                </div>
                <div class={styles.advancedToggleRow}>
                  <div>
                    <div class={styles.itemName}>Advanced Fields</div>
                    <div class={styles.itemMeta}>
                      Headers, provider options, variants, modalities, limits and
                      capability flags.
                    </div>
                  </div>
                  <button
                    class={styles.btnSecondary}
                    type="button"
                    onClick={() => props.onToggleAdvancedOverrideFields()}
                  >
                    {props.showAdvancedOverrideFields
                      ? "Hide Advanced"
                      : "Show Advanced"}
                  </button>
                </div>
                <Show when={props.showAdvancedOverrideFields}>
                  <>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Headers JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="4"
                        value={props.modelOverrideHeadersInput}
                        onInput={(e) =>
                          props.onModelOverrideHeadersInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "x-api-version": "2026-03-01"\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Options JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="5"
                        value={props.modelOverrideOptionsInput}
                        onInput={(e) =>
                          props.onModelOverrideOptionsInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "temperature": 0.2\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Variants JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="6"
                        value={props.modelOverrideVariantsInput}
                        onInput={(e) =>
                          props.onModelOverrideVariantsInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "high": {\n    "disabled": false\n  }\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Modalities JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="5"
                        value={props.modelOverrideModalitiesInput}
                        onInput={(e) =>
                          props.onModelOverrideModalitiesInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "input": ["text"],\n  "output": ["text"]\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Interleaved JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="4"
                        value={props.modelOverrideInterleavedInput}
                        onInput={(e) =>
                          props.onModelOverrideInterleavedInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'true\nor\n{\n  "field": "reasoning_content"\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Cost JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="5"
                        value={props.modelOverrideCostInput}
                        onInput={(e) =>
                          props.onModelOverrideCostInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "input": 1.2,\n  "output": 4.8\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Limit JSON</label>
                      <textarea
                        class={styles.fieldInput}
                        rows="5"
                        value={props.modelOverrideLimitInput}
                        onInput={(e) =>
                          props.onModelOverrideLimitInputChange(
                            e.currentTarget.value,
                          )}
                        placeholder={'{\n  "context": 128000,\n  "output": 8192\n}'}
                      />
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Attachment</label>
                      <select
                        class={styles.fieldSelect}
                        value={props.modelOverrideAttachmentInput}
                        onChange={(e) =>
                          props.onModelOverrideAttachmentInputChange(
                            e.currentTarget.value,
                          )}
                      >
                        <option value="">inherit</option>
                        <option value="true">enabled</option>
                        <option value="false">disabled</option>
                      </select>
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Temperature</label>
                      <select
                        class={styles.fieldSelect}
                        value={props.modelOverrideTemperatureInput}
                        onChange={(e) =>
                          props.onModelOverrideTemperatureInputChange(
                            e.currentTarget.value,
                          )}
                      >
                        <option value="">inherit</option>
                        <option value="true">enabled</option>
                        <option value="false">disabled</option>
                      </select>
                    </div>
                    <div class={styles.field}>
                      <label class={styles.fieldLabel}>Experimental</label>
                      <select
                        class={styles.fieldSelect}
                        value={props.modelOverrideExperimentalInput}
                        onChange={(e) =>
                          props.onModelOverrideExperimentalInputChange(
                            e.currentTarget.value,
                          )}
                      >
                        <option value="">inherit</option>
                        <option value="true">enabled</option>
                        <option value="false">disabled</option>
                      </select>
                    </div>
                  </>
                </Show>
                <div class={styles.metaCard}>
                  <div class={styles.metaText}>
                    Base fields cover identity and routing. Advanced fields are
                    optional per-model capability and pricing overrides.
                  </div>
                </div>
                <div class={styles.btnRow}>
                  <button
                    class={styles.btnPrimary}
                    disabled={props.modelOverrideSubmitting}
                    onClick={() => props.onSaveModelOverride()}
                  >
                    {props.modelOverrideSubmitting ? "Saving…" : "Save Override"}
                  </button>
                  <Show when={props.selectedModelOverride}>
                    <button
                      class={styles.btnDanger}
                      disabled={props.modelOverrideDeleting}
                      onClick={() => props.onDeleteModelOverride()}
                    >
                      {props.modelOverrideDeleting
                        ? "Removing…"
                        : "Delete Override"}
                    </button>
                  </Show>
                </div>
                <Show when={props.selectedModelOverride}>
                  <div class={styles.dangerHint}>
                    Deleting an override removes only this provider-local alias.
                    The underlying model is not deleted.
                  </div>
                </Show>
              </div>
            </div>
          </div>
        </div>

        <div class={styles.sectionCard}>
          <div class={styles.sectionCardHeader}>
            <div>
              <div class={styles.itemName}>Danger Zone</div>
              <div class={styles.itemMeta}>
                Remove the provider configuration and stored credential
              </div>
            </div>
          </div>
          <div class={styles.btnRow}>
            <button
              class={styles.btnDanger}
              disabled={props.deleteSubmitting}
              onClick={() => props.onDeleteProvider()}
            >
              {props.deleteSubmitting ? "Removing…" : "Disconnect Provider"}
            </button>
          </div>
          <div class={styles.dangerHint}>
            Disconnect removes the saved provider configuration and stored credential
            for this provider.
          </div>
        </div>
      </div>
    </div>
  );
};
