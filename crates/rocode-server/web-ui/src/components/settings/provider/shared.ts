import type { ManagedModelOverride, ManagedProvider } from "~/api/types";

export type FeedbackTone = "success" | "error" | "warn";
export type ProviderWorkspaceView = "connect" | "providers" | "detail";

export type PendingOAuthFlow = {
  providerId: string;
  methodIndex: number;
  methodLabel: string;
  methodType: string;
  instructions: string;
  url: string;
  popupBlocked: boolean;
};

export type ProviderGroupKey = "connected" | "needs-auth" | "other";

export type ManagedProviderGroup = {
  key: ProviderGroupKey;
  label: string;
  providers: ManagedProvider[];
};

export const NEW_MODEL_OVERRIDE_KEY = "__new_model_override__";

export function statusLabel(status: string): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "needs-auth":
      return "Needs Auth";
    case "saved":
      return "Auth Saved";
    case "configured":
      return "Configured";
    default:
      return status;
  }
}

export function authTypeLabel(authType?: string): string {
  switch (authType) {
    case "api":
      return "API Key";
    case "oauth":
      return "OAuth";
    case "wellknown":
      return "Well-Known";
    default:
      return "None";
  }
}

export function authStateDescription(provider: ManagedProvider): string {
  if (provider.auth_type === "oauth") {
    return provider.connected
      ? "OAuth credential is active and the runtime provider is connected."
      : "OAuth credential is stored. Reauthorize if access has expired.";
  }
  if (provider.auth_type === "api") {
    return provider.connected
      ? "API key is stored and the provider is connected."
      : "API key is stored. Reconnect if you replaced the key.";
  }
  if (provider.auth_type === "wellknown") {
    return "Credential comes from a well-known environment or managed source.";
  }
  if (provider.status === "needs-auth") {
    return "Provider configuration exists, but no credential is currently stored.";
  }
  if (provider.known) {
    return "Use API key connect above, or choose an available auth method below.";
  }
  return "Add a credential to activate this provider.";
}

export function protocolLabel(protocol?: string): string {
  switch (protocol) {
    case "openai":
      return "CloseAI-compatible";
    case "anthropic":
      return "Ethnopic / Messages";
    case "google":
      return "Google";
    case "bedrock":
      return "Bedrock";
    case "vertex":
      return "Vertex";
    case "github-copilot":
      return "GitHub Copilot";
    case "gitlab":
      return "GitLab";
    default:
      return protocol || "Built-in";
  }
}

export function oauthStepLabel(methodType: string): string {
  return methodType === "code"
    ? "Paste the returned code below"
    : "Return here after finishing in the browser";
}

export function boolSummary(
  value?: boolean,
  positive = "Enabled",
  negative = "Disabled",
): string {
  if (value === true) return positive;
  if (value === false) return negative;
  return "Inherit";
}

export function jsonFieldSummary(label: string, value: unknown): string | null {
  if (value == null) return null;
  if (Array.isArray(value)) {
    return `${label}: ${value.length} items`;
  }
  if (typeof value === "object") {
    return `${label}: ${Object.keys(value as Record<string, unknown>).length} keys`;
  }
  if (typeof value === "boolean") {
    return `${label}: ${value ? "enabled" : "disabled"}`;
  }
  return `${label}: set`;
}

export function hasAdvancedOverrideConfig(
  override: ManagedModelOverride | null,
): boolean {
  if (!override) return false;
  return Boolean(
    override.headers ||
      override.options ||
      override.variants ||
      override.modalities ||
      override.interleaved !== undefined ||
      override.cost ||
      override.limit ||
      override.attachment !== undefined ||
      override.temperature !== undefined ||
      override.experimental !== undefined,
  );
}

export function providerGroupKey(status: string): ProviderGroupKey {
  if (status === "connected") return "connected";
  if (status === "needs-auth") return "needs-auth";
  return "other";
}

export function providerGroupLabel(group: ProviderGroupKey): string {
  switch (group) {
    case "connected":
      return "Connected";
    case "needs-auth":
      return "Needs Attention";
    default:
      return "Configured / Saved";
  }
}

export function matchesProviderStatusFilter(
  provider: ManagedProvider,
  filter: string,
): boolean {
  if (filter === "all") return true;
  if (filter === "connected") return provider.status === "connected";
  if (filter === "needs-auth") return provider.status === "needs-auth";
  if (filter === "configured") {
    return provider.status !== "connected" && provider.status !== "needs-auth";
  }
  if (filter === "known") return provider.known;
  if (filter === "custom") return !provider.known;
  return true;
}

export function matchesProviderQuery(
  provider: ManagedProvider,
  query: string,
): boolean {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) return true;
  return [
    provider.id,
    provider.name,
    provider.auth_type,
    provider.protocol,
    provider.base_url,
  ]
    .filter((value): value is string => Boolean(value))
    .some((value) => value.toLowerCase().includes(trimmed));
}
