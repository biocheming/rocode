use crate::auth::AuthInfo;
use crate::azure::AzureProvider;
use crate::instance::ProviderInstance;
use crate::models::{ModelInfo, ModelInterleaved, ModelsData, ProviderInfo as ModelsProviderInfo};
use crate::protocol::{Protocol, ProviderConfig};
use crate::protocol_loader::{ProtocolLoader, ProtocolManifest};
use crate::protocol_validator::ProtocolValidator;
use crate::protocols::create_protocol_impl;
use crate::provider::{
    ModelInfo as RuntimeModelInfo, Provider as RuntimeProvider, ProviderRegistry,
};
use crate::runtime::{Pipeline, ProtocolSource, ProviderRuntime, RuntimeConfig, RuntimeContext};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Arc;
use std::time::Instant;
use tracing;

// ---------------------------------------------------------------------------
// Error types matching TS ModelNotFoundError and InitError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("Model not found: provider={provider_id} model={model_id}")]
    ModelNotFound {
        provider_id: String,
        model_id: String,
        suggestions: Vec<String>,
    },

    #[error("Provider initialization failed: {provider_id}")]
    InitError {
        provider_id: String,
        #[source]
        cause: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

// ---------------------------------------------------------------------------
// BUNDLED_PROVIDERS map (TS npm package -> provider name)
// ---------------------------------------------------------------------------

/// Map of bundled SDK package names to their provider identifiers.
/// Mirrors the TS `BUNDLED_PROVIDERS` record.
/* PLACEHOLDER_BUNDLED_PROVIDERS */
pub static BUNDLED_PROVIDERS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("@ai-sdk/amazon-bedrock", "amazon-bedrock");
    m.insert("@ai-sdk/anthropic", "ethnopic");
    m.insert("@ai-sdk/azure", "azure");
    m.insert("@ai-sdk/google", "google");
    m.insert("@ai-sdk/google-vertex", "google-vertex");
    m.insert("@ai-sdk/google-vertex/anthropic", "google-vertex-ethnopic");
    m.insert("@ai-sdk/openai", "openai");
    m.insert("@ai-sdk/openai-compatible", "openai-compatible");
    m.insert("@openrouter/ai-sdk-provider", "openrouter");
    m.insert("@ai-sdk/xai", "xai");
    m.insert("@ai-sdk/mistral", "mistral");
    m.insert("@ai-sdk/groq", "groq");
    m.insert("@ai-sdk/deepinfra", "deepinfra");
    m.insert("@ai-sdk/cerebras", "cerebras");
    m.insert("@ai-sdk/cohere", "cohere");
    m.insert("@ai-sdk/gateway", "gateway");
    m.insert("@ai-sdk/togetherai", "togetherai");
    m.insert("@ai-sdk/perplexity", "perplexity");
    m.insert("@ai-sdk/vercel", "vercel");
    m.insert("@gitlab/gitlab-ai-provider", "gitlab");
    m.insert("@ai-sdk/github-copilot", "github-copilot");
    m
});

// ---------------------------------------------------------------------------
// Helper functions matching TS helpers
// ---------------------------------------------------------------------------

/// Check if a model ID represents GPT-5 or later.
pub fn is_gpt5_or_later(model_id: &str) -> bool {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^gpt-(\d+)").unwrap());
    if let Some(caps) = RE.captures(model_id) {
        if let Some(num) = caps.get(1) {
            if let Ok(n) = num.as_str().parse::<u32>() {
                return n >= 5;
            }
        }
    }
    false
}

/// Determine whether to use the Copilot responses API for a given model.
pub fn should_use_copilot_responses_api(model_id: &str) -> bool {
    is_gpt5_or_later(model_id) && !model_id.starts_with("gpt-5-mini")
}

// ---------------------------------------------------------------------------
// Provider.Model - the runtime model type (matches TS Provider.Model)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub temperature: bool,
    pub reasoning: bool,
    pub attachment: bool,
    pub toolcall: bool,
    pub input: ModalitySet,
    pub output: ModalitySet,
    pub interleaved: InterleavedConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModalitySet {
    pub text: bool,
    pub audio: bool,
    pub image: bool,
    pub video: bool,
    pub pdf: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InterleavedConfig {
    Bool(bool),
    Field { field: String },
}

impl Default for InterleavedConfig {
    fn default() -> Self {
        InterleavedConfig::Bool(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCostCache {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCostOver200K {
    pub input: f64,
    pub output: f64,
    pub cache: ModelCostCache,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelCost {
    pub input: f64,
    pub output: f64,
    pub cache: ModelCostCache,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental_over_200k: Option<ModelCostOver200K>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelLimit {
    pub context: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelApi {
    pub id: String,
    pub url: String,
    pub npm: String,
}

/// Runtime model type matching TS `Provider.Model`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModel {
    pub id: String,
    pub provider_id: String,
    pub api: ProviderModelApi,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub capabilities: ModelCapabilities,
    pub cost: ProviderModelCost,
    pub limit: ProviderModelLimit,
    pub status: String,
    pub options: HashMap<String, serde_json::Value>,
    pub headers: HashMap<String, String>,
    pub release_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
}

// ---------------------------------------------------------------------------
// Provider.Info - the runtime provider type (matches TS Provider.Info)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderState {
    pub id: String,
    pub name: String,
    pub source: String, // "env" | "config" | "custom" | "api"
    pub env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub options: HashMap<String, serde_json::Value>,
    pub models: HashMap<String, ProviderModel>,
}

// ---------------------------------------------------------------------------
// CustomLoaderResult - result from a custom loader (matches TS CustomLoader return)
// ---------------------------------------------------------------------------

/// Result of a custom loader operation.
#[derive(Default)]
pub struct CustomLoaderResult {
    /// Whether the provider should be auto-loaded even without env/auth keys.
    pub autoload: bool,
    /// Options to merge into the provider.
    pub options: HashMap<String, serde_json::Value>,
    /// Whether this loader provides a custom getModel function.
    pub has_custom_get_model: bool,
    /// Models to add/override (legacy, kept for backward compat).
    pub models: HashMap<String, ModelInfo>,
    /// Headers to apply to all models (legacy).
    pub headers: HashMap<String, String>,
    /// Models to remove by ID pattern (legacy).
    pub blacklist: Vec<String>,
}

/// Trait for provider-specific model loading customization.
pub trait CustomLoader: Send + Sync {
    fn load(
        &self,
        provider: &ModelsProviderInfo,
        provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult;
}

// ---------------------------------------------------------------------------
// Custom loader implementations for all 14+ providers
// ---------------------------------------------------------------------------

/// OpenCode custom loader - checks API keys, filters paid models if no key.
struct OpenCodeLoader;

impl CustomLoader for OpenCodeLoader {
    fn load(
        &self,
        provider: &ModelsProviderInfo,
        provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let has_key = provider.env.iter().any(|e| std::env::var(e).is_ok())
            || provider_state
                .and_then(|state| provider_option_string(state, &["apiKey", "api_key", "apikey"]))
                .is_some();

        if !has_key {
            // Remove paid models (cost.input > 0)
            let paid_ids: Vec<String> = provider
                .models
                .iter()
                .filter(|(_, m)| m.cost.as_ref().map(|c| c.input > 0.0).unwrap_or(false))
                .map(|(id, _)| id.clone())
                .collect();
            for id in &paid_ids {
                result.blacklist.push(id.clone());
            }
        }

        let remaining = provider.models.len().saturating_sub(result.blacklist.len());
        result.autoload = remaining > 0;

        if !has_key {
            result.options.insert(
                "apiKey".to_string(),
                serde_json::Value::String("public".to_string()),
            );
        }

        result
    }
}

/// OpenAI custom loader - uses responses API.
struct OpenAILoader;

impl CustomLoader for OpenAILoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult {
            has_custom_get_model: true,
            ..Default::default()
        };
        // Blacklist non-chat models
        result.blacklist.extend(vec![
            "whisper".to_string(),
            "tts".to_string(),
            "dall-e".to_string(),
            "embedding".to_string(),
            "moderation".to_string(),
        ]);
        result
    }
}

/// GitHub Copilot custom loader - conditional responses vs chat API.
struct GitHubCopilotLoader;

impl CustomLoader for GitHubCopilotLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        CustomLoaderResult {
            has_custom_get_model: true,
            ..Default::default()
        }
    }
}

/// GitHub Copilot Enterprise custom loader - same as GitHub Copilot.
struct GitHubCopilotEnterpriseLoader;

impl CustomLoader for GitHubCopilotEnterpriseLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        CustomLoaderResult {
            has_custom_get_model: true,
            ..Default::default()
        }
    }
}

/// Azure custom loader - conditional getModel based on useCompletionUrls.
struct AzureLoader;

impl CustomLoader for AzureLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        CustomLoaderResult {
            has_custom_get_model: true,
            ..Default::default()
        }
    }
}

/// Azure Cognitive Services custom loader - resource name handling.
struct AzureCognitiveServicesLoader;

impl CustomLoader for AzureCognitiveServicesLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult {
            has_custom_get_model: true,
            ..Default::default()
        };

        if let Ok(resource_name) = std::env::var("AZURE_COGNITIVE_SERVICES_RESOURCE_NAME") {
            result.options.insert(
                "baseURL".to_string(),
                serde_json::Value::String(format!(
                    "https://{}.cognitiveservices.azure.com/openai",
                    resource_name
                )),
            );
        }

        result
    }
}

/// Amazon Bedrock custom loader - the most complex loader.
/// Handles region resolution, AWS credential chain, cross-region model prefixing.
struct AmazonBedrockLoader;

impl AmazonBedrockLoader {
    fn provider_option_string(state: Option<&ProviderState>, keys: &[&str]) -> Option<String> {
        let state = state?;
        for key in keys {
            let Some(value) = options_get_insensitive(&state.options, key) else {
                continue;
            };
            match value {
                serde_json::Value::String(s) if !s.trim().is_empty() => return Some(s.clone()),
                serde_json::Value::Number(n) => return Some(n.to_string()),
                serde_json::Value::Bool(b) => return Some(b.to_string()),
                _ => {}
            }
        }
        None
    }
}

impl CustomLoader for AmazonBedrockLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        // Region precedence: config options > env var > default.
        let region = Self::provider_option_string(provider_state, &["region"])
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-east-1".to_string());

        // Credential options from config or environment.
        let profile = Self::provider_option_string(provider_state, &["profile"])
            .or_else(|| std::env::var("AWS_PROFILE").ok());
        let endpoint = Self::provider_option_string(
            provider_state,
            &["endpoint", "endpointUrl", "endpointURL"],
        );

        let aws_access_key_id = Self::provider_option_string(provider_state, &["accessKeyId"])
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok());
        let aws_secret_access_key =
            Self::provider_option_string(provider_state, &["secretAccessKey"])
                .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok());
        let aws_bearer_token =
            Self::provider_option_string(provider_state, &["awsBearerTokenBedrock", "bearerToken"])
                .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok());
        let aws_web_identity_token_file =
            Self::provider_option_string(provider_state, &["webIdentityTokenFile"])
                .or_else(|| std::env::var("AWS_WEB_IDENTITY_TOKEN_FILE").ok());
        let container_creds = std::env::var("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_ok()
            || std::env::var("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_ok();

        if profile.is_none()
            && aws_access_key_id.is_none()
            && aws_secret_access_key.is_none()
            && aws_bearer_token.is_none()
            && aws_web_identity_token_file.is_none()
            && !container_creds
        {
            result.autoload = false;
            return result;
        }

        result.autoload = true;
        result
            .options
            .insert("region".to_string(), serde_json::Value::String(region));
        if let Some(profile) = profile {
            result
                .options
                .insert("profile".to_string(), serde_json::Value::String(profile));
        }
        if let Some(endpoint) = endpoint {
            result
                .options
                .insert("endpoint".to_string(), serde_json::Value::String(endpoint));
        }
        result.has_custom_get_model = true;

        result
    }
}

/// OpenRouter custom loader - adds custom headers.
struct OpenRouterLoader;

impl CustomLoader for OpenRouterLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();
        result.headers.insert(
            "HTTP-Referer".to_string(),
            "https://opencode.ai/".to_string(),
        );
        result
            .headers
            .insert("X-Title".to_string(), "opencode".to_string());
        result
    }
}

/// ZenMux custom loader - same branding headers as OpenRouter.
struct ZenMuxLoader;

impl CustomLoader for ZenMuxLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();
        result.headers.insert(
            "HTTP-Referer".to_string(),
            "https://opencode.ai/".to_string(),
        );
        result
            .headers
            .insert("X-Title".to_string(), "opencode".to_string());
        result
    }
}

/// Vercel custom loader - adds custom headers.
struct VercelLoader;

impl CustomLoader for VercelLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();
        result.headers.insert(
            "http-referer".to_string(),
            "https://opencode.ai/".to_string(),
        );
        result
            .headers
            .insert("x-title".to_string(), "opencode".to_string());
        result
    }
}

/// Google Vertex custom loader - project/location env var resolution.
struct GoogleVertexLoader;

impl CustomLoader for GoogleVertexLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let project = std::env::var("GOOGLE_CLOUD_PROJECT")
            .or_else(|_| std::env::var("GCP_PROJECT"))
            .or_else(|_| std::env::var("GCLOUD_PROJECT"))
            .ok();
        let location = std::env::var("GOOGLE_CLOUD_LOCATION")
            .or_else(|_| std::env::var("VERTEX_LOCATION"))
            .unwrap_or_else(|_| "us-east5".to_string());

        if let Some(ref proj) = project {
            result.autoload = true;
            result.options.insert(
                "project".to_string(),
                serde_json::Value::String(proj.clone()),
            );
            result
                .options
                .insert("location".to_string(), serde_json::Value::String(location));
            result.has_custom_get_model = true;
        }

        result
    }
}

/// Google Vertex Ethnopic custom loader - similar to google-vertex.
struct GoogleVertexEthnopicLoader;

impl CustomLoader for GoogleVertexEthnopicLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let project = std::env::var("GOOGLE_CLOUD_PROJECT")
            .or_else(|_| std::env::var("GCP_PROJECT"))
            .or_else(|_| std::env::var("GCLOUD_PROJECT"))
            .ok();
        let location = std::env::var("GOOGLE_CLOUD_LOCATION")
            .or_else(|_| std::env::var("VERTEX_LOCATION"))
            .unwrap_or_else(|_| "global".to_string());

        if let Some(ref proj) = project {
            result.autoload = true;
            result.options.insert(
                "project".to_string(),
                serde_json::Value::String(proj.clone()),
            );
            result
                .options
                .insert("location".to_string(), serde_json::Value::String(location));
            result.has_custom_get_model = true;
        }

        result
    }
}

/// SAP AI Core custom loader - service key and deployment ID.
struct SapAiCoreLoader;

impl CustomLoader for SapAiCoreLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let env_service_key = std::env::var("AICORE_SERVICE_KEY").ok();
        result.autoload = env_service_key.is_some();

        if env_service_key.is_some() {
            if let Ok(deployment_id) = std::env::var("AICORE_DEPLOYMENT_ID") {
                result.options.insert(
                    "deploymentId".to_string(),
                    serde_json::Value::String(deployment_id),
                );
            }
            if let Ok(resource_group) = std::env::var("AICORE_RESOURCE_GROUP") {
                result.options.insert(
                    "resourceGroup".to_string(),
                    serde_json::Value::String(resource_group),
                );
            }
        }
        result.has_custom_get_model = true;

        result
    }
}

/// GitLab custom loader - instance URL, auth type, User-Agent, feature flags.
struct GitLabLoader;

impl CustomLoader for GitLabLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let instance_url = std::env::var("GITLAB_INSTANCE_URL")
            .unwrap_or_else(|_| "https://gitlab.com".to_string());
        let api_key = std::env::var("GITLAB_TOKEN").ok();

        result.autoload = api_key.is_some();

        result.options.insert(
            "instanceUrl".to_string(),
            serde_json::Value::String(instance_url),
        );
        if let Some(key) = api_key {
            result
                .options
                .insert("apiKey".to_string(), serde_json::Value::String(key));
        }

        // User-Agent header
        let user_agent = format!(
            "opencode/0.1.0 gitlab-ai-provider/0.1.0 ({} {}; {})",
            std::env::consts::OS,
            "unknown",
            std::env::consts::ARCH,
        );
        let mut ai_gateway_headers = HashMap::new();
        ai_gateway_headers.insert(
            "User-Agent".to_string(),
            serde_json::Value::String(user_agent),
        );
        result.options.insert(
            "aiGatewayHeaders".to_string(),
            serde_json::to_value(ai_gateway_headers).unwrap_or_default(),
        );

        // Feature flags
        let mut feature_flags = HashMap::new();
        feature_flags.insert(
            "duo_agent_platform_agentic_chat".to_string(),
            serde_json::Value::Bool(true),
        );
        feature_flags.insert(
            "duo_agent_platform".to_string(),
            serde_json::Value::Bool(true),
        );
        result.options.insert(
            "featureFlags".to_string(),
            serde_json::to_value(feature_flags).unwrap_or_default(),
        );

        result.has_custom_get_model = true;

        result
    }
}

/// Cloudflare Workers AI custom loader - account ID and API key.
struct CloudflareWorkersAiLoader;

impl CustomLoader for CloudflareWorkersAiLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").ok();
        if account_id.is_none() {
            result.autoload = false;
            return result;
        }
        let account_id = account_id.unwrap();

        let api_key = std::env::var("CLOUDFLARE_API_KEY").ok();
        result.autoload = api_key.is_some();

        if let Some(key) = api_key {
            result
                .options
                .insert("apiKey".to_string(), serde_json::Value::String(key));
        }
        result.options.insert(
            "baseURL".to_string(),
            serde_json::Value::String(format!(
                "https://api.cloudflare.com/client/v4/accounts/{}/ai/v1",
                account_id
            )),
        );
        result.has_custom_get_model = true;

        result
    }
}

/// Cloudflare AI Gateway custom loader - account ID, gateway ID, API token.
struct CloudflareAiGatewayLoader;

impl CustomLoader for CloudflareAiGatewayLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();

        let account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").ok();
        let gateway = std::env::var("CLOUDFLARE_GATEWAY_ID").ok();

        if account_id.is_none() || gateway.is_none() {
            result.autoload = false;
            return result;
        }

        let api_token = std::env::var("CLOUDFLARE_API_TOKEN")
            .or_else(|_| std::env::var("CF_AIG_TOKEN"))
            .ok();

        result.autoload = api_token.is_some();

        if let Some(ref token) = api_token {
            result.options.insert(
                "apiKey".to_string(),
                serde_json::Value::String(token.clone()),
            );
        }
        if let Some(ref acc) = account_id {
            result.options.insert(
                "accountId".to_string(),
                serde_json::Value::String(acc.clone()),
            );
        }
        if let Some(ref gw) = gateway {
            result
                .options
                .insert("gateway".to_string(), serde_json::Value::String(gw.clone()));
        }
        result.has_custom_get_model = true;

        result
    }
}

/// Cerebras custom loader - adds custom header.
struct CerebrasLoader;

impl CustomLoader for CerebrasLoader {
    fn load(
        &self,
        _provider: &ModelsProviderInfo,
        _provider_state: Option<&ProviderState>,
    ) -> CustomLoaderResult {
        let mut result = CustomLoaderResult::default();
        result.headers.insert(
            "X-Cerebras-3rd-Party-Integration".to_string(),
            "opencode".to_string(),
        );
        result
    }
}

/// Get the custom loader for a provider ID.
fn get_custom_loader(provider_id: &str) -> Option<Box<dyn CustomLoader>> {
    match provider_id {
        "opencode" => Some(Box::new(OpenCodeLoader)),
        "openai" => Some(Box::new(OpenAILoader)),
        "github-copilot" => Some(Box::new(GitHubCopilotLoader)),
        "github-copilot-enterprise" => Some(Box::new(GitHubCopilotEnterpriseLoader)),
        "azure" => Some(Box::new(AzureLoader)),
        "azure-cognitive-services" => Some(Box::new(AzureCognitiveServicesLoader)),
        "amazon-bedrock" => Some(Box::new(AmazonBedrockLoader)),
        "openrouter" => Some(Box::new(OpenRouterLoader)),
        "zenmux" => Some(Box::new(ZenMuxLoader)),
        "vercel" => Some(Box::new(VercelLoader)),
        "google-vertex" => Some(Box::new(GoogleVertexLoader)),
        "google-vertex-ethnopic" => Some(Box::new(GoogleVertexEthnopicLoader)),
        "sap-ai-core" => Some(Box::new(SapAiCoreLoader)),
        "gitlab" => Some(Box::new(GitLabLoader)),
        "cloudflare-workers-ai" => Some(Box::new(CloudflareWorkersAiLoader)),
        "cloudflare-ai-gateway" => Some(Box::new(CloudflareAiGatewayLoader)),
        "cerebras" => Some(Box::new(CerebrasLoader)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Transform helpers: from_models_dev_model / from_models_dev_provider
// ---------------------------------------------------------------------------

/// Transform a models.dev model into a runtime ProviderModel.
pub fn from_models_dev_model(provider: &ModelsProviderInfo, model: &ModelInfo) -> ProviderModel {
    let modalities_input = model
        .modalities
        .as_ref()
        .map(|m| &m.input)
        .cloned()
        .unwrap_or_default();
    let modalities_output = model
        .modalities
        .as_ref()
        .map(|m| &m.output)
        .cloned()
        .unwrap_or_default();

    let interleaved = match model.interleaved.as_ref() {
        Some(ModelInterleaved::Bool(value)) => InterleavedConfig::Bool(*value),
        Some(ModelInterleaved::Field { field }) => InterleavedConfig::Field {
            field: field.clone(),
        },
        None => InterleavedConfig::Bool(false),
    };

    let cost = model.cost.as_ref();
    let over_200k = cost.and_then(|c| c.context_over_200k.as_ref());

    let mut variants = crate::transform::variants(model);
    if let Some(explicit_variants) = &model.variants {
        for (variant_name, options) in explicit_variants {
            variants.insert(variant_name.clone(), options.clone());
        }
    }

    ProviderModel {
        id: model.id.clone(),
        provider_id: provider.id.clone(),
        name: model.name.clone(),
        family: model.family.clone(),
        api: ProviderModelApi {
            id: model.id.clone(),
            url: model
                .provider
                .as_ref()
                .and_then(|p| p.api.clone())
                .or_else(|| provider.api.clone())
                .unwrap_or_default(),
            npm: model
                .provider
                .as_ref()
                .and_then(|p| p.npm.clone())
                .or_else(|| provider.npm.clone())
                .unwrap_or_else(|| "@ai-sdk/openai-compatible".to_string()),
        },
        status: model.status.clone().unwrap_or_else(|| "active".to_string()),
        headers: model.headers.clone().unwrap_or_default(),
        options: model.options.clone(),
        cost: ProviderModelCost {
            input: cost.map(|c| c.input).unwrap_or(0.0),
            output: cost.map(|c| c.output).unwrap_or(0.0),
            cache: ModelCostCache {
                read: cost.and_then(|c| c.cache_read).unwrap_or(0.0),
                write: cost.and_then(|c| c.cache_write).unwrap_or(0.0),
            },
            experimental_over_200k: over_200k.map(|o| ModelCostOver200K {
                input: o.input,
                output: o.output,
                cache: ModelCostCache {
                    read: o.cache_read.unwrap_or(0.0),
                    write: o.cache_write.unwrap_or(0.0),
                },
            }),
        },
        limit: ProviderModelLimit {
            context: model.limit.context,
            input: model.limit.input,
            output: model.limit.output,
        },
        capabilities: ModelCapabilities {
            temperature: model.temperature,
            reasoning: model.reasoning,
            attachment: model.attachment,
            toolcall: model.tool_call,
            input: ModalitySet {
                text: modalities_input.contains(&"text".to_string()),
                audio: modalities_input.contains(&"audio".to_string()),
                image: modalities_input.contains(&"image".to_string()),
                video: modalities_input.contains(&"video".to_string()),
                pdf: modalities_input.contains(&"pdf".to_string()),
            },
            output: ModalitySet {
                text: modalities_output.contains(&"text".to_string()),
                audio: modalities_output.contains(&"audio".to_string()),
                image: modalities_output.contains(&"image".to_string()),
                video: modalities_output.contains(&"video".to_string()),
                pdf: modalities_output.contains(&"pdf".to_string()),
            },
            interleaved,
        },
        release_date: model.release_date.clone().unwrap_or_default(),
        variants: if variants.is_empty() {
            None
        } else {
            Some(variants)
        },
    }
}

/// Transform a models.dev provider into a runtime ProviderState.
pub fn from_models_dev_provider(provider: &ModelsProviderInfo) -> ProviderState {
    let models = provider
        .models
        .iter()
        .map(|(id, model)| (id.clone(), from_models_dev_model(provider, model)))
        .collect();

    ProviderState {
        id: provider.id.clone(),
        source: "custom".to_string(),
        name: provider.name.clone(),
        env: provider.env.clone(),
        key: None,
        options: HashMap::new(),
        models,
    }
}

// ---------------------------------------------------------------------------
// ProviderBootstrapConfig - configuration input for initialization
// ---------------------------------------------------------------------------

/// Configuration for a single model from the config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigModel {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub interleaved: Option<bool>,
    #[serde(default)]
    pub cost: Option<ConfigModelCost>,
    #[serde(default)]
    pub limit: Option<ConfigModelLimit>,
    #[serde(default)]
    pub options: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub modalities: Option<ConfigModalities>,
    #[serde(default)]
    pub provider: Option<ConfigModelProvider>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub variants: Option<HashMap<String, HashMap<String, serde_json::Value>>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigModelCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigModelLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigModalities {
    #[serde(default)]
    pub input: Option<Vec<String>>,
    #[serde(default)]
    pub output: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigModelProvider {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
}

/// Configuration for a single provider from the config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigProvider {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub env: Option<Vec<String>>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub options: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub models: Option<HashMap<String, ConfigModel>>,
    #[serde(default)]
    pub blacklist: Option<Vec<String>>,
    #[serde(default)]
    pub whitelist: Option<Vec<String>>,
}

/// Top-level bootstrap configuration.
#[derive(Debug, Clone, Default)]
pub struct BootstrapConfig {
    /// Provider configs from opencode.json
    pub providers: HashMap<String, ConfigProvider>,
    /// Disabled provider IDs
    pub disabled_providers: HashSet<String>,
    /// Enabled provider IDs (if set, only these are allowed)
    pub enabled_providers: Option<HashSet<String>>,
    /// Whether to enable experimental/alpha models
    pub enable_experimental: bool,
    /// The configured model string (e.g. "ethnopic/test-model")
    pub model: Option<String>,
    /// The configured small model string
    pub small_model: Option<String>,
}

// ---------------------------------------------------------------------------
// ProviderBootstrapState - the initialized state with all providers/models
// ---------------------------------------------------------------------------

/// The initialized provider state, analogous to the TS `state()` return value.
pub struct ProviderBootstrapState {
    pub providers: HashMap<String, ProviderState>,
    /// Provider IDs that have custom getModel loaders.
    pub model_loaders: HashSet<String>,
}

impl ProviderBootstrapState {
    /// Initialize the provider bootstrap state from models.dev data and config.
    /// This is the Rust equivalent of the TS `state()` function.
    pub fn init(
        models_dev: &ModelsData,
        config: &BootstrapConfig,
        auth_store: &HashMap<String, AuthInfo>,
    ) -> Self {
        let mut database: HashMap<String, ProviderState> = models_dev
            .iter()
            .map(|(id, p)| (id.clone(), from_models_dev_provider(p)))
            .collect();

        let disabled = &config.disabled_providers;
        let enabled = &config.enabled_providers;

        let mut providers: HashMap<String, ProviderState> = HashMap::new();
        let mut model_loaders: HashSet<String> = HashSet::new();

        // Add GitHub Copilot Enterprise provider that inherits from GitHub Copilot
        if let Some(github_copilot) = database.get("github-copilot").cloned() {
            let mut enterprise = github_copilot.clone();
            enterprise.id = "github-copilot-enterprise".to_string();
            enterprise.name = "GitHub Copilot Enterprise".to_string();
            for model in enterprise.models.values_mut() {
                model.provider_id = "github-copilot-enterprise".to_string();
            }
            database.insert("github-copilot-enterprise".to_string(), enterprise);
        }

        // Helper closure to merge a partial update into providers
        let merge_provider = |providers: &mut HashMap<String, ProviderState>,
                              database: &HashMap<String, ProviderState>,
                              provider_id: &str,
                              patch: ProviderPatch| {
            if let Some(existing) = providers.get_mut(provider_id) {
                apply_patch(existing, patch);
            } else if let Some(base) = database.get(provider_id) {
                let mut merged = base.clone();
                apply_patch(&mut merged, patch);
                providers.insert(provider_id.to_string(), merged);
            }
        };

        // Extend database from config providers
        for (provider_id, cfg_provider) in &config.providers {
            let existing = database.get(provider_id);
            let mut parsed = ProviderState {
                id: provider_id.clone(),
                name: cfg_provider
                    .name
                    .clone()
                    .or_else(|| existing.map(|e| e.name.clone()))
                    .unwrap_or_else(|| provider_id.clone()),
                env: cfg_provider
                    .env
                    .clone()
                    .or_else(|| existing.map(|e| e.env.clone()))
                    .unwrap_or_default(),
                options: merge_json_maps(
                    existing.map(|e| &e.options).unwrap_or(&HashMap::new()),
                    cfg_provider.options.as_ref().unwrap_or(&HashMap::new()),
                ),
                source: "config".to_string(),
                key: None,
                models: existing.map(|e| e.models.clone()).unwrap_or_default(),
            };

            // Process config model overrides
            if let Some(cfg_models) = &cfg_provider.models {
                for (model_id, cfg_model) in cfg_models {
                    let existing_model = parsed
                        .models
                        .get(&cfg_model.id.clone().unwrap_or_else(|| model_id.clone()));
                    let pm = config_to_provider_model(
                        provider_id,
                        model_id,
                        cfg_model,
                        existing_model,
                        cfg_provider,
                        models_dev.get(provider_id),
                    );
                    parsed.models.insert(model_id.clone(), pm);
                }
            }

            database.insert(provider_id.clone(), parsed);
        }

        // Load from env vars
        for (provider_id, provider) in &database {
            if disabled.contains(provider_id) {
                continue;
            }
            let api_key = provider.env.iter().find_map(|e| std::env::var(e).ok());
            if let Some(_key) = api_key {
                let key_val = if provider.env.len() == 1 {
                    std::env::var(&provider.env[0]).ok()
                } else {
                    None
                };
                merge_provider(
                    &mut providers,
                    &database,
                    provider_id,
                    ProviderPatch {
                        source: Some("env".to_string()),
                        key: key_val,
                        ..Default::default()
                    },
                );
            }
        }

        // Load from auth store
        for (provider_id, auth) in auth_store {
            if disabled.contains(provider_id) {
                continue;
            }
            let maybe_key = match auth {
                AuthInfo::Api { key } => Some(key.clone()),
                AuthInfo::OAuth { access, .. } => Some(access.clone()),
                AuthInfo::WellKnown { token, .. } => Some(token.clone()),
            };
            if let Some(key) = maybe_key {
                merge_provider(
                    &mut providers,
                    &database,
                    provider_id,
                    ProviderPatch {
                        source: Some("api".to_string()),
                        key: Some(key),
                        ..Default::default()
                    },
                );
            }
        }

        // Apply custom loaders
        for (provider_id, data) in &database {
            if disabled.contains(provider_id) {
                continue;
            }
            if let Some(loader) = get_custom_loader(provider_id) {
                // Build a ModelsProviderInfo for the loader
                let models_provider = to_models_provider_info(data, models_dev.get(provider_id));
                let result = loader.load(&models_provider, Some(data));

                if result.autoload || providers.contains_key(provider_id) {
                    if result.has_custom_get_model {
                        model_loaders.insert(provider_id.clone());
                    }

                    let patch = ProviderPatch {
                        source: if providers.contains_key(provider_id) {
                            None
                        } else {
                            Some("custom".to_string())
                        },
                        options: if result.options.is_empty() {
                            None
                        } else {
                            Some(result.options)
                        },
                        ..Default::default()
                    };
                    merge_provider(&mut providers, &database, provider_id, patch);

                    // Apply headers from loader to all models
                    if !result.headers.is_empty() {
                        if let Some(p) = providers.get_mut(provider_id) {
                            for model in p.models.values_mut() {
                                for (k, v) in &result.headers {
                                    model.headers.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }

                    // Apply blacklist
                    if !result.blacklist.is_empty() {
                        if let Some(p) = providers.get_mut(provider_id) {
                            p.models.retain(|mid, _| {
                                let lower = mid.to_lowercase();
                                !result.blacklist.iter().any(|pat| lower.contains(pat))
                            });
                        }
                    }
                }
            }
        }

        // Re-apply config overrides (source, env, name, options)
        for (provider_id, cfg_provider) in &config.providers {
            let mut patch = ProviderPatch {
                source: Some("config".to_string()),
                ..Default::default()
            };
            if let Some(ref env) = cfg_provider.env {
                patch.env = Some(env.clone());
            }
            if let Some(ref name) = cfg_provider.name {
                patch.name = Some(name.clone());
            }
            if let Some(ref opts) = cfg_provider.options {
                patch.options = Some(opts.clone());
            }
            merge_provider(&mut providers, &database, provider_id, patch);
        }

        // Filter and clean up providers
        let is_provider_allowed = |pid: &str| -> bool {
            if let Some(ref en) = enabled {
                if !en.contains(pid) {
                    return false;
                }
            }
            !disabled.contains(pid)
        };

        let provider_ids: Vec<String> = providers.keys().cloned().collect();
        for provider_id in provider_ids {
            if !is_provider_allowed(&provider_id) {
                providers.remove(&provider_id);
                continue;
            }

            let cfg_provider = config.providers.get(&provider_id);

            if let Some(provider) = providers.get_mut(&provider_id) {
                let model_ids: Vec<String> = provider.models.keys().cloned().collect();
                for model_id in model_ids {
                    let should_remove = {
                        let model = &provider.models[&model_id];

                        let blocked_by_status = model_id == "gpt-5-chat-latest"
                            || (provider_id == "openrouter" && model_id == "openai/gpt-5-chat")
                            || (model.status == "alpha" && !config.enable_experimental)
                            || model.status == "deprecated";

                        if blocked_by_status {
                            true
                        }
                        // Apply blacklist/whitelist from config
                        else if let Some(cfg) = cfg_provider {
                            if let Some(ref bl) = cfg.blacklist {
                                if bl.contains(&model_id) {
                                    true
                                } else if let Some(ref wl) = cfg.whitelist {
                                    !wl.contains(&model_id)
                                } else {
                                    false
                                }
                            } else if let Some(ref wl) = cfg.whitelist {
                                !wl.contains(&model_id)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if should_remove {
                        provider.models.remove(&model_id);
                    }
                }

                // Remove providers with no models
                if provider.models.is_empty() {
                    providers.remove(&provider_id);
                }
            }
        }

        ProviderBootstrapState {
            providers,
            model_loaders,
        }
    }

    // -----------------------------------------------------------------------
    // Query functions matching TS Provider namespace exports
    // -----------------------------------------------------------------------

    /// Return all providers.
    pub fn list(&self) -> &HashMap<String, ProviderState> {
        &self.providers
    }

    /// Get a provider by ID.
    pub fn get_provider(&self, provider_id: &str) -> Option<&ProviderState> {
        self.providers.get(provider_id)
    }

    /// Get a model by provider ID and model ID, with fuzzy matching on failure.
    pub fn get_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<&ProviderModel, BootstrapError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            let available: Vec<String> = self.providers.keys().cloned().collect();
            let suggestions = fuzzy_match(provider_id, &available, 3);
            BootstrapError::ModelNotFound {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                suggestions,
            }
        })?;

        provider.models.get(model_id).ok_or_else(|| {
            let available: Vec<String> = provider.models.keys().cloned().collect();
            let suggestions = fuzzy_match(model_id, &available, 3);
            BootstrapError::ModelNotFound {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                suggestions,
            }
        })
    }

    /// Find the closest matching model for a provider given a list of query strings.
    pub fn closest(&self, provider_id: &str, queries: &[&str]) -> Option<(String, String)> {
        let provider = self.providers.get(provider_id)?;
        for query in queries {
            for model_id in provider.models.keys() {
                if model_id.contains(query) {
                    return Some((provider_id.to_string(), model_id.clone()));
                }
            }
        }
        None
    }

    /// Get the small model for a provider, using priority lists.
    pub fn get_small_model(
        &self,
        provider_id: &str,
        config_small_model: Option<&str>,
    ) -> Option<ProviderModel> {
        // If config specifies a small model, use it
        if let Some(model_str) = config_small_model {
            let parsed = parse_model(model_str);
            return self
                .get_model(&parsed.provider_id, &parsed.model_id)
                .ok()
                .cloned();
        }

        if let Some(provider) = self.providers.get(provider_id) {
            let mut priority: Vec<&str> = vec!["gemini-3-flash", "gemini-2.5-flash", "gpt-5-nano"];

            if provider_id.starts_with("opencode") {
                priority = vec!["gpt-5-nano"];
            }
            if provider_id.starts_with("github-copilot") {
                priority = vec!["gpt-5-mini"];
                priority.extend_from_slice(&["gemini-3-flash", "gemini-2.5-flash", "gpt-5-nano"]);
            }

            for item in &priority {
                if provider_id == "amazon-bedrock" {
                    let cross_region_prefixes = ["global.", "us.", "eu."];
                    let candidates: Vec<&String> = provider
                        .models
                        .keys()
                        .filter(|m| m.contains(item))
                        .collect();

                    // Priority: 1) global. 2) user's region prefix 3) unprefixed
                    if let Some(global_match) = candidates.iter().find(|m| m.starts_with("global."))
                    {
                        return provider.models.get(*global_match).cloned();
                    }

                    let region = provider
                        .options
                        .get("region")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let region_prefix = region.split('-').next().unwrap_or("");
                    if region_prefix == "us" || region_prefix == "eu" {
                        if let Some(regional) = candidates
                            .iter()
                            .find(|m| m.starts_with(&format!("{}.", region_prefix)))
                        {
                            return provider.models.get(*regional).cloned();
                        }
                    }

                    if let Some(unprefixed) = candidates
                        .iter()
                        .find(|m| !cross_region_prefixes.iter().any(|p| m.starts_with(p)))
                    {
                        return provider.models.get(*unprefixed).cloned();
                    }
                } else {
                    for model_id in provider.models.keys() {
                        if model_id.contains(item) {
                            return provider.models.get(model_id).cloned();
                        }
                    }
                }
            }
        }

        // Fallback: check opencode provider for gpt-5-nano
        if let Some(rocode_provider) = self.providers.get("opencode") {
            if let Some(model) = rocode_provider.models.get("gpt-5-nano") {
                return Some(model.clone());
            }
        }

        None
    }

    /// Sort models by priority (matching TS sort function).
    pub fn sort_models(models: &mut [ProviderModel]) {
        let priority_list = ["gpt-5", "big-pickle", "gemini-3-pro"];

        models.sort_by(|a, b| {
            let a_pri = priority_list
                .iter()
                .position(|p| a.id.contains(p))
                .map(|i| -(i as i64))
                .unwrap_or(i64::MAX);
            let b_pri = priority_list
                .iter()
                .position(|p| b.id.contains(p))
                .map(|i| -(i as i64))
                .unwrap_or(i64::MAX);

            a_pri
                .cmp(&b_pri)
                .then_with(|| {
                    let a_latest = if a.id.contains("latest") { 0 } else { 1 };
                    let b_latest = if b.id.contains("latest") { 0 } else { 1 };
                    a_latest.cmp(&b_latest)
                })
                .then_with(|| b.id.cmp(&a.id))
        });
    }

    /// Get the default model from config or first available provider.
    pub fn default_model(
        &self,
        config_model: Option<&str>,
        recent: &[(String, String)],
    ) -> Option<ParsedModel> {
        if let Some(model_str) = config_model {
            return Some(parse_model(model_str));
        }

        // Check recent models
        for (provider_id, model_id) in recent {
            if let Some(provider) = self.providers.get(provider_id) {
                if provider.models.contains_key(model_id) {
                    return Some(ParsedModel {
                        provider_id: provider_id.clone(),
                        model_id: model_id.clone(),
                    });
                }
            }
        }

        // Fall back to first provider, sorted models
        let provider = self.providers.values().next()?;
        let mut models: Vec<ProviderModel> = provider.models.values().cloned().collect();
        Self::sort_models(&mut models);
        let model = models.first()?;
        Some(ParsedModel {
            provider_id: provider.id.clone(),
            model_id: model.id.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// ParsedModel and parse_model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedModel {
    pub provider_id: String,
    pub model_id: String,
}

/// Parse a "provider/model" format string.
pub fn parse_model(model_str: &str) -> ParsedModel {
    if let Some(pos) = model_str.find('/') {
        ParsedModel {
            provider_id: model_str[..pos].to_string(),
            model_id: model_str[pos + 1..].to_string(),
        }
    } else {
        ParsedModel {
            provider_id: model_str.to_string(),
            model_id: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ProviderPatch - partial update for ProviderState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ProviderPatch {
    pub source: Option<String>,
    pub name: Option<String>,
    pub env: Option<Vec<String>>,
    pub key: Option<String>,
    pub options: Option<HashMap<String, serde_json::Value>>,
}

/// Apply a partial patch to a ProviderState, merging fields that are `Some`.
fn apply_patch(state: &mut ProviderState, patch: ProviderPatch) {
    if let Some(source) = patch.source {
        state.source = source;
    }
    if let Some(name) = patch.name {
        state.name = name;
    }
    if let Some(env) = patch.env {
        state.env = env;
    }
    if let Some(key) = patch.key {
        state.key = Some(key);
    }
    if let Some(options) = patch.options {
        for (k, v) in options {
            state.options.insert(k, v);
        }
    }
}

/// Merge two JSON option maps, with `overlay` values taking precedence.
fn merge_json_maps(
    base: &HashMap<String, serde_json::Value>,
    overlay: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut result = base.clone();
    for (k, v) in overlay {
        result.insert(k.clone(), v.clone());
    }
    result
}

/// Convert a ConfigModel (from user config) into a ProviderModel, using an
/// optional existing model as the base for defaults.
fn config_to_provider_model(
    provider_id: &str,
    model_id: &str,
    cfg: &ConfigModel,
    existing: Option<&ProviderModel>,
    cfg_provider: &ConfigProvider,
    models_provider: Option<&ModelsProviderInfo>,
) -> ProviderModel {
    let api_model_id = cfg.id.clone().unwrap_or_else(|| model_id.to_string());

    let default_npm = cfg_provider
        .npm
        .clone()
        .or_else(|| models_provider.and_then(|p| p.npm.clone()))
        .unwrap_or_else(|| "@ai-sdk/openai-compatible".to_string());
    let default_api = cfg_provider
        .api
        .clone()
        .or_else(|| models_provider.and_then(|p| p.api.clone()))
        .unwrap_or_default();

    let base_cost = existing.map(|e| &e.cost);
    let base_limit = existing.map(|e| &e.limit);
    let base_caps = existing.map(|e| &e.capabilities);

    let cost = {
        let cfg_cost = cfg.cost.as_ref();
        ProviderModelCost {
            input: cfg_cost
                .and_then(|c| c.input)
                .or_else(|| base_cost.map(|c| c.input))
                .unwrap_or(0.0),
            output: cfg_cost
                .and_then(|c| c.output)
                .or_else(|| base_cost.map(|c| c.output))
                .unwrap_or(0.0),
            cache: ModelCostCache {
                read: cfg_cost
                    .and_then(|c| c.cache_read)
                    .or_else(|| base_cost.map(|c| c.cache.read))
                    .unwrap_or(0.0),
                write: cfg_cost
                    .and_then(|c| c.cache_write)
                    .or_else(|| base_cost.map(|c| c.cache.write))
                    .unwrap_or(0.0),
            },
            experimental_over_200k: base_cost.and_then(|c| c.experimental_over_200k.clone()),
        }
    };

    let limit = ProviderModelLimit {
        context: cfg
            .limit
            .as_ref()
            .and_then(|l| l.context)
            .or_else(|| base_limit.map(|l| l.context))
            .unwrap_or(128000),
        input: base_limit.and_then(|l| l.input),
        output: cfg
            .limit
            .as_ref()
            .and_then(|l| l.output)
            .or_else(|| base_limit.map(|l| l.output))
            .unwrap_or(4096),
    };

    let modalities_input = cfg
        .modalities
        .as_ref()
        .and_then(|m| m.input.as_ref())
        .cloned()
        .unwrap_or_else(|| {
            if base_caps.map(|c| c.input.text).unwrap_or(true) {
                vec!["text".to_string()]
            } else {
                vec![]
            }
        });
    let modalities_output = cfg
        .modalities
        .as_ref()
        .and_then(|m| m.output.as_ref())
        .cloned()
        .unwrap_or_else(|| {
            if base_caps.map(|c| c.output.text).unwrap_or(true) {
                vec!["text".to_string()]
            } else {
                vec![]
            }
        });

    let interleaved = match cfg.interleaved {
        Some(v) => InterleavedConfig::Bool(v),
        None => existing
            .map(|e| e.capabilities.interleaved.clone())
            .unwrap_or_default(),
    };

    let options = merge_json_maps(
        &existing.map(|e| e.options.clone()).unwrap_or_default(),
        cfg.options.as_ref().unwrap_or(&HashMap::new()),
    );
    let headers = merge_string_maps(
        &existing.map(|e| e.headers.clone()).unwrap_or_default(),
        cfg.headers.as_ref().unwrap_or(&HashMap::new()),
    );

    ProviderModel {
        id: model_id.to_string(),
        provider_id: provider_id.to_string(),
        name: cfg.name.clone().unwrap_or_else(|| {
            if cfg.id.as_deref().is_some_and(|id| id != model_id) {
                model_id.to_string()
            } else {
                existing
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| model_id.to_string())
            }
        }),
        family: cfg
            .family
            .clone()
            .or_else(|| existing.and_then(|e| e.family.clone())),
        api: ProviderModelApi {
            id: api_model_id,
            url: cfg
                .provider
                .as_ref()
                .and_then(|p| p.api.clone())
                .or_else(|| existing.map(|e| e.api.url.clone()))
                .unwrap_or_else(|| default_api.clone()),
            npm: cfg
                .provider
                .as_ref()
                .and_then(|p| p.npm.clone())
                .or_else(|| existing.map(|e| e.api.npm.clone()))
                .unwrap_or_else(|| default_npm.clone()),
        },

        status: cfg
            .status
            .clone()
            .or_else(|| existing.map(|e| e.status.clone()))
            .unwrap_or_else(|| "active".to_string()),
        cost,
        limit,
        capabilities: ModelCapabilities {
            temperature: cfg
                .temperature
                .or_else(|| base_caps.map(|c| c.temperature))
                .unwrap_or(true),
            reasoning: cfg
                .reasoning
                .or_else(|| base_caps.map(|c| c.reasoning))
                .unwrap_or(false),
            attachment: cfg
                .attachment
                .or_else(|| base_caps.map(|c| c.attachment))
                .unwrap_or(false),
            toolcall: cfg
                .tool_call
                .or_else(|| base_caps.map(|c| c.toolcall))
                .unwrap_or(true),
            input: ModalitySet {
                text: modalities_input.contains(&"text".to_string()),
                audio: modalities_input.contains(&"audio".to_string()),
                image: modalities_input.contains(&"image".to_string()),
                video: modalities_input.contains(&"video".to_string()),
                pdf: modalities_input.contains(&"pdf".to_string()),
            },
            output: ModalitySet {
                text: modalities_output.contains(&"text".to_string()),
                audio: modalities_output.contains(&"audio".to_string()),
                image: modalities_output.contains(&"image".to_string()),
                video: modalities_output.contains(&"video".to_string()),
                pdf: modalities_output.contains(&"pdf".to_string()),
            },
            interleaved,
        },
        options,
        headers,
        release_date: cfg
            .release_date
            .clone()
            .or_else(|| existing.map(|e| e.release_date.clone()))
            .unwrap_or_default(),
        variants: cfg
            .variants
            .clone()
            .or_else(|| existing.and_then(|e| e.variants.clone())),
    }
}

/// Merge two String->String maps, with `overlay` values taking precedence.
fn merge_string_maps(
    base: &HashMap<String, String>,
    overlay: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut result = base.clone();
    for (k, v) in overlay {
        result.insert(k.clone(), v.clone());
    }
    result
}

/// Convert a ProviderState back to a ModelsProviderInfo for use by custom loaders.
/// Falls back to the original models.dev data when available.
fn to_models_provider_info(
    state: &ProviderState,
    original: Option<&ModelsProviderInfo>,
) -> ModelsProviderInfo {
    // If we have the original models.dev data, prefer it (loaders expect that shape).
    if let Some(orig) = original {
        return orig.clone();
    }

    // Otherwise, reconstruct a minimal ModelsProviderInfo from the runtime state.
    let models = state
        .models
        .iter()
        .map(|(id, pm)| {
            let mi = ModelInfo {
                id: pm.id.clone(),
                name: pm.name.clone(),
                family: pm.family.clone(),
                release_date: Some(pm.release_date.clone()),
                attachment: pm.capabilities.attachment,
                reasoning: pm.capabilities.reasoning,
                temperature: pm.capabilities.temperature,
                tool_call: pm.capabilities.toolcall,
                interleaved: match &pm.capabilities.interleaved {
                    InterleavedConfig::Bool(b) => Some(ModelInterleaved::Bool(*b)),
                    InterleavedConfig::Field { field } => Some(ModelInterleaved::Field {
                        field: field.clone(),
                    }),
                },
                cost: Some(crate::models::ModelCost {
                    input: pm.cost.input,
                    output: pm.cost.output,
                    cache_read: Some(pm.cost.cache.read),
                    cache_write: Some(pm.cost.cache.write),
                    context_over_200k: None,
                }),
                limit: crate::models::ModelLimit {
                    context: pm.limit.context,
                    input: pm.limit.input,
                    output: pm.limit.output,
                },
                modalities: None,
                experimental: None,
                status: Some(pm.status.clone()),
                options: pm.options.clone(),
                headers: if pm.headers.is_empty() {
                    None
                } else {
                    Some(pm.headers.clone())
                },
                provider: Some(crate::models::ModelProvider {
                    npm: Some(pm.api.npm.clone()),
                    api: Some(pm.api.url.clone()),
                }),
                variants: pm.variants.clone(),
            };
            (id.clone(), mi)
        })
        .collect();

    ModelsProviderInfo {
        id: state.id.clone(),
        name: state.name.clone(),
        env: state.env.clone(),
        api: None,
        npm: None,
        models,
    }
}

/// Simple fuzzy string matching: returns up to `max` candidates from `options`
/// that share a common substring with `query`, sorted by edit-distance-like score.
fn fuzzy_match(query: &str, options: &[String], max: usize) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(usize, &String)> = options
        .iter()
        .filter_map(|opt| {
            let opt_lower = opt.to_lowercase();
            // Score: length of longest common substring (simple heuristic)
            let score = longest_common_substring_len(&query_lower, &opt_lower);
            if score >= 2 || opt_lower.contains(&query_lower) || query_lower.contains(&opt_lower) {
                Some((score, opt))
            } else {
                None
            }
        })
        .collect();

    // Sort descending by score
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(max)
        .map(|(_, s)| s.clone())
        .collect()
}

/// Length of the longest common substring between two strings.
fn longest_common_substring_len(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut max_len = 0;
    // Simple O(n*m) DP approach
    let mut prev = vec![0usize; b_bytes.len() + 1];
    for i in 1..=a_bytes.len() {
        let mut curr = vec![0usize; b_bytes.len() + 1];
        for j in 1..=b_bytes.len() {
            if a_bytes[i - 1] == b_bytes[j - 1] {
                curr[j] = prev[j - 1] + 1;
                if curr[j] > max_len {
                    max_len = curr[j];
                }
            }
        }
        prev = curr;
    }
    max_len
}

// ---------------------------------------------------------------------------
// Public API functions exported from lib.rs
// ---------------------------------------------------------------------------

fn env_any(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn provider_option_string(provider: &ProviderState, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = options_get_insensitive(&provider.options, key) else {
            continue;
        };
        match value {
            serde_json::Value::String(s) if !s.trim().is_empty() => return Some(s.clone()),
            serde_json::Value::Number(n) => return Some(n.to_string()),
            serde_json::Value::Bool(b) => return Some(b.to_string()),
            _ => {}
        }
    }
    None
}

/// Look up a value from an options map: exact match first, then case-insensitive fallback.
fn options_get_insensitive<'a>(
    options: &'a HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(v) = options.get(key) {
        return Some(v);
    }
    let key_lower = key.to_lowercase();
    options
        .iter()
        .find_map(|(k, v)| (k.to_lowercase() == key_lower).then_some(v))
}

fn provider_secret(provider: &ProviderState, fallback_env: &[&str]) -> Option<String> {
    provider_option_string(provider, &["apiKey", "api_key", "apikey"])
        .or_else(|| provider.key.clone().filter(|k| !k.trim().is_empty()))
        .or_else(|| {
            provider
                .env
                .iter()
                .find_map(|name| std::env::var(name).ok())
                .filter(|k| !k.trim().is_empty())
        })
        .or_else(|| env_any(fallback_env))
}

fn provider_base_url(provider: &ProviderState) -> Option<String> {
    provider_option_string(provider, &["baseURL", "baseUrl", "url", "api"])
        .or_else(|| {
            provider
                .models
                .values()
                .find_map(|model| (!model.api.url.trim().is_empty()).then(|| model.api.url.clone()))
        })
        .or_else(|| {
            // GLM Coding Plan requires a dedicated endpoint instead of the generic API.
            // TS users commonly configure this as provider id `zhipuai-coding-plan`.
            if provider.id == "zhipuai-coding-plan" {
                Some("https://open.bigmodel.cn/api/coding/paas/v4".to_string())
            } else {
                None
            }
        })
}

fn default_npm_for_provider_id(provider_id: &str) -> &'static str {
    match provider_id {
        "ethnopic" => "@ai-sdk/anthropic",
        "google" => "@ai-sdk/google",
        "google-vertex" | "google-vertex-ethnopic" => "@ai-sdk/google-vertex",
        "amazon-bedrock" => "@ai-sdk/amazon-bedrock",
        "github-copilot" | "github-copilot-enterprise" => "@ai-sdk/github-copilot",
        "gitlab" => "@gitlab/gitlab-ai-provider",
        "openai" => "@ai-sdk/openai",
        _ => "@ai-sdk/openai-compatible",
    }
}

fn resolve_npm_for_provider(provider_id: &str, provider: &ProviderState) -> String {
    if let Some(npm) = provider_option_string(provider, &["npm"]) {
        return npm;
    }

    if let Some(npm) = provider
        .models
        .values()
        .find_map(|model| (!model.api.npm.trim().is_empty()).then(|| model.api.npm.clone()))
    {
        return npm;
    }

    default_npm_for_provider_id(provider_id).to_string()
}

fn default_secret_env_for_provider(provider_id: &str, protocol: Protocol) -> Vec<&'static str> {
    match protocol {
        Protocol::Messages => vec!["ANTHROPIC_API_KEY"],
        Protocol::Google => vec!["GOOGLE_API_KEY", "GOOGLE_GENERATIVE_AI_API_KEY"],
        Protocol::Bedrock => vec!["AWS_ACCESS_KEY_ID"],
        Protocol::Vertex => vec![
            "GOOGLE_VERTEX_ACCESS_TOKEN",
            "GOOGLE_CLOUD_ACCESS_TOKEN",
            "GOOGLE_OAUTH_ACCESS_TOKEN",
            "GCP_ACCESS_TOKEN",
        ],
        Protocol::GitHubCopilot => vec!["GITHUB_COPILOT_TOKEN"],
        Protocol::GitLab => vec!["GITLAB_TOKEN"],
        Protocol::OpenAI => match provider_id {
            "openai" => vec!["OPENAI_API_KEY"],
            "opencode" => vec!["ROCODE_API_KEY", "OPENCODE_API_KEY"],
            "openrouter" => vec!["OPENROUTER_API_KEY"],
            "mistral" => vec!["MISTRAL_API_KEY"],
            "groq" => vec!["GROQ_API_KEY"],
            "deepinfra" => vec!["DEEPINFRA_API_KEY"],
            "deepseek" => vec!["DEEPSEEK_API_KEY"],
            "xai" => vec!["XAI_API_KEY"],
            "cerebras" => vec!["CEREBRAS_API_KEY"],
            "cohere" => vec!["COHERE_API_KEY"],
            "together" | "togetherai" => vec!["TOGETHER_API_KEY", "TOGETHERAI_API_KEY"],
            "perplexity" => vec!["PERPLEXITY_API_KEY"],
            "vercel" => vec!["VERCEL_API_KEY"],
            _ => vec![],
        },
    }
}

fn collect_provider_headers(provider: &ProviderState) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    for model in provider.models.values() {
        headers.extend(model.headers.clone());
    }

    if let Some(serde_json::Value::Object(map)) = provider.options.get("headers") {
        for (key, value) in map {
            if let Some(s) = value.as_str() {
                headers.insert(key.clone(), s.to_string());
            }
        }
    }

    headers
}

fn parse_bool_text(raw: &str) -> Option<bool> {
    let lower = raw.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "1" | "true" | "yes" | "on") {
        return Some(true);
    }
    if matches!(lower.as_str(), "0" | "false" | "no" | "off") {
        return Some(false);
    }
    None
}

fn option_bool(options: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<bool> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        match value {
            serde_json::Value::Bool(v) => return Some(*v),
            serde_json::Value::Number(n) => return Some(n.as_i64().unwrap_or(0) != 0),
            serde_json::Value::String(s) => {
                if let Some(v) = parse_bool_text(s) {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn option_u32(options: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<u32> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        match value {
            serde_json::Value::Number(n) => {
                if let Some(v) = n.as_u64() {
                    return Some(v as u32);
                }
                if let Some(v) = n.as_i64() {
                    return Some(v.max(0) as u32);
                }
            }
            serde_json::Value::String(s) => {
                if let Ok(v) = s.parse::<u32>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn option_u64(options: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        match value {
            serde_json::Value::Number(n) => {
                if let Some(v) = n.as_u64() {
                    return Some(v);
                }
                if let Some(v) = n.as_i64() {
                    return Some(v.max(0) as u64);
                }
            }
            serde_json::Value::String(s) => {
                if let Ok(v) = s.parse::<u64>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn option_f64(options: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<f64> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        match value {
            serde_json::Value::Number(n) => {
                if let Some(v) = n.as_f64() {
                    return Some(v);
                }
            }
            serde_json::Value::String(s) => {
                if let Ok(v) = s.parse::<f64>() {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn option_string(options: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        match value {
            serde_json::Value::String(v) if !v.trim().is_empty() => return Some(v.clone()),
            serde_json::Value::Number(v) => return Some(v.to_string()),
            serde_json::Value::Bool(v) => return Some(v.to_string()),
            _ => {}
        }
    }
    None
}

fn env_bool(keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            if let Some(v) = parse_bool_text(&raw) {
                return Some(v);
            }
        }
    }
    None
}

fn env_u32(keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            if let Ok(v) = raw.parse::<u32>() {
                return Some(v);
            }
        }
    }
    None
}

fn env_u64(keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            if let Ok(v) = raw.parse::<u64>() {
                return Some(v);
            }
        }
    }
    None
}

fn env_f64(keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            if let Ok(v) = raw.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

fn env_string(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(raw) = std::env::var(key) {
            if !raw.trim().is_empty() {
                return Some(raw);
            }
        }
    }
    None
}

fn build_runtime_config(options: &HashMap<String, serde_json::Value>) -> RuntimeConfig {
    let defaults = RuntimeConfig::default();
    RuntimeConfig {
        enabled: option_bool(options, &["runtime_enabled"])
            .or_else(|| env_bool(&["ROCODE_RUNTIME_ENABLED"]))
            .unwrap_or(defaults.enabled),
        preflight_enabled: option_bool(options, &["runtime_preflight", "preflight_enabled"])
            .or_else(|| env_bool(&["ROCODE_RUNTIME_PREFLIGHT"]))
            .unwrap_or(defaults.preflight_enabled),
        pipeline_enabled: option_bool(options, &["runtime_pipeline", "pipeline_enabled"])
            .or_else(|| env_bool(&["ROCODE_RUNTIME_PIPELINE"]))
            .unwrap_or(defaults.pipeline_enabled),
        circuit_breaker_threshold: option_u32(
            options,
            &[
                "circuit_breaker_threshold",
                "runtime_circuit_breaker_threshold",
            ],
        )
        .or_else(|| env_u32(&["ROCODE_RUNTIME_CIRCUIT_BREAKER_THRESHOLD"]))
        .unwrap_or(defaults.circuit_breaker_threshold),
        circuit_breaker_cooldown_secs: option_u64(
            options,
            &[
                "circuit_breaker_cooldown_secs",
                "runtime_circuit_breaker_cooldown_secs",
            ],
        )
        .or_else(|| env_u64(&["ROCODE_RUNTIME_CIRCUIT_BREAKER_COOLDOWN_SECS"]))
        .unwrap_or(defaults.circuit_breaker_cooldown_secs),
        rate_limit_rps: option_f64(options, &["rate_limit_rps", "runtime_rate_limit_rps"])
            .or_else(|| env_f64(&["ROCODE_RUNTIME_RATE_LIMIT_RPS"]))
            .unwrap_or(defaults.rate_limit_rps),
        max_inflight: option_u32(options, &["max_inflight", "runtime_max_inflight"])
            .or_else(|| env_u32(&["ROCODE_RUNTIME_MAX_INFLIGHT"]))
            .unwrap_or(defaults.max_inflight),
        protocol_path: option_string(options, &["protocol_path", "runtime_protocol_path"])
            .or_else(|| env_string(&["ROCODE_RUNTIME_PROTOCOL_PATH"])),
        protocol_version: option_string(options, &["protocol_version", "runtime_protocol_version"])
            .or_else(|| env_string(&["ROCODE_RUNTIME_PROTOCOL_VERSION"])),
        hot_reload: option_bool(options, &["hot_reload", "runtime_hot_reload"])
            .or_else(|| env_bool(&["ROCODE_RUNTIME_HOT_RELOAD"]))
            .unwrap_or(defaults.hot_reload),
    }
}

fn provider_config_for_protocol(
    provider_id: &str,
    provider: &ProviderState,
    protocol: Protocol,
) -> Option<ProviderConfig> {
    let fallback_env = default_secret_env_for_provider(provider_id, protocol);
    let npm = resolve_npm_for_provider(provider_id, provider);
    let headers = collect_provider_headers(provider);
    let mut options = provider.options.clone();
    options.insert("npm".to_string(), serde_json::Value::String(npm));

    // For the OpenAI protocol, mark non-OpenAI providers as legacy-only
    // so they use Chat Completions directly instead of the Responses API.
    if matches!(protocol, Protocol::OpenAI) && provider_id != "openai" {
        options.insert("legacy_only".to_string(), serde_json::Value::Bool(true));
    }

    let base_url = provider_base_url(provider).unwrap_or_default();

    let api_key = match protocol {
        Protocol::Bedrock => {
            let access_key_id = provider_option_string(provider, &["accessKeyId", "access_key_id"])
                .or_else(|| env_any(&["AWS_ACCESS_KEY_ID"]))
                .or_else(|| provider_secret(provider, &fallback_env))?;
            let secret =
                provider_option_string(provider, &["secretAccessKey", "secret_access_key"])
                    .or_else(|| env_any(&["AWS_SECRET_ACCESS_KEY"]))?;
            let region = provider_option_string(provider, &["region"])
                .or_else(|| env_any(&["AWS_REGION"]))
                .unwrap_or_else(|| "us-east-1".to_string());
            options.insert(
                "access_key_id".to_string(),
                serde_json::Value::String(access_key_id.clone()),
            );
            options.insert(
                "secret_access_key".to_string(),
                serde_json::Value::String(secret),
            );
            options.insert("region".to_string(), serde_json::Value::String(region));
            if let Some(session_token) =
                provider_option_string(provider, &["sessionToken", "session_token"])
                    .or_else(|| env_any(&["AWS_SESSION_TOKEN"]))
            {
                options.insert(
                    "session_token".to_string(),
                    serde_json::Value::String(session_token),
                );
            }
            access_key_id
        }
        Protocol::Vertex => {
            let token = provider_option_string(provider, &["accessToken", "access_token", "token"])
                .or_else(|| provider_secret(provider, &fallback_env))?;
            let project = provider_option_string(provider, &["project", "projectId", "project_id"])
                .or_else(|| env_any(&["GOOGLE_CLOUD_PROJECT", "GCP_PROJECT", "GCLOUD_PROJECT"]))?;
            let location = provider_option_string(provider, &["location"])
                .or_else(|| env_any(&["GOOGLE_CLOUD_LOCATION", "VERTEX_LOCATION"]))
                .unwrap_or_else(|| "us-east5".to_string());
            options.insert("project".to_string(), serde_json::Value::String(project));
            options.insert("location".to_string(), serde_json::Value::String(location));
            token
        }
        _ => provider_secret(provider, &fallback_env)?,
    };

    Some(ProviderConfig {
        provider_id: provider_id.to_string(),
        base_url,
        api_key,
        headers,
        options,
    })
}

fn create_protocol_provider(
    provider_id: &str,
    provider: &ProviderState,
) -> Option<Arc<dyn RuntimeProvider>> {
    // Keep Azure on legacy path for now: it requires endpoint-specific wiring.
    if provider_id == "azure" {
        return None;
    }

    let npm = resolve_npm_for_provider(provider_id, provider);
    let protocol = Protocol::from_npm(&npm);
    let mut config = provider_config_for_protocol(provider_id, provider, protocol)?;

    let manifest: Option<ProtocolManifest> = ProtocolLoader::new()
        .try_load_provider(provider_id, &config.options)
        .and_then(|manifest| match ProtocolValidator::validate(&manifest) {
            Ok(()) => Some(manifest),
            Err(err) => {
                tracing::warn!(
                    provider = provider_id,
                    error = %err,
                    "protocol manifest validation failed, using legacy protocol routing"
                );
                None
            }
        });

    if let Some(manifest) = &manifest {
        if config.base_url.trim().is_empty() && !manifest.endpoint.base_url.trim().is_empty() {
            config.base_url = manifest.endpoint.base_url.clone();
        }
        config.options.insert(
            "runtime_manifest_id".to_string(),
            serde_json::Value::String(manifest.id.clone()),
        );
        config.options.insert(
            "runtime_manifest_version".to_string(),
            serde_json::Value::String(manifest.protocol_version.clone()),
        );
    }

    let mut runtime_config = build_runtime_config(&config.options);
    if runtime_config.protocol_version.is_none() {
        if let Some(manifest) = &manifest {
            runtime_config.protocol_version = Some(manifest.protocol_version.clone());
        }
    }
    config.options.insert(
        "runtime_enabled".to_string(),
        serde_json::Value::Bool(runtime_config.enabled),
    );
    config.options.insert(
        "runtime_preflight".to_string(),
        serde_json::Value::Bool(runtime_config.preflight_enabled),
    );
    config.options.insert(
        "runtime_pipeline".to_string(),
        serde_json::Value::Bool(runtime_config.pipeline_enabled),
    );

    let protocol_impl = create_protocol_impl(protocol);

    let mut models: HashMap<String, RuntimeModelInfo> = provider
        .models
        .values()
        .map(|model| (model.id.clone(), state_model_to_runtime(provider_id, model)))
        .collect();

    if models.is_empty() {
        if let Some(legacy) = create_legacy_provider(provider_id, provider) {
            models = legacy
                .models()
                .into_iter()
                .map(|model| (model.id.clone(), model))
                .collect();
        }
    }

    let mut instance = ProviderInstance::new(
        provider_id.to_string(),
        provider.name.clone(),
        config,
        protocol_impl,
        models,
    );

    if runtime_config.enabled {
        let protocol_source = if let Some(manifest) = &manifest {
            ProtocolSource::Manifest {
                path: runtime_config
                    .protocol_path
                    .clone()
                    .unwrap_or_else(|| "env/auto".to_string()),
                version: runtime_config
                    .protocol_version
                    .clone()
                    .unwrap_or_else(|| manifest.protocol_version.clone()),
            }
        } else {
            ProtocolSource::Legacy { npm: npm.clone() }
        };

        let context = RuntimeContext {
            protocol_source,
            provider_id: provider_id.to_string(),
            created_at: Instant::now(),
        };
        let mut runtime = ProviderRuntime::new(runtime_config.clone(), context);
        if runtime.is_pipeline_enabled() {
            let pipeline = match manifest.as_ref() {
                Some(manifest) => Pipeline::from_manifest(manifest).unwrap_or_else(|err| {
                    tracing::warn!(
                        provider = provider_id,
                        error = %err,
                        "failed to build runtime pipeline from manifest, using provider defaults"
                    );
                    Pipeline::for_provider(provider_id)
                }),
                None => Pipeline::for_provider(provider_id),
            };
            runtime.set_pipeline(Arc::new(pipeline));
        }
        instance = instance.with_runtime(runtime);
    }

    Some(Arc::new(instance))
}

fn create_concrete_provider(
    provider_id: &str,
    provider: &ProviderState,
) -> Option<Arc<dyn RuntimeProvider>> {
    create_protocol_provider(provider_id, provider)
        .or_else(|| create_legacy_provider(provider_id, provider))
}

fn create_legacy_provider(
    provider_id: &str,
    provider: &ProviderState,
) -> Option<Arc<dyn RuntimeProvider>> {
    match provider_id {
        "azure" => {
            let api_key = provider_secret(provider, &["AZURE_API_KEY", "AZURE_OPENAI_API_KEY"])?;
            let endpoint =
                provider_option_string(provider, &["endpoint", "baseURL", "baseUrl", "url"])
                    .or_else(|| env_any(&["AZURE_ENDPOINT", "AZURE_OPENAI_ENDPOINT"]))?;
            Some(Arc::new(AzureProvider::new(api_key, endpoint)))
        }
        _ => {
            let is_openai_compatible = provider.models.values().any(|model| {
                model
                    .api
                    .npm
                    .to_ascii_lowercase()
                    .contains("openai-compatible")
            });
            if !is_openai_compatible {
                return None;
            }
            let api_key = provider_secret(provider, &[])?;
            let base_url = provider_base_url(provider)?;
            let config = ProviderConfig::new(provider_id, base_url, api_key)
                .with_option("legacy_only", serde_json::json!(true));
            let models: HashMap<String, RuntimeModelInfo> = provider
                .models
                .values()
                .map(|model| (model.id.clone(), state_model_to_runtime(provider_id, model)))
                .collect();
            Some(Arc::new(crate::ProviderInstance::new(
                provider_id.to_string(),
                provider_id.to_string(),
                config,
                crate::protocols::create_protocol_impl(Protocol::OpenAI),
                models,
            )))
        }
    }
}

struct AliasedProvider {
    id: String,
    name: String,
    inner: Arc<dyn RuntimeProvider>,
    models: Vec<RuntimeModelInfo>,
    model_index: HashMap<String, RuntimeModelInfo>,
}

impl AliasedProvider {
    fn new(
        id: String,
        name: String,
        inner: Arc<dyn RuntimeProvider>,
        models: Vec<RuntimeModelInfo>,
    ) -> Self {
        let model_index = models
            .iter()
            .map(|model| (model.id.clone(), model.clone()))
            .collect();
        Self {
            id,
            name,
            inner,
            models,
            model_index,
        }
    }
}

#[async_trait]
impl RuntimeProvider for AliasedProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn models(&self) -> Vec<RuntimeModelInfo> {
        self.models.clone()
    }

    fn get_model(&self, id: &str) -> Option<&RuntimeModelInfo> {
        self.model_index.get(id)
    }

    async fn chat(
        &self,
        request: crate::ChatRequest,
    ) -> Result<crate::ChatResponse, crate::ProviderError> {
        self.inner.chat(request).await
    }

    async fn chat_stream(
        &self,
        request: crate::ChatRequest,
    ) -> Result<crate::StreamResult, crate::ProviderError> {
        self.inner.chat_stream(request).await
    }
}

fn state_model_to_runtime(provider_id: &str, model: &ProviderModel) -> RuntimeModelInfo {
    RuntimeModelInfo {
        id: model.id.clone(),
        name: model.name.clone(),
        provider: provider_id.to_string(),
        context_window: model.limit.context,
        max_input_tokens: model.limit.input,
        max_output_tokens: model.limit.output,
        supports_vision: model.capabilities.input.image
            || model.capabilities.output.image
            || model.capabilities.input.video
            || model.capabilities.output.video,
        supports_tools: model.capabilities.toolcall,
        cost_per_million_input: model.cost.input,
        cost_per_million_output: model.cost.output,
    }
}

fn wrap_provider_for_state(
    provider_state: &ProviderState,
    provider: Arc<dyn RuntimeProvider>,
) -> Arc<dyn RuntimeProvider> {
    let should_wrap = provider_state.id != provider.id()
        || provider_state.name != provider.name()
        || !provider_state.models.is_empty();

    if !should_wrap {
        return provider;
    }

    let models = if provider_state.models.is_empty() {
        provider.models()
    } else {
        provider_state
            .models
            .values()
            .map(|model| state_model_to_runtime(&provider_state.id, model))
            .collect()
    };

    Arc::new(AliasedProvider::new(
        provider_state.id.clone(),
        provider_state.name.clone(),
        provider,
        models,
    ))
}

fn load_models_dev_cache() -> ModelsData {
    let cache_path = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rocode")
        .join("models.json");

    let Ok(raw) = fs::read_to_string(cache_path) else {
        return HashMap::new();
    };

    if let Ok(parsed) = serde_json::from_str::<ModelsData>(&raw) {
        return parsed;
    }

    // Fallback: tolerate per-provider schema drift instead of dropping everything.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return HashMap::new();
    };
    let Some(map) = value.as_object() else {
        return HashMap::new();
    };

    let mut data = HashMap::new();
    for (provider_id, provider_value) in map {
        match serde_json::from_value::<ModelsProviderInfo>(provider_value.clone()) {
            Ok(mut provider) => {
                if provider.id.trim().is_empty() {
                    provider.id = provider_id.clone();
                }
                data.insert(provider_id.clone(), provider);
            }
            Err(error) => {
                tracing::debug!(
                    provider = provider_id,
                    %error,
                    "Skipping invalid provider entry from models.dev cache"
                );
            }
        }
    }

    data
}

fn register_fallback_env_providers(registry: &mut ProviderRegistry) {
    let fallback: Vec<(&str, Vec<&str>)> = vec![
        ("ethnopic", vec!["ANTHROPIC_API_KEY"]),
        ("openai", vec!["OPENAI_API_KEY"]),
        (
            "google",
            vec!["GOOGLE_API_KEY", "GOOGLE_GENERATIVE_AI_API_KEY"],
        ),
        ("azure", vec!["AZURE_API_KEY", "AZURE_OPENAI_API_KEY"]),
        (
            "amazon-bedrock",
            vec!["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"],
        ),
        ("openrouter", vec!["OPENROUTER_API_KEY"]),
        ("mistral", vec!["MISTRAL_API_KEY"]),
        ("groq", vec!["GROQ_API_KEY"]),
        ("deepseek", vec!["DEEPSEEK_API_KEY"]),
        ("xai", vec!["XAI_API_KEY"]),
        ("cerebras", vec!["CEREBRAS_API_KEY"]),
        ("cohere", vec!["COHERE_API_KEY"]),
        ("deepinfra", vec!["DEEPINFRA_API_KEY"]),
        ("together", vec!["TOGETHER_API_KEY", "TOGETHERAI_API_KEY"]),
        ("perplexity", vec!["PERPLEXITY_API_KEY"]),
        ("vercel", vec!["VERCEL_API_KEY"]),
        ("gitlab", vec!["GITLAB_TOKEN"]),
        ("github-copilot", vec!["GITHUB_COPILOT_TOKEN"]),
        (
            "google-vertex",
            vec![
                "GOOGLE_VERTEX_ACCESS_TOKEN",
                "GOOGLE_CLOUD_ACCESS_TOKEN",
                "GOOGLE_OAUTH_ACCESS_TOKEN",
                "GCP_ACCESS_TOKEN",
            ],
        ),
    ];

    for (provider_id, env_keys) in fallback {
        let state = ProviderState {
            id: provider_id.to_string(),
            name: provider_id.to_string(),
            source: "env".to_string(),
            env: env_keys.into_iter().map(|k| k.to_string()).collect(),
            key: None,
            options: HashMap::new(),
            models: HashMap::new(),
        };
        if let Some(provider) = create_concrete_provider(provider_id, &state) {
            registry.register_arc(provider);
        }
    }
}

/// Create a ProviderRegistry populated from environment variables.
/// Scans known provider env vars and registers any that are configured.
pub fn create_registry_from_env() -> ProviderRegistry {
    let auth_store: HashMap<String, AuthInfo> = HashMap::new();
    create_registry_from_env_with_auth_store(&auth_store)
}

/// Create a ProviderRegistry populated from environment variables plus explicit
/// auth store entries (for example plugin-provided auth tokens).
pub fn create_registry_from_env_with_auth_store(
    auth_store: &HashMap<String, AuthInfo>,
) -> ProviderRegistry {
    bootstrap_registry(&BootstrapConfig::default(), auth_store)
}

/// Create a ProviderRegistry using the given bootstrap config and auth store.
/// This is the primary entry point when you have a loaded application config
/// whose provider/model fields have been converted into a `BootstrapConfig`.
pub fn create_registry_from_bootstrap_config(
    config: &BootstrapConfig,
    auth_store: &HashMap<String, AuthInfo>,
) -> ProviderRegistry {
    bootstrap_registry(config, auth_store)
}

fn bootstrap_registry(
    config: &BootstrapConfig,
    auth_store: &HashMap<String, AuthInfo>,
) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    let models_dev = load_models_dev_cache();
    let state = ProviderBootstrapState::init(&models_dev, config, auth_store);

    for (provider_id, provider_state) in &state.providers {
        if let Some(provider) = create_concrete_provider(provider_id, provider_state) {
            let provider = wrap_provider_for_state(provider_state, provider);
            let registered_id = provider.id().to_string();
            registry.register_arc(provider);
            if !provider_state.options.is_empty() {
                registry.merge_config(&registered_id, provider_state.options.clone());
            }
            tracing::debug!(
                provider = provider_id,
                concrete_provider = registered_id,
                "Registered provider from bootstrap state"
            );
        } else {
            tracing::debug!(
                provider = provider_id,
                "No concrete provider implementation for bootstrap provider"
            );
        }
    }

    if registry.list().is_empty() {
        tracing::debug!(
            "No providers registered from bootstrap state, falling back to direct env registration"
        );
        register_fallback_env_providers(&mut registry);
    }

    registry
}

/// Build a `BootstrapConfig` from the raw config fields typically found in
/// `rocode_config::Config`. This bridges the gap between the config loader
/// and the provider bootstrap system.
///
/// The `providers` map should be converted from `rocode_config::ProviderConfig`
/// to `ConfigProvider` by the caller (see `config_provider_to_bootstrap` helper).
pub fn bootstrap_config_from_raw(
    providers: HashMap<String, ConfigProvider>,
    disabled_providers: Vec<String>,
    enabled_providers: Vec<String>,
    model: Option<String>,
    small_model: Option<String>,
) -> BootstrapConfig {
    BootstrapConfig {
        providers,
        disabled_providers: disabled_providers.into_iter().collect(),
        enabled_providers: if enabled_providers.is_empty() {
            None
        } else {
            Some(enabled_providers.into_iter().collect())
        },
        enable_experimental: false,
        model,
        small_model,
    }
}

/// Apply custom loaders to models data, mutating it in place.
/// This runs each provider's custom loader and applies blacklists, headers,
/// and option overrides.
pub fn apply_custom_loaders(data: &mut ModelsData) {
    let provider_ids: Vec<String> = data.keys().cloned().collect();

    for provider_id in &provider_ids {
        if let Some(loader) = get_custom_loader(provider_id) {
            let provider_info = match data.get(provider_id) {
                Some(p) => p.clone(),
                None => continue,
            };
            let result = loader.load(&provider_info, None);

            // Apply blacklist: remove models matching any blacklist pattern
            if !result.blacklist.is_empty() {
                if let Some(provider) = data.get_mut(provider_id) {
                    provider.models.retain(|mid, _| {
                        let lower = mid.to_lowercase();
                        !result.blacklist.iter().any(|pat| lower.contains(pat))
                    });
                }
            }

            // Apply headers to all models
            if !result.headers.is_empty() {
                if let Some(provider) = data.get_mut(provider_id) {
                    for model in provider.models.values_mut() {
                        let headers = model.headers.get_or_insert_with(HashMap::new);
                        for (k, v) in &result.headers {
                            headers.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Filter models by status, removing deprecated models and optionally alpha models.
pub fn filter_models_by_status(data: &mut ModelsData, enable_experimental: bool) {
    for provider in data.values_mut() {
        provider.models.retain(|_mid, model| {
            let status = model.status.as_deref().unwrap_or("active");
            // Always remove deprecated
            if status == "deprecated" {
                return false;
            }
            // Remove alpha unless experimental is enabled
            if status == "alpha" && !enable_experimental {
                return false;
            }
            true
        });
    }
    // Remove providers with no remaining models
    data.retain(|_pid, provider| !provider.models.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ModelLimit, ModelModalities, ModelProvider};

    fn provider_model(model_id: &str) -> ProviderModel {
        ProviderModel {
            id: model_id.to_string(),
            provider_id: "test".to_string(),
            name: model_id.to_string(),
            api: ProviderModelApi {
                id: model_id.to_string(),
                url: "https://example.com".to_string(),
                npm: "@ai-sdk/openai".to_string(),
            },
            family: None,
            capabilities: ModelCapabilities {
                temperature: true,
                reasoning: true,
                attachment: false,
                toolcall: true,
                input: ModalitySet {
                    text: true,
                    audio: false,
                    image: false,
                    video: false,
                    pdf: false,
                },
                output: ModalitySet {
                    text: true,
                    audio: false,
                    image: false,
                    video: false,
                    pdf: false,
                },
                interleaved: InterleavedConfig::Bool(false),
            },
            cost: ProviderModelCost {
                input: 0.0,
                output: 0.0,
                cache: ModelCostCache {
                    read: 0.0,
                    write: 0.0,
                },
                experimental_over_200k: None,
            },
            limit: ProviderModelLimit {
                context: 128_000,
                input: None,
                output: 8_192,
            },
            status: "active".to_string(),
            options: HashMap::new(),
            headers: HashMap::new(),
            release_date: "2026-01-01".to_string(),
            variants: None,
        }
    }

    fn model_info(model_id: &str) -> ModelInfo {
        ModelInfo {
            id: model_id.to_string(),
            name: model_id.to_string(),
            family: None,
            release_date: Some("2026-01-01".to_string()),
            attachment: false,
            reasoning: true,
            temperature: true,
            tool_call: true,
            interleaved: Some(ModelInterleaved::Bool(false)),
            cost: None,
            limit: ModelLimit {
                context: 128_000,
                input: None,
                output: 8_192,
            },
            modalities: Some(ModelModalities {
                input: vec!["text".to_string()],
                output: vec!["text".to_string()],
            }),
            experimental: None,
            status: Some("active".to_string()),
            options: HashMap::new(),
            headers: None,
            provider: Some(ModelProvider {
                npm: Some("@ai-sdk/openai".to_string()),
                api: Some("https://api.openai.com/v1".to_string()),
            }),
            variants: None,
        }
    }

    fn provider_info(provider_id: &str, model: ModelInfo) -> ModelsProviderInfo {
        let mut models = HashMap::new();
        models.insert(model.id.clone(), model);
        ModelsProviderInfo {
            api: Some("https://example.com".to_string()),
            name: provider_id.to_string(),
            env: vec![],
            id: provider_id.to_string(),
            npm: Some("@ai-sdk/openai".to_string()),
            models,
        }
    }

    fn provider_state(id: &str) -> ProviderState {
        ProviderState {
            id: id.to_string(),
            name: id.to_string(),
            source: "env".to_string(),
            env: vec![],
            key: None,
            options: HashMap::new(),
            models: HashMap::new(),
        }
    }

    #[test]
    fn creates_openai_provider_from_state_key() {
        let mut state = provider_state("openai");
        state.key = Some("test-key".to_string());

        let provider = create_concrete_provider("openai", &state).expect("provider should exist");
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn azure_provider_requires_endpoint() {
        let mut state = provider_state("azure");
        state.key = Some("test-key".to_string());
        assert!(create_concrete_provider("azure", &state).is_none());

        state.options.insert(
            "endpoint".to_string(),
            serde_json::Value::String("https://example.openai.azure.com".to_string()),
        );
        let provider = create_concrete_provider("azure", &state).expect("provider should exist");
        assert_eq!(provider.id(), "azure");
    }

    #[test]
    fn creates_bedrock_provider_from_options() {
        let mut state = provider_state("amazon-bedrock");
        state.options.insert(
            "accessKeyId".to_string(),
            serde_json::Value::String("akid".to_string()),
        );
        state.options.insert(
            "secretAccessKey".to_string(),
            serde_json::Value::String("secret".to_string()),
        );
        state.options.insert(
            "region".to_string(),
            serde_json::Value::String("us-east-1".to_string()),
        );

        let provider =
            create_concrete_provider("amazon-bedrock", &state).expect("provider should exist");
        assert_eq!(provider.id(), "amazon-bedrock");
    }

    #[test]
    fn sort_models_prioritizes_big_pickle_over_non_priority_models() {
        let mut models = vec![
            provider_model("my-custom-model"),
            provider_model("big-pickle-v2"),
        ];
        ProviderBootstrapState::sort_models(&mut models);
        assert_eq!(models[0].id, "big-pickle-v2");
    }

    #[test]
    fn apply_custom_loaders_applies_zenmux_headers() {
        let model = model_info("zenmux-model");
        let mut data = HashMap::new();
        data.insert("zenmux".to_string(), provider_info("zenmux", model));

        apply_custom_loaders(&mut data);

        let provider = data.get("zenmux").expect("zenmux provider should exist");
        let model = provider
            .models
            .get("zenmux-model")
            .expect("zenmux model should exist");
        let headers = model.headers.as_ref().expect("headers should be set");
        assert_eq!(
            headers.get("HTTP-Referer").map(String::as_str),
            Some("https://opencode.ai/")
        );
        assert_eq!(headers.get("X-Title").map(String::as_str), Some("opencode"));
    }

    #[test]
    fn bedrock_loader_reads_provider_state_options() {
        let loader = AmazonBedrockLoader;
        let mut state = provider_state("amazon-bedrock");
        state.options.insert(
            "region".to_string(),
            serde_json::Value::String("us-west-2".to_string()),
        );
        state.options.insert(
            "profile".to_string(),
            serde_json::Value::String("dev-profile".to_string()),
        );
        state.options.insert(
            "endpoint".to_string(),
            serde_json::Value::String("https://bedrock.internal".to_string()),
        );

        let result = loader.load(
            &provider_info("amazon-bedrock", model_info("test-vendor.test-model-large")),
            Some(&state),
        );
        assert!(result.autoload);
        assert_eq!(
            result.options.get("region"),
            Some(&serde_json::Value::String("us-west-2".to_string()))
        );
        assert_eq!(
            result.options.get("profile"),
            Some(&serde_json::Value::String("dev-profile".to_string()))
        );
        assert_eq!(
            result.options.get("endpoint"),
            Some(&serde_json::Value::String(
                "https://bedrock.internal".to_string()
            ))
        );
        assert!(result.has_custom_get_model);
    }

    #[test]
    fn from_models_dev_model_merges_transform_and_explicit_variants() {
        let mut model = model_info("gpt-5");
        let mut explicit = HashMap::new();
        explicit.insert(
            "custom".to_string(),
            HashMap::from([(
                "reasoningEffort".to_string(),
                serde_json::Value::String("custom".to_string()),
            )]),
        );
        model.variants = Some(explicit);

        let provider = provider_info("openai", model.clone());
        let runtime_model = from_models_dev_model(&provider, &model);
        let variants = runtime_model
            .variants
            .expect("variants should include generated and explicit values");
        assert!(variants.contains_key("custom"));
        assert!(variants.contains_key("low"));
    }
}
