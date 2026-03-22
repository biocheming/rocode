import {
  type Component,
  Show,
  createEffect,
  createMemo,
  createResource,
  createSignal,
} from "solid-js";
import { ApiError } from "~/api/client";
import {
  authorizeProviderOAuth,
  clearProviderAuth,
  completeProviderOAuth,
  connectProvider,
  deleteProvider,
  deleteProviderModel,
  getManagedProviders,
  getProviderAuthMethods,
  getProviderConnectSchema,
  updateProvider,
  updateProviderModel,
} from "~/api/providers";
import type {
  ConnectProviderRequest,
  ManagedModelOverride,
  UpdateProviderModelRequest,
  UpdateProviderRequest,
} from "~/api/types";
import { loadProviders } from "~/stores/app";
import styles from "./SettingsDrawer.module.css";
import { ProviderConnectView } from "./provider/ProviderConnectView";
import { ProviderDetailView } from "./provider/ProviderDetailView";
import { ProviderManagedListView } from "./provider/ProviderManagedListView";
import {
  NEW_MODEL_OVERRIDE_KEY,
  hasAdvancedOverrideConfig,
  matchesProviderQuery,
  matchesProviderStatusFilter,
  providerGroupKey,
  providerGroupLabel,
  type FeedbackTone,
  type ManagedProviderGroup,
  type PendingOAuthFlow,
  type ProviderWorkspaceView,
} from "./provider/shared";

function formatError(error: unknown): string {
  if (error instanceof ApiError) {
    return error.body || error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Request failed";
}

function boolInputValue(value?: boolean): string {
  if (value === true) return "true";
  if (value === false) return "false";
  return "";
}

function parseBoolInput(value: string): boolean | undefined {
  if (value === "true") return true;
  if (value === "false") return false;
  return undefined;
}

function stringifyJsonInput(value: unknown): string {
  if (value == null) return "";
  return JSON.stringify(value, null, 2);
}

function parseJsonInput(
  label: string,
  raw: string,
):
  | { ok: true; value: Record<string, unknown> | Record<string, string> | undefined }
  | { ok: false; message: string } {
  const trimmed = raw.trim();
  if (!trimmed) {
    return { ok: true, value: undefined };
  }
  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return { ok: false, message: `${label} must be a JSON object` };
    }
    return { ok: true, value: parsed as Record<string, unknown> };
  } catch (error) {
    return {
      ok: false,
      message: `${label} JSON is invalid: ${error instanceof Error ? error.message : "parse error"}`,
    };
  }
}

function parseJsonValueInput(
  label: string,
  raw: string,
): { ok: true; value: unknown } | { ok: false; message: string } {
  const trimmed = raw.trim();
  if (!trimmed) {
    return { ok: true, value: undefined };
  }
  try {
    return { ok: true, value: JSON.parse(trimmed) };
  } catch (error) {
    return {
      ok: false,
      message: `${label} JSON is invalid: ${error instanceof Error ? error.message : "parse error"}`,
    };
  }
}

export const ProviderSection: Component = () => {
  const [selectedProviderId, setSelectedProviderId] = createSignal<string | null>(null);

  const [connectMode, setConnectMode] = createSignal<"known" | "custom">("known");
  const [knownProviderId, setKnownProviderId] = createSignal("");
  const [customProviderId, setCustomProviderId] = createSignal("");
  const [customBaseUrl, setCustomBaseUrl] = createSignal("");
  const [customProtocol, setCustomProtocol] = createSignal("");
  const [apiKey, setApiKey] = createSignal("");
  const [connectSubmitting, setConnectSubmitting] = createSignal(false);
  const [providerFilterQuery, setProviderFilterQuery] = createSignal("");
  const [providerStatusFilter, setProviderStatusFilter] = createSignal("all");
  const [modelFilterQuery, setModelFilterQuery] = createSignal("");
  const [activeWorkspaceView, setActiveWorkspaceView] =
    createSignal<ProviderWorkspaceView>("providers");

  const [editName, setEditName] = createSignal("");
  const [editBaseUrl, setEditBaseUrl] = createSignal("");
  const [editProtocol, setEditProtocol] = createSignal("");
  const [saveSubmitting, setSaveSubmitting] = createSignal(false);
  const [deleteSubmitting, setDeleteSubmitting] = createSignal(false);
  const [clearAuthSubmitting, setClearAuthSubmitting] = createSignal(false);
  const [modelOverrideSubmitting, setModelOverrideSubmitting] = createSignal(false);
  const [modelOverrideDeleting, setModelOverrideDeleting] = createSignal(false);
  const [oauthSubmitting, setOAuthSubmitting] = createSignal(false);
  const [oauthCode, setOAuthCode] = createSignal("");
  const [pendingOAuth, setPendingOAuth] = createSignal<PendingOAuthFlow | null>(null);
  const [selectedModelOverrideKey, setSelectedModelOverrideKey] = createSignal<string | null>(
    null,
  );
  const [modelOverrideKeyInput, setModelOverrideKeyInput] = createSignal("");
  const [modelOverrideNameInput, setModelOverrideNameInput] = createSignal("");
  const [modelOverrideModelInput, setModelOverrideModelInput] = createSignal("");
  const [modelOverrideBaseUrlInput, setModelOverrideBaseUrlInput] = createSignal("");
  const [modelOverrideFamilyInput, setModelOverrideFamilyInput] = createSignal("");
  const [modelOverrideReasoningInput, setModelOverrideReasoningInput] = createSignal("");
  const [modelOverrideToolCallInput, setModelOverrideToolCallInput] = createSignal("");
  const [modelOverrideHeadersInput, setModelOverrideHeadersInput] = createSignal("");
  const [modelOverrideOptionsInput, setModelOverrideOptionsInput] = createSignal("");
  const [modelOverrideVariantsInput, setModelOverrideVariantsInput] = createSignal("");
  const [modelOverrideModalitiesInput, setModelOverrideModalitiesInput] = createSignal("");
  const [modelOverrideInterleavedInput, setModelOverrideInterleavedInput] = createSignal("");
  const [modelOverrideCostInput, setModelOverrideCostInput] = createSignal("");
  const [modelOverrideLimitInput, setModelOverrideLimitInput] = createSignal("");
  const [modelOverrideAttachmentInput, setModelOverrideAttachmentInput] = createSignal("");
  const [modelOverrideTemperatureInput, setModelOverrideTemperatureInput] = createSignal("");
  const [modelOverrideStatusInput, setModelOverrideStatusInput] = createSignal("");
  const [modelOverrideReleaseDateInput, setModelOverrideReleaseDateInput] =
    createSignal("");
  const [modelOverrideExperimentalInput, setModelOverrideExperimentalInput] =
    createSignal("");
  const [showAdvancedOverrideFields, setShowAdvancedOverrideFields] = createSignal(false);

  const [feedback, setFeedback] = createSignal<{
    tone: FeedbackTone;
    text: string;
  } | null>(null);

  const [connectSchema, { refetch: refetchConnectSchema }] =
    createResource(getProviderConnectSchema);
  const [managedProviders, { refetch: refetchManagedProviders }] =
    createResource(getManagedProviders);
  const [providerAuthMethods, { refetch: refetchProviderAuthMethods }] =
    createResource(getProviderAuthMethods);

  const managedList = createMemo(() => managedProviders()?.providers ?? []);
  const selectedManagedProvider = createMemo(
    () =>
      managedList().find((provider) => provider.id === selectedProviderId()) ?? null,
  );
  const filteredManagedList = createMemo(() =>
    managedList().filter(
      (provider) =>
        matchesProviderStatusFilter(provider, providerStatusFilter())
        && matchesProviderQuery(provider, providerFilterQuery()),
    ),
  );
  const managedProviderCounts = createMemo(() => {
    const providers = managedList();
    return {
      all: providers.length,
      connected: providers.filter((provider) => provider.status === "connected").length,
      needsAuth: providers.filter((provider) => provider.status === "needs-auth").length,
      configured: providers.filter(
        (provider) => provider.status !== "connected" && provider.status !== "needs-auth",
      ).length,
    };
  });
  const groupedManagedProviders = createMemo<ManagedProviderGroup[]>(() => {
    const groups: ManagedProviderGroup[] = [];
    for (const key of ["connected", "needs-auth", "other"] as const) {
      const providers = filteredManagedList().filter(
        (provider) => providerGroupKey(provider.status) === key,
      );
      if (providers.length === 0) continue;
      groups.push({
        key,
        label: providerGroupLabel(key),
        providers,
      });
    }
    return groups;
  });
  const selectedKnownProvider = createMemo(
    () =>
      connectSchema()?.providers.find((provider) => provider.id === knownProviderId())
      ?? null,
  );
  const selectedCustomProtocol = createMemo(
    () =>
      connectSchema()?.protocols.find((protocol) => protocol.id === customProtocol())
      ?? null,
  );
  const selectedAuthMethods = createMemo(() => {
    const providerId = selectedManagedProvider()?.id;
    if (!providerId) return [];
    return providerAuthMethods()?.[providerId] ?? [];
  });
  const filteredSelectedModels = createMemo(() => {
    const provider = selectedManagedProvider();
    if (!provider) return [];
    const query = modelFilterQuery().trim().toLowerCase();
    if (!query) return provider.models;
    return provider.models.filter((model) =>
      [model.id, model.name, model.family]
        .filter((value): value is string => Boolean(value))
        .some((value) => value.toLowerCase().includes(query)),
    );
  });
  const selectedModelOverride = createMemo<ManagedModelOverride | null>(() => {
    const provider = selectedManagedProvider();
    const key = selectedModelOverrideKey();
    if (!provider || !key || key === NEW_MODEL_OVERRIDE_KEY) {
      return null;
    }
    return provider.model_overrides.find((override) => override.key === key) ?? null;
  });
  const selectedOverrideHasAdvancedConfig = createMemo(() =>
    hasAdvancedOverrideConfig(selectedModelOverride()),
  );

  const refreshProviderState = async () => {
    await Promise.all([
      loadProviders(),
      refetchConnectSchema(),
      refetchManagedProviders(),
      refetchProviderAuthMethods(),
    ]);
  };

  createEffect(() => {
    const schema = connectSchema();
    if (!schema) return;
    if (!knownProviderId() && schema.providers.length > 0) {
      setKnownProviderId(schema.providers[0].id);
    }
    if (!customProtocol() && schema.protocols.length > 0) {
      setCustomProtocol(schema.protocols[0].id);
    }
  });

  createEffect(() => {
    const providers = managedList();
    const current = selectedProviderId();
    if (providers.length === 0) {
      if (current !== null) {
        setSelectedProviderId(null);
      }
      if (activeWorkspaceView() === "detail") {
        setActiveWorkspaceView("providers");
      }
      return;
    }
    if (!current || !providers.some((provider) => provider.id === current)) {
      setSelectedProviderId(providers[0].id);
    }
  });

  createEffect(() => {
    if (!selectedManagedProvider() && activeWorkspaceView() === "detail") {
      setActiveWorkspaceView("providers");
    }
  });

  createEffect(() => {
    const provider = selectedManagedProvider();
    if (!provider) {
      setEditName("");
      setEditBaseUrl("");
      setEditProtocol("");
      setSelectedModelOverrideKey(null);
      setModelOverrideKeyInput("");
      setModelOverrideNameInput("");
      setModelOverrideModelInput("");
      setModelOverrideBaseUrlInput("");
      setModelOverrideFamilyInput("");
      setModelOverrideReasoningInput("");
      setModelOverrideToolCallInput("");
      setModelOverrideHeadersInput("");
      setModelOverrideOptionsInput("");
      setModelOverrideVariantsInput("");
      setModelOverrideModalitiesInput("");
      setModelOverrideInterleavedInput("");
      setModelOverrideCostInput("");
      setModelOverrideLimitInput("");
      setModelOverrideAttachmentInput("");
      setModelOverrideTemperatureInput("");
      setModelOverrideStatusInput("");
      setModelOverrideReleaseDateInput("");
      setModelOverrideExperimentalInput("");
      return;
    }
    setEditName(provider.name ?? "");
    setEditBaseUrl(provider.base_url ?? "");
    setEditProtocol(provider.protocol ?? "");
  });

  createEffect(() => {
    const provider = selectedManagedProvider();
    if (!provider) return;
    const currentKey = selectedModelOverrideKey();
    const overrides = provider.model_overrides;

    if (currentKey === NEW_MODEL_OVERRIDE_KEY) {
      setModelOverrideKeyInput("");
      setModelOverrideNameInput("");
      setModelOverrideModelInput("");
      setModelOverrideBaseUrlInput("");
      setModelOverrideFamilyInput("");
      setModelOverrideReasoningInput("");
      setModelOverrideToolCallInput("");
      setModelOverrideHeadersInput("");
      setModelOverrideOptionsInput("");
      setModelOverrideVariantsInput("");
      setModelOverrideModalitiesInput("");
      setModelOverrideInterleavedInput("");
      setModelOverrideCostInput("");
      setModelOverrideLimitInput("");
      setModelOverrideAttachmentInput("");
      setModelOverrideTemperatureInput("");
      setModelOverrideStatusInput("");
      setModelOverrideReleaseDateInput("");
      setModelOverrideExperimentalInput("");
      return;
    }

    const selectedOverride = overrides.find((override) => override.key === currentKey);
    if (selectedOverride) {
      setModelOverrideKeyInput(selectedOverride.key);
      setModelOverrideNameInput(selectedOverride.name ?? "");
      setModelOverrideModelInput(selectedOverride.model ?? "");
      setModelOverrideBaseUrlInput(selectedOverride.base_url ?? "");
      setModelOverrideFamilyInput(selectedOverride.family ?? "");
      setModelOverrideReasoningInput(boolInputValue(selectedOverride.reasoning));
      setModelOverrideToolCallInput(boolInputValue(selectedOverride.tool_call));
      setModelOverrideHeadersInput(stringifyJsonInput(selectedOverride.headers));
      setModelOverrideOptionsInput(stringifyJsonInput(selectedOverride.options));
      setModelOverrideVariantsInput(stringifyJsonInput(selectedOverride.variants));
      setModelOverrideModalitiesInput(stringifyJsonInput(selectedOverride.modalities));
      setModelOverrideInterleavedInput(
        stringifyJsonInput(selectedOverride.interleaved),
      );
      setModelOverrideCostInput(stringifyJsonInput(selectedOverride.cost));
      setModelOverrideLimitInput(stringifyJsonInput(selectedOverride.limit));
      setModelOverrideAttachmentInput(boolInputValue(selectedOverride.attachment));
      setModelOverrideTemperatureInput(boolInputValue(selectedOverride.temperature));
      setModelOverrideStatusInput(selectedOverride.status ?? "");
      setModelOverrideReleaseDateInput(selectedOverride.release_date ?? "");
      setModelOverrideExperimentalInput(
        boolInputValue(selectedOverride.experimental),
      );
      return;
    }

    if (overrides.length > 0) {
      const first = overrides[0];
      setSelectedModelOverrideKey(first.key);
      setModelOverrideKeyInput(first.key);
      setModelOverrideNameInput(first.name ?? "");
      setModelOverrideModelInput(first.model ?? "");
      setModelOverrideBaseUrlInput(first.base_url ?? "");
      setModelOverrideFamilyInput(first.family ?? "");
      setModelOverrideReasoningInput(boolInputValue(first.reasoning));
      setModelOverrideToolCallInput(boolInputValue(first.tool_call));
      setModelOverrideHeadersInput(stringifyJsonInput(first.headers));
      setModelOverrideOptionsInput(stringifyJsonInput(first.options));
      setModelOverrideVariantsInput(stringifyJsonInput(first.variants));
      setModelOverrideModalitiesInput(stringifyJsonInput(first.modalities));
      setModelOverrideInterleavedInput(stringifyJsonInput(first.interleaved));
      setModelOverrideCostInput(stringifyJsonInput(first.cost));
      setModelOverrideLimitInput(stringifyJsonInput(first.limit));
      setModelOverrideAttachmentInput(boolInputValue(first.attachment));
      setModelOverrideTemperatureInput(boolInputValue(first.temperature));
      setModelOverrideStatusInput(first.status ?? "");
      setModelOverrideReleaseDateInput(first.release_date ?? "");
      setModelOverrideExperimentalInput(boolInputValue(first.experimental));
      return;
    }

    setSelectedModelOverrideKey(NEW_MODEL_OVERRIDE_KEY);
    setModelOverrideKeyInput("");
    setModelOverrideNameInput("");
    setModelOverrideModelInput("");
    setModelOverrideBaseUrlInput("");
    setModelOverrideFamilyInput("");
    setModelOverrideReasoningInput("");
    setModelOverrideToolCallInput("");
    setModelOverrideHeadersInput("");
    setModelOverrideOptionsInput("");
    setModelOverrideVariantsInput("");
    setModelOverrideModalitiesInput("");
    setModelOverrideInterleavedInput("");
    setModelOverrideCostInput("");
    setModelOverrideLimitInput("");
    setModelOverrideAttachmentInput("");
    setModelOverrideTemperatureInput("");
    setModelOverrideStatusInput("");
    setModelOverrideReleaseDateInput("");
    setModelOverrideExperimentalInput("");
  });

  createEffect(() => {
    const providerId = selectedManagedProvider()?.id ?? null;
    const flow = pendingOAuth();
    if (!flow) return;
    if (providerId !== flow.providerId) {
      setPendingOAuth(null);
      setOAuthCode("");
    }
  });

  createEffect(() => {
    selectedManagedProvider()?.id;
    setModelFilterQuery("");
  });

  createEffect(() => {
    const currentKey = selectedModelOverrideKey();
    if (currentKey === NEW_MODEL_OVERRIDE_KEY) {
      setShowAdvancedOverrideFields(false);
      return;
    }
    setShowAdvancedOverrideFields(selectedOverrideHasAdvancedConfig());
  });

  const submitConnect = async () => {
    const key = apiKey().trim();
    if (!key) {
      setFeedback({ tone: "error", text: "API key is required" });
      return;
    }

    const isCustom = connectMode() === "custom";
    const providerId = isCustom ? customProviderId().trim() : knownProviderId().trim();
    if (!providerId) {
      setFeedback({ tone: "error", text: "Provider ID is required" });
      return;
    }

    let request: ConnectProviderRequest;
    if (isCustom) {
      const baseUrl = customBaseUrl().trim();
      const protocol = customProtocol().trim();
      if (!baseUrl) {
        setFeedback({
          tone: "error",
          text: "Base URL is required for custom providers",
        });
        return;
      }
      if (!protocol) {
        setFeedback({
          tone: "error",
          text: "Protocol is required for custom providers",
        });
        return;
      }
      request = {
        provider_id: providerId,
        api_key: key,
        base_url: baseUrl,
        protocol,
      };
    } else {
      request = {
        provider_id: providerId,
        api_key: key,
      };
    }

    setConnectSubmitting(true);
    setFeedback(null);
    try {
      await connectProvider(request);
      await refreshProviderState();
      setSelectedProviderId(providerId);
      setActiveWorkspaceView("detail");
      setApiKey("");
      setFeedback({ tone: "success", text: `Connected ${providerId}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setConnectSubmitting(false);
    }
  };

  const saveSelectedProvider = async () => {
    const provider = selectedManagedProvider();
    if (!provider) return;

    const request: UpdateProviderRequest = {
      name: editName().trim(),
    };

    const canEditConnection =
      !provider.known || Boolean(provider.base_url) || Boolean(provider.protocol);
    if (canEditConnection) {
      const baseUrl = editBaseUrl().trim();
      const protocol = editProtocol().trim();
      if (!baseUrl) {
        setFeedback({ tone: "error", text: "Base URL is required" });
        return;
      }
      if (!protocol) {
        setFeedback({ tone: "error", text: "Protocol is required" });
        return;
      }
      request.base_url = baseUrl;
      request.protocol = protocol;
    }

    setSaveSubmitting(true);
    setFeedback(null);
    try {
      await updateProvider(provider.id, request);
      await refreshProviderState();
      setFeedback({ tone: "success", text: `Updated ${provider.id}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setSaveSubmitting(false);
    }
  };

  const removeSelectedProvider = async () => {
    const provider = selectedManagedProvider();
    if (!provider) return;
    const confirmed = window.confirm(
      `Disconnect provider "${provider.id}"? This removes its saved config and credential.`,
    );
    if (!confirmed) return;

    setDeleteSubmitting(true);
    setFeedback(null);
    try {
      await deleteProvider(provider.id);
      await refreshProviderState();
      setFeedback({ tone: "success", text: `Removed ${provider.id}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setDeleteSubmitting(false);
    }
  };

  const clearSelectedProviderAuth = async () => {
    const provider = selectedManagedProvider();
    if (!provider || !provider.has_auth) return;
    const confirmed = window.confirm(
      `Clear the stored credential for "${provider.id}"? The provider config will remain.`,
    );
    if (!confirmed) return;

    setClearAuthSubmitting(true);
    setFeedback(null);
    try {
      await clearProviderAuth(provider.id);
      await refreshProviderState();
      setPendingOAuth(null);
      setOAuthCode("");
      setFeedback({ tone: "success", text: `Cleared credential for ${provider.id}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setClearAuthSubmitting(false);
    }
  };

  const saveModelOverride = async () => {
    const provider = selectedManagedProvider();
    if (!provider) return;

    const modelKey = modelOverrideKeyInput().trim();
    if (!modelKey) {
      setFeedback({ tone: "error", text: "Model override key is required" });
      return;
    }

    const headers = parseJsonInput("Headers", modelOverrideHeadersInput());
    if (!headers.ok) {
      setFeedback({ tone: "error", text: headers.message });
      return;
    }
    const options = parseJsonInput("Options", modelOverrideOptionsInput());
    if (!options.ok) {
      setFeedback({ tone: "error", text: options.message });
      return;
    }
    const variants = parseJsonInput("Variants", modelOverrideVariantsInput());
    if (!variants.ok) {
      setFeedback({ tone: "error", text: variants.message });
      return;
    }
    const modalities = parseJsonInput("Modalities", modelOverrideModalitiesInput());
    if (!modalities.ok) {
      setFeedback({ tone: "error", text: modalities.message });
      return;
    }
    const cost = parseJsonInput("Cost", modelOverrideCostInput());
    if (!cost.ok) {
      setFeedback({ tone: "error", text: cost.message });
      return;
    }
    const limit = parseJsonInput("Limit", modelOverrideLimitInput());
    if (!limit.ok) {
      setFeedback({ tone: "error", text: limit.message });
      return;
    }
    const interleaved = parseJsonValueInput(
      "Interleaved",
      modelOverrideInterleavedInput(),
    );
    if (!interleaved.ok) {
      setFeedback({ tone: "error", text: interleaved.message });
      return;
    }

    const request: UpdateProviderModelRequest = {
      name: modelOverrideNameInput().trim() || undefined,
      model: modelOverrideModelInput().trim() || undefined,
      base_url: modelOverrideBaseUrlInput().trim() || undefined,
      family: modelOverrideFamilyInput().trim() || undefined,
      reasoning: parseBoolInput(modelOverrideReasoningInput()),
      tool_call: parseBoolInput(modelOverrideToolCallInput()),
      headers: headers.value as Record<string, string> | undefined,
      options: options.value as Record<string, unknown> | undefined,
      variants: variants.value as Record<string, unknown> | undefined,
      modalities: modalities.value as Record<string, unknown> | undefined,
      interleaved: interleaved.value,
      cost: cost.value as Record<string, unknown> | undefined,
      limit: limit.value as Record<string, unknown> | undefined,
      attachment: parseBoolInput(modelOverrideAttachmentInput()),
      temperature: parseBoolInput(modelOverrideTemperatureInput()),
      status: modelOverrideStatusInput().trim() || undefined,
      release_date: modelOverrideReleaseDateInput().trim() || undefined,
      experimental: parseBoolInput(modelOverrideExperimentalInput()),
    };

    setModelOverrideSubmitting(true);
    setFeedback(null);
    try {
      await updateProviderModel(provider.id, modelKey, request);
      await refreshProviderState();
      setSelectedModelOverrideKey(modelKey);
      setFeedback({ tone: "success", text: `Saved model override ${modelKey}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setModelOverrideSubmitting(false);
    }
  };

  const removeModelOverride = async () => {
    const provider = selectedManagedProvider();
    const override = selectedModelOverride();
    if (!provider || !override) return;
    const confirmed = window.confirm(
      `Delete model override "${override.key}" from "${provider.id}"?`,
    );
    if (!confirmed) return;

    setModelOverrideDeleting(true);
    setFeedback(null);
    try {
      await deleteProviderModel(provider.id, override.key);
      await refreshProviderState();
      setSelectedModelOverrideKey(null);
      setFeedback({ tone: "success", text: `Removed model override ${override.key}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setModelOverrideDeleting(false);
    }
  };

  const openOAuthUrl = (url: string): boolean => {
    if (!url) return false;
    const opened = window.open(url, "_blank", "noopener,noreferrer");
    return opened !== null;
  };

  const startOAuth = async (methodIndex: number, method: { name: string }) => {
    const provider = selectedManagedProvider();
    if (!provider) return;

    setOAuthSubmitting(true);
    setFeedback(null);
    try {
      const response = await authorizeProviderOAuth(provider.id, methodIndex);
      const popupOpened = response.url ? openOAuthUrl(response.url) : true;
      setPendingOAuth({
        providerId: provider.id,
        methodIndex,
        methodLabel: method.name,
        methodType: response.method,
        instructions: response.instructions,
        url: response.url,
        popupBlocked: !popupOpened,
      });
      setOAuthCode("");
      if (response.url && !popupOpened) {
        setFeedback({
          tone: "warn",
          text: "Authorization page was blocked by the browser. Use the link below to open it manually.",
        });
      } else {
        setFeedback({ tone: "success", text: `Started ${method.name} for ${provider.id}` });
      }
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setOAuthSubmitting(false);
    }
  };

  const finishOAuth = async () => {
    const flow = pendingOAuth();
    if (!flow) return;

    if (flow.methodType === "code" && !oauthCode().trim()) {
      setFeedback({ tone: "error", text: "Authorization code is required" });
      return;
    }

    setOAuthSubmitting(true);
    setFeedback(null);
    try {
      await completeProviderOAuth(
        flow.providerId,
        flow.methodIndex,
        flow.methodType === "code" ? oauthCode().trim() : undefined,
      );
      await refreshProviderState();
      setPendingOAuth(null);
      setOAuthCode("");
      setFeedback({ tone: "success", text: `Authorized ${flow.providerId}` });
    } catch (error) {
      setFeedback({ tone: "error", text: formatError(error) });
    } finally {
      setOAuthSubmitting(false);
    }
  };

  return (
    <div>
      <div class={styles.section}>
        <div class={styles.sectionTitle}>Providers Workspace</div>
        <div class={styles.metaText}>
          Connect new providers, browse configured ones, and manage the selected
          provider from one page.
        </div>
        <div class={styles.segmentedControl}>
          <button
            class={styles.segmentedButton}
            classList={{ [styles.active]: activeWorkspaceView() === "connect" }}
            onClick={() => setActiveWorkspaceView("connect")}
          >
            Connect
          </button>
          <button
            class={styles.segmentedButton}
            classList={{ [styles.active]: activeWorkspaceView() === "providers" }}
            onClick={() => setActiveWorkspaceView("providers")}
          >
            Providers
          </button>
          <Show when={selectedManagedProvider()}>
            <button
              class={styles.segmentedButton}
              classList={{ [styles.active]: activeWorkspaceView() === "detail" }}
              onClick={() => setActiveWorkspaceView("detail")}
            >
              Detail
            </button>
          </Show>
        </div>
      </div>

      <Show when={activeWorkspaceView() === "connect"}>
        <ProviderConnectView
          loading={Boolean(connectSchema.loading)}
          hasError={Boolean(connectSchema.error)}
          connectMode={connectMode()}
          knownProviderId={knownProviderId()}
          customProviderId={customProviderId()}
          customBaseUrl={customBaseUrl()}
          customProtocol={customProtocol()}
          apiKey={apiKey()}
          connectSubmitting={connectSubmitting()}
          knownProviders={connectSchema()?.providers ?? []}
          protocols={connectSchema()?.protocols ?? []}
          selectedKnownProvider={selectedKnownProvider()}
          selectedCustomProtocol={selectedCustomProtocol()}
          onConnectModeChange={setConnectMode}
          onKnownProviderIdChange={setKnownProviderId}
          onCustomProviderIdChange={setCustomProviderId}
          onCustomBaseUrlChange={setCustomBaseUrl}
          onCustomProtocolChange={setCustomProtocol}
          onApiKeyChange={setApiKey}
          onSubmitConnect={() => void submitConnect()}
          onRefresh={() => void refreshProviderState()}
        />
      </Show>

      <Show when={feedback()}>
        {(message) => (
          <div
            class={styles.inlineNotice}
            classList={{
              [styles.inlineNoticeError]: message().tone === "error",
              [styles.inlineNoticeSuccess]: message().tone === "success",
              [styles.inlineNoticeWarn]: message().tone === "warn",
            }}
          >
            {message().text}
          </div>
        )}
      </Show>

      <Show when={activeWorkspaceView() === "providers"}>
        <ProviderManagedListView
          loading={Boolean(managedProviders.loading)}
          managedList={managedList()}
          managedProviderCounts={managedProviderCounts()}
          providerFilterQuery={providerFilterQuery()}
          providerStatusFilter={providerStatusFilter()}
          filteredManagedListCount={filteredManagedList().length}
          groupedManagedProviders={groupedManagedProviders()}
          selectedProviderId={selectedProviderId()}
          onProviderFilterQueryChange={setProviderFilterQuery}
          onProviderStatusFilterChange={setProviderStatusFilter}
          onSelectProvider={(providerId) => {
            setSelectedProviderId(providerId);
            setActiveWorkspaceView("detail");
          }}
        />
      </Show>

      <Show when={activeWorkspaceView() === "detail" && selectedManagedProvider()}>
        {(provider) => (
          <ProviderDetailView
            provider={provider()}
            connectProtocols={connectSchema()?.protocols ?? []}
            canEditConnection={
              !provider().known
              || Boolean(provider().base_url)
              || Boolean(provider().protocol)
            }
            editName={editName()}
            editBaseUrl={editBaseUrl()}
            editProtocol={editProtocol()}
            saveSubmitting={saveSubmitting()}
            clearAuthSubmitting={clearAuthSubmitting()}
            deleteSubmitting={deleteSubmitting()}
            selectedAuthMethods={selectedAuthMethods()}
            oauthSubmitting={oauthSubmitting()}
            pendingOAuth={pendingOAuth()}
            oauthCode={oauthCode()}
            filteredSelectedModels={filteredSelectedModels()}
            modelFilterQuery={modelFilterQuery()}
            selectedModelOverrideKey={selectedModelOverrideKey()}
            selectedModelOverride={selectedModelOverride()}
            modelOverrideSubmitting={modelOverrideSubmitting()}
            modelOverrideDeleting={modelOverrideDeleting()}
            modelOverrideKeyInput={modelOverrideKeyInput()}
            modelOverrideModelInput={modelOverrideModelInput()}
            modelOverrideNameInput={modelOverrideNameInput()}
            modelOverrideBaseUrlInput={modelOverrideBaseUrlInput()}
            modelOverrideFamilyInput={modelOverrideFamilyInput()}
            modelOverrideReasoningInput={modelOverrideReasoningInput()}
            modelOverrideToolCallInput={modelOverrideToolCallInput()}
            modelOverrideStatusInput={modelOverrideStatusInput()}
            modelOverrideReleaseDateInput={modelOverrideReleaseDateInput()}
            modelOverrideHeadersInput={modelOverrideHeadersInput()}
            modelOverrideOptionsInput={modelOverrideOptionsInput()}
            modelOverrideVariantsInput={modelOverrideVariantsInput()}
            modelOverrideModalitiesInput={modelOverrideModalitiesInput()}
            modelOverrideInterleavedInput={modelOverrideInterleavedInput()}
            modelOverrideCostInput={modelOverrideCostInput()}
            modelOverrideLimitInput={modelOverrideLimitInput()}
            modelOverrideAttachmentInput={modelOverrideAttachmentInput()}
            modelOverrideTemperatureInput={modelOverrideTemperatureInput()}
            modelOverrideExperimentalInput={modelOverrideExperimentalInput()}
            showAdvancedOverrideFields={showAdvancedOverrideFields()}
            onEditNameChange={setEditName}
            onEditBaseUrlChange={setEditBaseUrl}
            onEditProtocolChange={setEditProtocol}
            onSaveSelectedProvider={() => void saveSelectedProvider()}
            onClearSelectedProviderAuth={() => void clearSelectedProviderAuth()}
            onDeleteProvider={() => void removeSelectedProvider()}
            onStartOAuth={(methodIndex, method) => void startOAuth(methodIndex, method)}
            onFinishOAuth={() => void finishOAuth()}
            onOpenOAuthAgain={() => {
              const flow = pendingOAuth();
              if (!flow) return;
              const opened = openOAuthUrl(flow.url);
              if (!opened) {
                setPendingOAuth({
                  ...flow,
                  popupBlocked: true,
                });
                setFeedback({
                  tone: "warn",
                  text: "Authorization page is still blocked. Use the manual link below.",
                });
                return;
              }
              setPendingOAuth({
                ...flow,
                popupBlocked: false,
              });
            }}
            onCancelOAuth={() => {
              setPendingOAuth(null);
              setOAuthCode("");
            }}
            onOAuthCodeChange={setOAuthCode}
            onModelFilterQueryChange={setModelFilterQuery}
            onSelectModelOverrideKey={setSelectedModelOverrideKey}
            onModelOverrideKeyInputChange={setModelOverrideKeyInput}
            onModelOverrideModelInputChange={setModelOverrideModelInput}
            onModelOverrideNameInputChange={setModelOverrideNameInput}
            onModelOverrideBaseUrlInputChange={setModelOverrideBaseUrlInput}
            onModelOverrideFamilyInputChange={setModelOverrideFamilyInput}
            onModelOverrideReasoningInputChange={setModelOverrideReasoningInput}
            onModelOverrideToolCallInputChange={setModelOverrideToolCallInput}
            onModelOverrideStatusInputChange={setModelOverrideStatusInput}
            onModelOverrideReleaseDateInputChange={setModelOverrideReleaseDateInput}
            onModelOverrideHeadersInputChange={setModelOverrideHeadersInput}
            onModelOverrideOptionsInputChange={setModelOverrideOptionsInput}
            onModelOverrideVariantsInputChange={setModelOverrideVariantsInput}
            onModelOverrideModalitiesInputChange={setModelOverrideModalitiesInput}
            onModelOverrideInterleavedInputChange={setModelOverrideInterleavedInput}
            onModelOverrideCostInputChange={setModelOverrideCostInput}
            onModelOverrideLimitInputChange={setModelOverrideLimitInput}
            onModelOverrideAttachmentInputChange={setModelOverrideAttachmentInput}
            onModelOverrideTemperatureInputChange={setModelOverrideTemperatureInput}
            onModelOverrideExperimentalInputChange={setModelOverrideExperimentalInput}
            onToggleAdvancedOverrideFields={() =>
              setShowAdvancedOverrideFields((value) => !value)}
            onSaveModelOverride={() => void saveModelOverride()}
            onDeleteModelOverride={() => void removeModelOverride()}
          />
        )}
      </Show>
    </div>
  );
};
