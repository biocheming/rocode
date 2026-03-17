use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use rocode_orchestrator::SchedulerConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

use crate::session_runtime::events::broadcast_config_updated;
use crate::{Result, ServerState};
use rocode_config::{
    Config as AppConfig, McpServerConfig, ModelConfig, PluginConfig, ProviderConfig,
};

pub(crate) fn config_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(get_config).patch(patch_config))
        .route("/providers", get(get_config_providers))
        .route(
            "/provider/{key}",
            put(put_provider_config).delete(delete_provider_config),
        )
        .route(
            "/provider/{key}/models/{model_key}",
            put(put_provider_model_config).delete(delete_provider_model_config),
        )
        .route(
            "/plugin/{key}",
            put(put_plugin_config).delete(delete_plugin_config),
        )
        .route("/mcp/{key}", put(put_mcp_config).delete(delete_mcp_config))
        .route(
            "/scheduler",
            get(get_scheduler_config).put(put_scheduler_config),
        )
}

async fn get_config(State(state): State<Arc<ServerState>>) -> Result<Json<AppConfig>> {
    let config = state.config_store.config();
    Ok(Json((*config).clone()))
}

async fn patch_config(
    State(state): State<Arc<ServerState>>,
    Json(patch): Json<serde_json::Value>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .patch(patch)
        .map_err(|e| crate::ApiError::BadRequest(e.to_string()))?;
    state.config_store.invalidate_plugin_cache().await;
    broadcast_config_updated(state.as_ref());
    // Invalidate mode caches so next request rebuilds with new config
    *crate::routes::AGENT_LIST_CACHE.write().await = None;
    *crate::routes::MODE_LIST_CACHE.write().await = None;
    Ok(Json((*updated).clone()))
}

async fn finalize_config_change(
    state: &ServerState,
    updated: Arc<AppConfig>,
) -> Result<Json<AppConfig>> {
    state.config_store.invalidate_plugin_cache().await;
    broadcast_config_updated(state);
    *crate::routes::AGENT_LIST_CACHE.write().await = None;
    *crate::routes::MODE_LIST_CACHE.write().await = None;
    Ok(Json((*updated).clone()))
}

#[derive(Debug, Serialize)]
pub struct ConfigProvidersResponse {
    pub providers: Vec<crate::routes::provider::ProviderInfo>,
    #[serde(rename = "default")]
    pub default_model: HashMap<String, String>,
}

async fn get_config_providers(
    State(state): State<Arc<ServerState>>,
) -> Json<ConfigProvidersResponse> {
    let variant_lookup = crate::routes::provider::get_model_variant_lookup().await;
    let models = state.providers.read().await.list_models();
    let mut provider_map: HashMap<String, Vec<crate::routes::provider::ModelInfo>> = HashMap::new();
    let mut provider_names: HashMap<String, String> = HashMap::new();
    for m in models {
        let provider_id = m.provider.clone();
        let model_id = m.id.clone();
        provider_names
            .entry(provider_id.clone())
            .or_insert_with(|| provider_id.clone());
        let variants =
            crate::routes::provider::variants_for_model(variant_lookup, &provider_id, &model_id);
        provider_map.entry(provider_id.clone()).or_default().push(
            crate::routes::provider::ModelInfo {
                id: model_id,
                name: m.name,
                provider: provider_id,
                variants,
            },
        );
    }
    let config = state.config_store.config();
    if let Some(configured_providers) = &config.provider {
        for (provider_id, provider) in configured_providers {
            provider_names
                .entry(provider_id.clone())
                .or_insert_with(|| provider.name.clone().unwrap_or_else(|| provider_id.clone()));
            let entries = provider_map.entry(provider_id.clone()).or_default();
            let mut existing: HashMap<String, usize> = entries
                .iter()
                .enumerate()
                .map(|(idx, model)| (model.id.clone(), idx))
                .collect();
            if let Some(models) = &provider.models {
                for (configured_model_key, configured_model) in models {
                    let model_id = configured_model
                        .model
                        .clone()
                        .unwrap_or_else(|| configured_model_key.clone());
                    let variants = configured_model
                        .variants
                        .as_ref()
                        .map(|items| items.keys().cloned().collect::<Vec<_>>())
                        .filter(|items| !items.is_empty())
                        .unwrap_or_else(|| {
                            crate::routes::provider::variants_for_model(
                                variant_lookup,
                                provider_id,
                                &model_id,
                            )
                        });
                    let info = crate::routes::provider::ModelInfo {
                        id: model_id.clone(),
                        name: configured_model
                            .name
                            .clone()
                            .unwrap_or_else(|| model_id.clone()),
                        provider: provider_id.clone(),
                        variants,
                    };
                    if let Some(index) = existing.get(&model_id).copied() {
                        entries[index] = info;
                    } else {
                        existing.insert(model_id.clone(), entries.len());
                        entries.push(info);
                    }
                }
            }
        }
    }
    let providers: Vec<crate::routes::provider::ProviderInfo> = provider_map
        .into_iter()
        .map(|(id, models)| crate::routes::provider::ProviderInfo {
            id: id.clone(),
            name: provider_names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| id.clone()),
            models,
        })
        .collect();
    let default_model: HashMap<String, String> = providers
        .iter()
        .filter_map(|p| p.models.first().map(|m| (p.id.clone(), m.id.clone())))
        .collect();
    Json(ConfigProvidersResponse {
        providers,
        default_model,
    })
}

async fn put_provider_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(provider): Json<ProviderConfig>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let provider_map = config.provider.get_or_insert_with(HashMap::new);
            provider_map.insert(key.clone(), provider);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn delete_provider_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let provider_map = config.provider.get_or_insert_with(HashMap::new);
            provider_map.remove(&key);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn put_provider_model_config(
    State(state): State<Arc<ServerState>>,
    Path((key, model_key)): Path<(String, String)>,
    Json(model): Json<ModelConfig>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let provider_map = config.provider.get_or_insert_with(HashMap::new);
            let provider = provider_map
                .entry(key.clone())
                .or_insert_with(ProviderConfig::default);
            let models = provider.models.get_or_insert_with(HashMap::new);
            models.insert(model_key.clone(), model);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn delete_provider_model_config(
    State(state): State<Arc<ServerState>>,
    Path((key, model_key)): Path<(String, String)>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let provider_map = config.provider.get_or_insert_with(HashMap::new);
            if let Some(provider) = provider_map.get_mut(&key) {
                if let Some(models) = provider.models.as_mut() {
                    models.remove(&model_key);
                }
            }
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn put_plugin_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(plugin): Json<PluginConfig>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            config.plugin.insert(key.clone(), plugin);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn delete_plugin_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            config.plugin.remove(&key);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn put_mcp_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
    Json(mcp): Json<McpServerConfig>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let mcp_map = config.mcp.get_or_insert_with(HashMap::new);
            mcp_map.insert(key.clone(), mcp);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

async fn delete_mcp_config(
    State(state): State<Arc<ServerState>>,
    Path(key): Path<String>,
) -> Result<Json<AppConfig>> {
    let updated = state
        .config_store
        .replace_with(|config| {
            let mcp_map = config.mcp.get_or_insert_with(HashMap::new);
            mcp_map.remove(&key);
            Ok(())
        })
        .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    finalize_config_change(&state, updated).await
}

#[derive(Debug, Serialize)]
pub struct SchedulerConfigResponse {
    #[serde(rename = "path")]
    pub raw_path: Option<String>,
    #[serde(rename = "resolvedPath")]
    pub resolved_path: Option<String>,
    pub exists: bool,
    pub content: String,
    #[serde(rename = "defaultProfile")]
    pub default_profile: Option<String>,
    pub profiles: Vec<SchedulerProfileSummary>,
    #[serde(rename = "parseError")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SchedulerProfileSummary {
    pub key: String,
    pub orchestrator: Option<String>,
    pub description: Option<String>,
    pub stages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PutSchedulerConfigRequest {
    #[serde(default)]
    path: Option<String>,
    content: String,
}

fn summarize_scheduler_profiles(
    content: &str,
) -> (Option<String>, Vec<SchedulerProfileSummary>, Option<String>) {
    match SchedulerConfig::load_from_str(content) {
        Ok(config) => {
            let mut profiles = config
                .profiles
                .into_iter()
                .map(|(key, profile)| SchedulerProfileSummary {
                    key,
                    orchestrator: profile.orchestrator,
                    description: profile.description,
                    stages: profile
                        .stages
                        .into_iter()
                        .map(|stage| stage.kind().event_name().to_string())
                        .collect(),
                })
                .collect::<Vec<_>>();
            profiles.sort_by(|a, b| a.key.cmp(&b.key));
            (
                config
                    .defaults
                    .and_then(|defaults| defaults.profile)
                    .filter(|value| !value.trim().is_empty()),
                profiles,
                None,
            )
        }
        Err(error) => (None, Vec::new(), Some(error.to_string())),
    }
}

async fn scheduler_config_response(
    state: &Arc<ServerState>,
) -> Result<Json<SchedulerConfigResponse>> {
    let config = state.config_store.config();
    let raw_path = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let resolved_path = state.config_store.resolved_scheduler_path().await;

    let (exists, content) = match resolved_path.as_ref() {
        Some(path) => match fs::read_to_string(path).await {
            Ok(content) => (true, content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, String::new()),
            Err(error) => {
                return Err(crate::ApiError::InternalError(format!(
                    "failed to read scheduler config: {error}"
                )));
            }
        },
        None => (false, String::new()),
    };

    let (default_profile, profiles, parse_error) = if content.trim().is_empty() {
        (None, Vec::new(), None)
    } else {
        summarize_scheduler_profiles(&content)
    };

    Ok(Json(SchedulerConfigResponse {
        raw_path,
        resolved_path: resolved_path.map(|path| path.display().to_string()),
        exists,
        content,
        default_profile,
        profiles,
        parse_error,
    }))
}

async fn get_scheduler_config(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<SchedulerConfigResponse>> {
    scheduler_config_response(&state).await
}

fn resolve_scheduler_write_target(
    state: &Arc<ServerState>,
    requested_path: Option<String>,
) -> Result<(String, PathBuf)> {
    let raw_path = requested_path
        .or_else(|| state.config_store.config().scheduler_path.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".rocode/scheduler.jsonc".to_string());

    let path = PathBuf::from(&raw_path);
    if path.is_absolute() {
        return Ok((raw_path, path));
    }

    let project_dir = state.config_store.project_dir().ok_or_else(|| {
        crate::ApiError::BadRequest("scheduler config requires a project directory".to_string())
    })?;
    Ok((raw_path, project_dir.join(path)))
}

async fn put_scheduler_config(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<PutSchedulerConfigRequest>,
) -> Result<Json<SchedulerConfigResponse>> {
    let (raw_path, resolved_path) = resolve_scheduler_write_target(&state, request.path)?;

    if let Some(parent) = resolved_path.parent() {
        fs::create_dir_all(parent).await.map_err(|error| {
            crate::ApiError::InternalError(format!(
                "failed to create scheduler config directory: {error}"
            ))
        })?;
    }

    fs::write(&resolved_path, &request.content)
        .await
        .map_err(|error| {
            crate::ApiError::InternalError(format!("failed to write scheduler config: {error}"))
        })?;

    if state.config_store.config().scheduler_path.as_deref() != Some(raw_path.as_str()) {
        state
            .config_store
            .patch(serde_json::json!({ "schedulerPath": raw_path }))
            .map_err(|error| crate::ApiError::BadRequest(error.to_string()))?;
    }

    state.config_store.invalidate_plugin_cache().await;
    broadcast_config_updated(state.as_ref());
    *crate::routes::AGENT_LIST_CACHE.write().await = None;
    *crate::routes::MODE_LIST_CACHE.write().await = None;

    scheduler_config_response(&state).await
}
