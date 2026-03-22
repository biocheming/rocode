use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

use crate::oauth::ProviderAuth;
use crate::{ApiError, Result, ServerState};
use rocode_provider::{AuthInfo, AuthMethodType, ModelsData, ModelsDevInfo, ModelsRegistry};

pub(crate) fn provider_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_providers))
        .route("/managed", get(list_managed_providers))
        .route("/known", get(list_known_providers))
        .route("/connect/schema", get(get_provider_connect_schema))
        .route("/connect", post(connect_provider))
        .route("/register", post(register_custom_provider))
        .route("/auth", get(get_provider_auth))
        .route("/{id}", put(update_provider).delete(delete_provider))
        .route("/{id}/oauth/authorize", post(oauth_authorize))
        .route("/{id}/oauth/callback", post(oauth_callback))
}

#[derive(Debug, Serialize)]
pub struct ProviderListResponse {
    pub all: Vec<ProviderInfo>,
    #[serde(rename = "default")]
    pub default_model: HashMap<String, String>,
    pub connected: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ManagedProvidersResponse {
    pub providers: Vec<ManagedProviderInfo>,
}

#[derive(Debug, Serialize)]
pub struct ManagedProviderInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub connected: bool,
    pub has_auth: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_type: Option<String>,
    pub configured: bool,
    pub known: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    pub known_model_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_overrides: Vec<ManagedModelOverrideInfo>,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ManagedModelOverrideInfo {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
}

static MODEL_VARIANT_LOOKUP: OnceCell<HashMap<String, HashMap<String, Vec<String>>>> =
    OnceCell::const_new();

const CONNECT_PROTOCOL_OPTIONS: &[(&str, &str)] = &[
    ("openai", "OpenAI"),
    ("anthropic", "Ethnopic / Messages"),
    ("google", "Google"),
    ("bedrock", "Bedrock"),
    ("vertex", "Vertex"),
    ("github-copilot", "GitHub Copilot"),
    ("gitlab", "GitLab"),
];

fn protocol_to_npm(protocol: &str) -> Option<&'static str> {
    match protocol {
        "openai" => Some("@ai-sdk/openai-compatible"),
        "anthropic" => Some("@ai-sdk/anthropic"),
        "google" => Some("@ai-sdk/google"),
        "bedrock" => Some("@ai-sdk/amazon-bedrock"),
        "vertex" => Some("@ai-sdk/google-vertex"),
        "github-copilot" => Some("@ai-sdk/github-copilot"),
        "gitlab" => Some("@ai-sdk/gitlab"),
        _ => None,
    }
}

fn npm_to_protocol(npm: &str) -> Option<&'static str> {
    match npm {
        "@ai-sdk/openai-compatible" => Some("openai"),
        "@ai-sdk/anthropic" => Some("anthropic"),
        "@ai-sdk/google" => Some("google"),
        "@ai-sdk/amazon-bedrock" => Some("bedrock"),
        "@ai-sdk/google-vertex" => Some("vertex"),
        "@ai-sdk/github-copilot" => Some("github-copilot"),
        "@ai-sdk/gitlab" => Some("gitlab"),
        _ => None,
    }
}

async fn load_models_dev_data() -> ModelsData {
    let cache_path = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rocode")
        .join("models.json");

    if let Ok(content) = tokio::fs::read_to_string(&cache_path).await {
        if let Ok(parsed) = serde_json::from_str::<ModelsData>(&content) {
            return parsed;
        }
    }

    let registry = ModelsRegistry::default();
    tokio::time::timeout(Duration::from_secs(2), registry.get())
        .await
        .unwrap_or_default()
}

fn build_model_variant_lookup(data: ModelsData) -> HashMap<String, HashMap<String, Vec<String>>> {
    data.into_iter()
        .map(|(provider_id, provider)| {
            let model_map = provider
                .models
                .into_iter()
                .map(|(model_id, model)| {
                    let mut variants = model
                        .variants
                        .as_ref()
                        .map(|items| items.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    if variants.is_empty() {
                        variants = synthetic_variant_names(&provider_id, &model);
                    }
                    variants.sort();
                    (model_id, variants)
                })
                .collect::<HashMap<_, _>>();
            (provider_id, model_map)
        })
        .collect()
}

/// Detect whether a provider+model pair uses the ethnopic/messages protocol family.
///
/// This is a **protocol compatibility check**, not a brand reference.  When users
/// configure an Anthropic-compatible provider (directly or via Bedrock/Vertex),
/// the thinking variant surface is `["high", "max"]` rather than the OpenAI-style
/// `["low", "medium", "high"]`.
fn is_ethnopic_protocol_family(provider_id: &str) -> bool {
    let provider = provider_id.to_ascii_lowercase();
    provider.contains("anthropic") || provider.contains("ethnopic")
}

fn synthetic_variant_names(provider_id: &str, model: &ModelsDevInfo) -> Vec<String> {
    if !model.reasoning {
        return Vec::new();
    }

    if is_ethnopic_protocol_family(provider_id) {
        return vec!["high".to_string(), "max".to_string()];
    }

    let provider = provider_id.to_ascii_lowercase();
    let model_id = model.id.to_ascii_lowercase();

    let is_google =
        provider.contains("google") || provider.contains("vertex") || model_id.contains("gemini");
    if is_google {
        return vec!["high".to_string(), "max".to_string()];
    }

    vec!["low".to_string(), "medium".to_string(), "high".to_string()]
}

pub(crate) async fn get_model_variant_lookup(
) -> &'static HashMap<String, HashMap<String, Vec<String>>> {
    MODEL_VARIANT_LOOKUP
        .get_or_init(|| async {
            let data = load_models_dev_data().await;
            build_model_variant_lookup(data)
        })
        .await
}

pub(crate) fn variants_for_model(
    lookup: &HashMap<String, HashMap<String, Vec<String>>>,
    provider_id: &str,
    model_id: &str,
) -> Vec<String> {
    lookup
        .get(provider_id)
        .and_then(|models| models.get(model_id))
        .cloned()
        .unwrap_or_default()
}

async fn list_providers(State(state): State<Arc<ServerState>>) -> Json<ProviderListResponse> {
    let variant_lookup = get_model_variant_lookup().await;
    let models_data = load_models_dev_data().await;

    let providers_guard = state.providers.read().await;
    let connected: std::collections::HashSet<String> = providers_guard
        .list()
        .into_iter()
        .map(|provider| provider.id().to_string())
        .collect();
    let connected_models = providers_guard.list_models();
    drop(providers_guard);

    let mut provider_names: HashMap<String, String> = HashMap::new();
    let mut provider_models: HashMap<String, HashMap<String, ModelInfo>> = HashMap::new();

    let mut upsert_model = |provider_id: &str, model: ModelInfo| {
        provider_models
            .entry(provider_id.to_string())
            .or_default()
            .insert(model.id.clone(), model);
    };

    // 1) models.dev full provider catalogue.
    for (provider_id, provider) in &models_data {
        provider_names
            .entry(provider_id.clone())
            .or_insert_with(|| provider.name.clone());
        for model in provider.models.values() {
            let variants = variants_for_model(variant_lookup, provider_id, &model.id);
            upsert_model(
                provider_id,
                ModelInfo {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    provider: provider_id.clone(),
                    variants,
                },
            );
        }
    }

    // 2) Config-defined providers/models (even if absent from models.dev).
    let config = state.config_store.config();
    if let Some(configured_providers) = &config.provider {
        for (provider_id, provider) in configured_providers {
            provider_names
                .entry(provider_id.clone())
                .or_insert_with(|| provider.name.clone().unwrap_or_else(|| provider_id.clone()));
            if let Some(models) = &provider.models {
                for (configured_model_id, configured) in models {
                    let model_id = configured
                        .model
                        .clone()
                        .unwrap_or_else(|| configured_model_id.clone());
                    let mut variants = configured
                        .variants
                        .as_ref()
                        .map(|items| items.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    if variants.is_empty() {
                        variants = variants_for_model(variant_lookup, provider_id, &model_id);
                    } else {
                        variants.sort();
                    }
                    upsert_model(
                        provider_id,
                        ModelInfo {
                            id: model_id.clone(),
                            name: configured.name.clone().unwrap_or_else(|| model_id.clone()),
                            provider: provider_id.clone(),
                            variants,
                        },
                    );
                }
            }
        }
    }

    // 3) Connected runtime models override names/capabilities-derived variants.
    for model in connected_models {
        let provider_id = model.provider.clone();
        provider_names
            .entry(provider_id.clone())
            .or_insert_with(|| provider_id.clone());
        let variants = variants_for_model(variant_lookup, &provider_id, &model.id);
        upsert_model(
            &provider_id,
            ModelInfo {
                id: model.id,
                name: model.name,
                provider: provider_id.clone(),
                variants,
            },
        );
    }

    for provider_id in provider_names.keys() {
        provider_models.entry(provider_id.clone()).or_default();
    }

    let mut all: Vec<ProviderInfo> = provider_models
        .into_iter()
        .map(|(id, model_map)| {
            let mut models: Vec<ModelInfo> = model_map.into_values().collect();
            models.sort_by(|a, b| a.id.cmp(&b.id));
            ProviderInfo {
                name: provider_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| id.clone()),
                id,
                models,
            }
        })
        .collect();
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let mut connected: Vec<String> = connected.into_iter().collect();
    connected.sort();

    let default_model: HashMap<String, String> = all
        .iter()
        .filter_map(|provider| {
            provider
                .models
                .first()
                .map(|model| (provider.id.clone(), model.id.clone()))
        })
        .collect();

    Json(ProviderListResponse {
        all,
        default_model,
        connected,
    })
}

fn managed_provider_status(connected: bool, configured: bool, has_auth: bool) -> &'static str {
    if connected {
        "connected"
    } else if configured && !has_auth {
        "needs-auth"
    } else if has_auth {
        "saved"
    } else {
        "configured"
    }
}

fn managed_provider_auth_type(auth: Option<&AuthInfo>) -> Option<String> {
    match auth {
        Some(AuthInfo::Api { .. }) => Some("api".to_string()),
        Some(AuthInfo::OAuth { .. }) => Some("oauth".to_string()),
        Some(AuthInfo::WellKnown { .. }) => Some("wellknown".to_string()),
        None => None,
    }
}

async fn list_managed_providers(
    State(state): State<Arc<ServerState>>,
) -> Json<ManagedProvidersResponse> {
    let variant_lookup = get_model_variant_lookup().await;
    let models_data = load_models_dev_data().await;
    let auth_store = state.auth_manager.list().await;
    let config = state.config_store.config();

    let providers_guard = state.providers.read().await;
    let runtime_provider_ids: std::collections::HashSet<String> = providers_guard
        .list()
        .into_iter()
        .map(|provider| provider.id().to_string())
        .collect();
    let runtime_models = providers_guard.list_models();
    drop(providers_guard);

    let mut provider_ids: std::collections::HashSet<String> = auth_store.keys().cloned().collect();
    if let Some(configured_providers) = &config.provider {
        provider_ids.extend(configured_providers.keys().cloned());
    }

    let mut providers = provider_ids
        .into_iter()
        .map(|id| {
            let known = models_data.get(&id);
            let configured = config
                .provider
                .as_ref()
                .and_then(|provider_map| provider_map.get(&id));
            let mut model_map: HashMap<String, ModelInfo> = HashMap::new();

            if let Some(configured_models) =
                configured.and_then(|provider| provider.models.as_ref())
            {
                for (configured_model_id, configured_model) in configured_models {
                    let model_id = configured_model
                        .model
                        .clone()
                        .unwrap_or_else(|| configured_model_id.clone());
                    let mut variants = configured_model
                        .variants
                        .as_ref()
                        .map(|items| items.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    if variants.is_empty() {
                        variants = variants_for_model(variant_lookup, &id, &model_id);
                    } else {
                        variants.sort();
                    }
                    model_map.insert(
                        model_id.clone(),
                        ModelInfo {
                            id: model_id.clone(),
                            name: configured_model
                                .name
                                .clone()
                                .unwrap_or_else(|| model_id.clone()),
                            provider: id.clone(),
                            variants,
                        },
                    );
                }
            }

            for runtime_model in runtime_models.iter().filter(|model| model.provider == id) {
                let variants = variants_for_model(variant_lookup, &id, &runtime_model.id);
                model_map.insert(
                    runtime_model.id.clone(),
                    ModelInfo {
                        id: runtime_model.id.clone(),
                        name: runtime_model.name.clone(),
                        provider: id.clone(),
                        variants,
                    },
                );
            }

            let mut models: Vec<ModelInfo> = model_map.into_values().collect();
            models.sort_by(|a, b| a.id.cmp(&b.id));
            let mut model_overrides = configured
                .and_then(|provider| provider.models.as_ref())
                .map(|configured_models| {
                    configured_models
                        .iter()
                        .map(|(key, configured_model)| ManagedModelOverrideInfo {
                            key: key.clone(),
                            name: configured_model.name.clone(),
                            model: configured_model.model.clone(),
                            base_url: configured_model.base_url.clone(),
                            family: configured_model.family.clone(),
                            reasoning: configured_model.reasoning,
                            tool_call: configured_model.tool_call,
                            headers: configured_model.headers.clone(),
                            options: configured_model
                                .options
                                .as_ref()
                                .map(|value| serde_json::to_value(value).unwrap_or_default()),
                            variants: configured_model
                                .variants
                                .as_ref()
                                .map(|value| serde_json::to_value(value).unwrap_or_default()),
                            modalities: configured_model
                                .modalities
                                .as_ref()
                                .map(|value| serde_json::to_value(value).unwrap_or_default()),
                            interleaved: configured_model.interleaved.clone(),
                            cost: configured_model
                                .cost
                                .as_ref()
                                .map(|value| serde_json::to_value(value).unwrap_or_default()),
                            limit: configured_model
                                .limit
                                .as_ref()
                                .map(|value| serde_json::to_value(value).unwrap_or_default()),
                            attachment: configured_model.attachment,
                            temperature: configured_model.temperature,
                            status: configured_model.status.clone(),
                            release_date: configured_model.release_date.clone(),
                            experimental: configured_model.experimental,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            model_overrides.sort_by(|a, b| a.key.cmp(&b.key));

            let connected = runtime_provider_ids.contains(&id);
            let auth = auth_store.get(&id);
            let has_auth = auth.is_some();
            let configured_flag = configured.is_some();

            ManagedProviderInfo {
                id: id.clone(),
                name: configured
                    .and_then(|provider| provider.name.clone())
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| known.map(|provider| provider.name.clone()))
                    .unwrap_or_else(|| id.clone()),
                status: managed_provider_status(connected, configured_flag, has_auth).to_string(),
                connected,
                has_auth,
                auth_type: managed_provider_auth_type(auth),
                configured: configured_flag,
                known: known.is_some(),
                env: known
                    .map(|provider| provider.env.clone())
                    .unwrap_or_default(),
                known_model_count: known.map(|provider| provider.models.len()).unwrap_or(0),
                base_url: configured.and_then(|provider| provider.base_url.clone()),
                protocol: configured
                    .and_then(|provider| provider.npm.as_deref())
                    .and_then(npm_to_protocol)
                    .map(str::to_string),
                model_overrides,
                models,
            }
        })
        .collect::<Vec<_>>();

    providers.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.has_auth.cmp(&a.has_auth))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Json(ManagedProvidersResponse { providers })
}

async fn known_provider_entries(state: &ServerState) -> Vec<KnownProviderEntry> {
    let models_data = load_models_dev_data().await;
    let connected_ids: std::collections::HashSet<String> = state
        .providers
        .read()
        .await
        .list_models()
        .into_iter()
        .map(|m| m.provider)
        .collect();

    let mut providers: Vec<KnownProviderEntry> = models_data
        .into_iter()
        .map(|(id, info)| KnownProviderEntry {
            connected: connected_ids.contains(&id),
            model_count: info.models.len(),
            env: info.env,
            name: info.name,
            id,
        })
        .collect();
    providers.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    providers
}

/// A lightweight provider entry for the "known providers" catalogue.
#[derive(Debug, Serialize)]
pub struct KnownProviderEntry {
    pub id: String,
    pub name: String,
    pub env: Vec<String>,
    pub model_count: usize,
    pub connected: bool,
}

#[derive(Debug, Serialize)]
pub struct KnownProvidersResponse {
    pub providers: Vec<KnownProviderEntry>,
}

/// Returns all providers known to `models.dev`, regardless of whether they are
/// currently connected.  Each entry includes the primary env var(s) and a flag
/// indicating whether the provider is already connected.
async fn list_known_providers(
    State(state): State<Arc<ServerState>>,
) -> Json<KnownProvidersResponse> {
    let providers = known_provider_entries(state.as_ref()).await;
    Json(KnownProvidersResponse { providers })
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectProtocolOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ProviderConnectSchemaResponse {
    pub providers: Vec<KnownProviderEntry>,
    pub protocols: Vec<ConnectProtocolOption>,
}

async fn get_provider_connect_schema(
    State(state): State<Arc<ServerState>>,
) -> Json<ProviderConnectSchemaResponse> {
    let providers = known_provider_entries(state.as_ref()).await;
    let protocols = CONNECT_PROTOCOL_OPTIONS
        .iter()
        .map(|(id, name)| ConnectProtocolOption {
            id: (*id).to_string(),
            name: (*name).to_string(),
        })
        .collect();
    Json(ProviderConnectSchemaResponse {
        providers,
        protocols,
    })
}

#[derive(Debug, Serialize)]
pub struct AuthMethodInfo {
    pub name: String,
    pub description: String,
}

async fn get_provider_auth(
    State(state): State<Arc<ServerState>>,
) -> Json<HashMap<String, Vec<AuthMethodInfo>>> {
    if let Err(error) = super::plugin_auth::ensure_plugin_loader_active(&state).await {
        tracing::warn!(%error, "failed to warm plugin loader for provider auth list");
    }
    let Some(loader) = super::get_plugin_loader() else {
        return Json(HashMap::new());
    };
    let methods = ProviderAuth::methods(loader).await;
    let result = methods
        .into_iter()
        .map(|(provider, values)| {
            let mapped = values
                .into_iter()
                .map(|method| AuthMethodInfo {
                    name: method.label,
                    description: method.method_type,
                })
                .collect::<Vec<_>>();
            (provider, mapped)
        })
        .collect::<HashMap<_, _>>();
    Json(result)
}

#[derive(Debug, Deserialize)]
pub struct OAuthAuthorizeRequest {
    pub method: usize,
}

#[derive(Debug, Serialize)]
pub struct OAuthAuthorizeResponse {
    pub url: String,
    #[serde(rename = "method")]
    pub method_type: String,
    pub instructions: String,
}

async fn oauth_authorize(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<OAuthAuthorizeRequest>,
) -> Result<Json<OAuthAuthorizeResponse>> {
    let _ = super::plugin_auth::ensure_plugin_loader_active(&state).await?;
    let loader = super::get_plugin_loader()
        .ok_or_else(|| ApiError::NotFound("no plugin loader initialized".to_string()))?;
    let authorization = ProviderAuth::authorize(loader, &id, req.method, None)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    Ok(Json(OAuthAuthorizeResponse {
        url: authorization.url,
        method_type: match authorization.method {
            AuthMethodType::Auto => "auto".to_string(),
            AuthMethodType::Code => "code".to_string(),
        },
        instructions: authorization.instructions,
    }))
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackRequest {
    pub method: usize,
    pub code: Option<String>,
}

async fn oauth_callback(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<OAuthCallbackRequest>,
) -> Result<Json<bool>> {
    let _ = super::plugin_auth::ensure_plugin_loader_active(&state).await?;
    let loader = super::get_plugin_loader()
        .ok_or_else(|| ApiError::NotFound("no plugin loader initialized".to_string()))?;
    ProviderAuth::new(state.auth_manager.clone())
        .callback(loader, &id, req.code.as_deref())
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Refresh auth loader state after callback and apply custom-fetch proxy changes immediately.
    if let Some(bridge) = loader.auth_bridge(&id).await {
        match bridge.load().await {
            Ok(load_result) => {
                crate::server::sync_custom_fetch_proxy(
                    &id,
                    bridge,
                    loader,
                    load_result.has_custom_fetch,
                );
            }
            Err(error) => {
                crate::server::sync_custom_fetch_proxy(&id, bridge, loader, false);
                tracing::warn!(
                    provider = %id,
                    %error,
                    "failed to refresh plugin auth loader after oauth callback"
                );
            }
        }
    }

    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct ConnectProviderRequest {
    pub provider_id: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
}

async fn connect_provider(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ConnectProviderRequest>,
) -> Result<Json<bool>> {
    let provider_id = req.provider_id.trim();
    let api_key = req.api_key.trim();
    if provider_id.is_empty() {
        return Err(ApiError::BadRequest("provider_id is required".to_string()));
    }
    if api_key.is_empty() {
        return Err(ApiError::BadRequest("api_key is required".to_string()));
    }

    match (&req.base_url, &req.protocol) {
        (Some(_), None) | (None, Some(_)) => {
            return Err(ApiError::BadRequest(
                "base_url and protocol must be provided together".to_string(),
            ));
        }
        _ => {}
    }

    if let (Some(base_url), Some(protocol)) = (&req.base_url, &req.protocol) {
        let base_url = base_url.trim();
        let protocol = protocol.trim();
        if base_url.is_empty() {
            return Err(ApiError::BadRequest("base_url is required".to_string()));
        }
        let npm = protocol_to_npm(protocol)
            .ok_or_else(|| ApiError::BadRequest(format!("Invalid protocol: {}", protocol)))?;

        let updated = state
            .config_store
            .replace_with(|config| {
                let providers = config.provider.get_or_insert_with(HashMap::new);
                let provider = providers
                    .entry(provider_id.to_string())
                    .or_insert_with(rocode_config::ProviderConfig::default);
                if provider
                    .name
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                {
                    provider.name = Some(provider_id.to_string());
                }
                provider.id = Some(provider_id.to_string());
                provider.base_url = Some(base_url.to_string());
                provider.npm = Some(npm.to_string());
                Ok(())
            })
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        drop(updated);
    }

    state
        .auth_manager
        .set(
            provider_id,
            rocode_provider::AuthInfo::Api {
                key: api_key.to_string(),
            },
        )
        .await;
    state.rebuild_providers().await;
    crate::session_runtime::events::broadcast_config_updated(state.as_ref());

    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
}

async fn update_provider(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<Json<bool>> {
    let provider_id = id.trim();
    if provider_id.is_empty() {
        return Err(ApiError::BadRequest("provider id is required".to_string()));
    }

    match (&req.base_url, &req.protocol) {
        (Some(_), None) | (None, Some(_)) => {
            return Err(ApiError::BadRequest(
                "base_url and protocol must be provided together".to_string(),
            ));
        }
        _ => {}
    }

    let updated = state
        .config_store
        .replace_with(|config| {
            let providers = config.provider.get_or_insert_with(HashMap::new);
            let provider = providers
                .entry(provider_id.to_string())
                .or_insert_with(rocode_config::ProviderConfig::default);

            if let Some(name) = &req.name {
                let trimmed = name.trim();
                provider.name = (!trimmed.is_empty()).then_some(trimmed.to_string());
            }

            if let (Some(base_url), Some(protocol)) = (&req.base_url, &req.protocol) {
                let base_url = base_url.trim();
                let protocol = protocol.trim();
                if base_url.is_empty() {
                    return Err(anyhow::anyhow!("base_url is required"));
                }
                let npm = protocol_to_npm(protocol)
                    .ok_or_else(|| anyhow::anyhow!("Invalid protocol: {}", protocol))?;
                provider.id = Some(provider_id.to_string());
                provider.base_url = Some(base_url.to_string());
                provider.npm = Some(npm.to_string());
            }

            Ok(())
        })
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    drop(updated);

    state.rebuild_providers().await;
    crate::session_runtime::events::broadcast_config_updated(state.as_ref());
    Ok(Json(true))
}

async fn delete_provider(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<bool>> {
    let provider_id = id.trim();
    if provider_id.is_empty() {
        return Err(ApiError::BadRequest("provider id is required".to_string()));
    }

    let updated = state
        .config_store
        .replace_with(|config| {
            if let Some(providers) = config.provider.as_mut() {
                providers.remove(provider_id);
            }
            Ok(())
        })
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    drop(updated);

    state.auth_manager.remove(provider_id).await;
    state.rebuild_providers().await;
    crate::session_runtime::events::broadcast_config_updated(state.as_ref());
    Ok(Json(true))
}

async fn register_custom_provider(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ConnectProviderRequest>,
) -> Result<Json<bool>> {
    connect_provider(State(state), Json(req)).await
}
