use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;

use crate::oauth::ProviderAuth;
use crate::{ApiError, Result, ServerState};
use rocode_provider::{AuthMethodType, ModelsData, ModelsDevInfo, ModelsRegistry};

pub(crate) fn provider_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_providers))
        .route("/known", get(list_known_providers))
        .route("/register", post(register_custom_provider))
        .route("/auth", get(get_provider_auth))
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
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
}

static MODEL_VARIANT_LOOKUP: OnceCell<HashMap<String, HashMap<String, Vec<String>>>> =
    OnceCell::const_new();

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
fn is_ethnopic_protocol_family(provider_id: &str, _model_id: &str) -> bool {
    let provider = provider_id.to_ascii_lowercase();
    provider.contains("anthropic")
        || provider.contains("ethnopic")
}

fn synthetic_variant_names(provider_id: &str, model: &ModelsDevInfo) -> Vec<String> {
    if !model.reasoning {
        return Vec::new();
    }

    if is_ethnopic_protocol_family(provider_id, &model.id) {
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
    for (provider_id, provider) in &state.bootstrap_config.providers {
        provider_names
            .entry(provider_id.clone())
            .or_insert_with(|| provider.name.clone().unwrap_or_else(|| provider_id.clone()));
        if let Some(models) = &provider.models {
            for (configured_model_id, configured) in models {
                let model_id = configured
                    .id
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
    Json(KnownProvidersResponse { providers })
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
pub struct RegisterCustomProviderRequest {
    pub provider_id: String,
    pub base_url: String,
    pub protocol: String,
    pub api_key: String,
}

async fn register_custom_provider(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RegisterCustomProviderRequest>,
) -> Result<Json<bool>> {
    // Validate protocol
    let valid_protocols = ["openai", "anthropic", "google", "bedrock", "vertex", "github-copilot", "gitlab"];
    if !valid_protocols.contains(&req.protocol.as_str()) {
        return Err(ApiError::BadRequest(format!("Invalid protocol: {}", req.protocol)));
    }

    // Store auth
    state.auth_manager.set(&req.provider_id, rocode_provider::AuthInfo::Api { key: req.api_key }).await;

    // Store custom provider config
    {
        let mut custom = state.custom_providers.write().await;
        custom.insert(req.provider_id.clone(), crate::server::CustomProviderConfig {
            base_url: req.base_url,
            protocol: req.protocol,
        });
    }

    // Rebuild providers to include the new custom provider
    state.rebuild_providers().await;

    Ok(Json(true))
}
