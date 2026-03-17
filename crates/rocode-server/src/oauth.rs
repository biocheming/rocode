use std::collections::HashMap;
use std::sync::Arc;

use rocode_plugin::subprocess::PluginLoader;
use rocode_provider::{AuthError, AuthInfo, AuthManager, AuthMethodType, Authorization};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethodInfo {
    #[serde(rename = "type")]
    pub method_type: String,
    pub label: String,
}

pub struct ProviderAuth {
    auth_manager: Arc<AuthManager>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AuthCallbackWire {
    #[serde(
        default,
        rename = "type",
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    kind: Option<String>,
    #[serde(
        default,
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    provider: Option<String>,
    #[serde(
        default,
        alias = "apiKey",
        alias = "token",
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    key: Option<String>,
    #[serde(
        default,
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    access: Option<String>,
    #[serde(
        default,
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    refresh: Option<String>,
    #[serde(default, deserialize_with = "rocode_types::deserialize_opt_i64_lossy")]
    expires: Option<i64>,
    #[serde(
        default,
        alias = "accountId",
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    account_id: Option<String>,
    #[serde(
        default,
        alias = "enterpriseUrl",
        alias = "enterpriseURL",
        deserialize_with = "rocode_types::deserialize_opt_string_lossy"
    )]
    enterprise_url: Option<String>,
}

impl ProviderAuth {
    pub fn new(auth_manager: Arc<AuthManager>) -> Self {
        Self { auth_manager }
    }

    pub async fn methods(loader: &PluginLoader) -> HashMap<String, Vec<AuthMethodInfo>> {
        let bridges = loader.auth_bridges().await;
        bridges
            .iter()
            .map(|(provider, bridge)| {
                let methods = bridge
                    .methods()
                    .iter()
                    .map(|method| AuthMethodInfo {
                        method_type: method.method_type.clone(),
                        label: method.label.clone(),
                    })
                    .collect::<Vec<_>>();
                (provider.clone(), methods)
            })
            .collect()
    }

    pub async fn authorize(
        loader: &PluginLoader,
        provider_id: &str,
        method: usize,
        inputs: Option<HashMap<String, String>>,
    ) -> Result<Authorization, AuthError> {
        let bridge = loader
            .auth_bridge(provider_id)
            .await
            .ok_or_else(|| AuthError::OauthMissing(provider_id.to_string()))?;
        let result = bridge
            .authorize(method, inputs)
            .await
            .map_err(|_| AuthError::OauthCallbackFailed)?;

        let method_type = match result.method.as_deref() {
            Some("code") => AuthMethodType::Code,
            _ => AuthMethodType::Auto,
        };

        Ok(Authorization {
            url: result.url.unwrap_or_default(),
            method: method_type,
            instructions: result.instructions.unwrap_or_default(),
        })
    }

    pub async fn callback(
        &self,
        loader: &PluginLoader,
        provider_id: &str,
        code: Option<&str>,
    ) -> Result<(), AuthError> {
        let bridge = loader
            .auth_bridge(provider_id)
            .await
            .ok_or_else(|| AuthError::OauthMissing(provider_id.to_string()))?;
        let result = bridge
            .callback(code)
            .await
            .map_err(|_| AuthError::OauthCallbackFailed)?;

        let wire: AuthCallbackWire = serde_json::from_value(result).unwrap_or_default();

        if wire.kind.as_deref() != Some("success") {
            return Err(AuthError::OauthCallbackFailed);
        }

        // Plugin callback can override target provider (e.g. copilot enterprise).
        let target_provider = wire.provider.as_deref().unwrap_or(provider_id);

        if let Some(key) = wire.key.as_deref() {
            self.auth_manager
                .set(
                    target_provider,
                    AuthInfo::Api {
                        key: key.to_string(),
                    },
                )
                .await;
            return Ok(());
        }

        let access = wire.access.unwrap_or_default();
        let refresh = wire.refresh.unwrap_or_default();

        if access.is_empty() && refresh.is_empty() {
            return Err(AuthError::OauthCallbackFailed);
        }

        self.auth_manager
            .set(
                target_provider,
                AuthInfo::OAuth {
                    access,
                    refresh,
                    expires: wire.expires,
                    account_id: wire.account_id,
                    enterprise_url: wire.enterprise_url,
                },
            )
            .await;

        Ok(())
    }

    pub async fn set_api_key(&self, provider_id: &str, key: String) {
        self.auth_manager
            .set(provider_id, AuthInfo::Api { key })
            .await;
    }

    pub async fn remove(&self, provider_id: &str) {
        self.auth_manager.remove(provider_id).await;
    }
}
