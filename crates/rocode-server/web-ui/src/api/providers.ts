import { apiJson } from "~/api/client";
import type {
  ConnectProviderRequest,
  ManagedProvidersResponse,
  OAuthAuthorizeResponse,
  ProviderAuthMethodsResponse,
  ProviderConnectSchemaResponse,
  UpdateProviderModelRequest,
  UpdateProviderRequest,
} from "~/api/types";

export async function getProviderConnectSchema(): Promise<ProviderConnectSchemaResponse> {
  return apiJson<ProviderConnectSchemaResponse>("/provider/connect/schema");
}

export async function connectProvider(request: ConnectProviderRequest): Promise<boolean> {
  return apiJson<boolean>("/provider/connect", {
    method: "POST",
    body: JSON.stringify(request),
  });
}

export async function getManagedProviders(): Promise<ManagedProvidersResponse> {
  return apiJson<ManagedProvidersResponse>("/provider/managed");
}

export async function getProviderAuthMethods(): Promise<ProviderAuthMethodsResponse> {
  return apiJson<ProviderAuthMethodsResponse>("/provider/auth");
}

export async function updateProvider(
  providerId: string,
  request: UpdateProviderRequest,
): Promise<boolean> {
  return apiJson<boolean>(`/provider/${providerId}`, {
    method: "PUT",
    body: JSON.stringify(request),
  });
}

export async function deleteProvider(providerId: string): Promise<boolean> {
  return apiJson<boolean>(`/provider/${providerId}`, {
    method: "DELETE",
  });
}

export async function updateProviderModel(
  providerId: string,
  modelKey: string,
  request: UpdateProviderModelRequest,
): Promise<boolean> {
  return apiJson<unknown>(`/config/provider/${providerId}/models/${modelKey}`, {
    method: "PUT",
    body: JSON.stringify(request),
  }).then(() => true);
}

export async function deleteProviderModel(
  providerId: string,
  modelKey: string,
): Promise<boolean> {
  return apiJson<unknown>(`/config/provider/${providerId}/models/${modelKey}`, {
    method: "DELETE",
  }).then(() => true);
}

export async function clearProviderAuth(providerId: string): Promise<boolean> {
  return apiJson<{ deleted?: boolean }>(`/auth/${providerId}`, {
    method: "DELETE",
  }).then((response) => response.deleted ?? true);
}

export async function authorizeProviderOAuth(
  providerId: string,
  method: number,
): Promise<OAuthAuthorizeResponse> {
  return apiJson<OAuthAuthorizeResponse>(`/provider/${providerId}/oauth/authorize`, {
    method: "POST",
    body: JSON.stringify({ method }),
  });
}

export async function completeProviderOAuth(
  providerId: string,
  method: number,
  code?: string,
): Promise<boolean> {
  return apiJson<boolean>(`/provider/${providerId}/oauth/callback`, {
    method: "POST",
    body: JSON.stringify({ method, code }),
  });
}
