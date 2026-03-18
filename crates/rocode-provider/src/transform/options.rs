use std::collections::HashMap;

use crate::models;
use serde::Deserialize;

use super::model_config::sdk_key;
use super::normalize::slug_override;

fn deserialize_opt_bool_lossy<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => value.as_i64().map(|value| value != 0),
        Some(serde_json::Value::String(value)) => {
            match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        }
        _ => None,
    })
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeOptionsWire {
    #[serde(
        rename = "setCacheKey",
        alias = "set_cache_key",
        default,
        deserialize_with = "deserialize_opt_bool_lossy"
    )]
    set_cache_key: Option<bool>,
}

fn provider_runtime_options_wire(
    provider_options: &HashMap<String, serde_json::Value>,
) -> RuntimeOptionsWire {
    serde_json::from_value::<RuntimeOptionsWire>(serde_json::Value::Object(
        provider_options.clone().into_iter().collect(),
    ))
    .unwrap_or_default()
}

pub fn options(
    provider_id: &str,
    model: &models::ModelInfo,
    session_id: &str,
    provider_options: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    use serde_json::json;
    let mut result = HashMap::new();

    let npm = model
        .provider
        .as_ref()
        .and_then(|p| p.npm.as_deref())
        .unwrap_or("");
    let api_id = model
        .provider
        .as_ref()
        .and_then(|p| p.api.as_deref())
        .unwrap_or("");
    let provider_id = provider_id.to_ascii_lowercase();

    // OpenAI store=false
    if provider_id == "openai" || npm == "@ai-sdk/openai" || npm == "@ai-sdk/github-copilot" {
        result.insert("store".to_string(), json!(false));
    }

    // OpenRouter usage include
    if npm == "@openrouter/ai-sdk-provider" {
        result.insert("usage".to_string(), json!({"include": true}));
        if api_id.contains("gemini-3") {
            result.insert("reasoning".to_string(), json!({"effort": "high"}));
        }
    }

    // Baseten / opencode chat_template_args
    if provider_id == "baseten"
        || (provider_id.starts_with("opencode")
            && (api_id == "kimi-k2-thinking" || api_id == "glm-4.6"))
    {
        result.insert(
            "chat_template_args".to_string(),
            json!({"enable_thinking": true}),
        );
    }

    // zai/zhipuai thinking config
    if (provider_id == "zai" || provider_id == "zhipuai") && npm == "@ai-sdk/openai-compatible" {
        result.insert(
            "thinking".to_string(),
            json!({"type": "enabled", "clear_thinking": false}),
        );
    }

    // OpenAI prompt cache key
    let runtime_options = provider_runtime_options_wire(provider_options);
    if provider_id == "openai" || runtime_options.set_cache_key.unwrap_or(false) {
        result.insert("promptCacheKey".to_string(), json!(session_id));
    }

    // Google thinking config
    if npm == "@ai-sdk/google" || npm == "@ai-sdk/google-vertex" {
        let mut thinking = json!({"includeThoughts": true});
        if api_id.contains("gemini-3") {
            thinking["thinkingLevel"] = json!("high");
        }
        result.insert("thinkingConfig".to_string(), thinking);
    }

    // Anthropic thinking for kimi-k2.5/k2p5 models
    let api_id_lower = api_id.to_lowercase();
    if (npm == "@ai-sdk/anthropic" || npm == "@ai-sdk/google-vertex/anthropic")
        && (api_id_lower.contains("k2p5")
            || api_id_lower.contains("kimi-k2.5")
            || api_id_lower.contains("kimi-k2p5"))
    {
        let budget = 16_000u64.min(model.limit.output / 2 - 1);
        result.insert(
            "thinking".to_string(),
            json!({"type": "enabled", "budgetTokens": budget}),
        );
    }

    // Alibaba-cn enable_thinking
    if provider_id == "alibaba-cn"
        && model.reasoning
        && npm == "@ai-sdk/openai-compatible"
        && !api_id_lower.contains("kimi-k2-thinking")
    {
        result.insert("enable_thinking".to_string(), json!(true));
    }

    // GPT-5 reasoning effort/summary/verbosity
    if api_id.contains("gpt-5") && !api_id.contains("gpt-5-chat") {
        if !api_id.contains("gpt-5-pro") {
            result.insert("reasoningEffort".to_string(), json!("medium"));
            result.insert("reasoningSummary".to_string(), json!("auto"));
        }

        // textVerbosity for non-chat gpt-5.x models
        if api_id.contains("gpt-5.")
            && !api_id.contains("codex")
            && !api_id.contains("-chat")
            && provider_id != "azure"
        {
            result.insert("textVerbosity".to_string(), json!("low"));
        }

        if provider_id.starts_with("opencode") {
            result.insert("promptCacheKey".to_string(), json!(session_id));
            result.insert(
                "include".to_string(),
                json!(["reasoning.encrypted_content"]),
            );
            result.insert("reasoningSummary".to_string(), json!("auto"));
        }
    }

    // Venice promptCacheKey
    if provider_id == "venice" {
        result.insert("promptCacheKey".to_string(), json!(session_id));
    }

    // OpenRouter prompt_cache_key
    if provider_id == "openrouter" {
        result.insert("prompt_cache_key".to_string(), json!(session_id));
    }

    // Gateway caching
    if npm == "@ai-sdk/gateway" {
        result.insert("gateway".to_string(), json!({"caching": "auto"}));
    }

    result
}

// ---------------------------------------------------------------------------
// small_options
// ---------------------------------------------------------------------------

/// Generate small model options (reduced reasoning effort).
pub fn small_options(model: &models::ModelInfo) -> HashMap<String, serde_json::Value> {
    use serde_json::json;
    let mut result = HashMap::new();

    let npm = model
        .provider
        .as_ref()
        .and_then(|p| p.npm.as_deref())
        .unwrap_or("");
    let api_id = model
        .provider
        .as_ref()
        .and_then(|p| p.api.as_deref())
        .unwrap_or("");
    let provider_id = model.id.to_lowercase();

    if provider_id == "openai" || npm == "@ai-sdk/openai" || npm == "@ai-sdk/github-copilot" {
        result.insert("store".to_string(), json!(false));
        if api_id.contains("gpt-5") {
            if api_id.contains("5.") {
                result.insert("reasoningEffort".to_string(), json!("low"));
            } else {
                result.insert("reasoningEffort".to_string(), json!("minimal"));
            }
        }
        return result;
    }

    if provider_id == "google" {
        // gemini-3 uses thinkingLevel, gemini-2.5 uses thinkingBudget
        if api_id.contains("gemini-3") {
            result.insert(
                "thinkingConfig".to_string(),
                json!({"thinkingLevel": "minimal"}),
            );
        } else {
            result.insert("thinkingConfig".to_string(), json!({"thinkingBudget": 0}));
        }
        return result;
    }

    if provider_id == "openrouter" {
        if api_id.contains("google") {
            result.insert("reasoning".to_string(), json!({"enabled": false}));
        } else {
            result.insert("reasoningEffort".to_string(), json!("minimal"));
        }
        return result;
    }

    result
}

// ---------------------------------------------------------------------------
// schema (Gemini schema sanitization)
// ---------------------------------------------------------------------------

/// Sanitize a JSON schema for Gemini/Google models.
/// - Convert integer enums to string enums
/// - Recursive sanitization of nested objects/arrays
/// - Filter required array to only include fields in properties
/// - Remove properties/required from non-object types
/// - Handle empty array items
pub fn schema(model: &models::ModelInfo, input_schema: serde_json::Value) -> serde_json::Value {
    let provider_id = model.id.to_lowercase();
    let api_id = model
        .provider
        .as_ref()
        .and_then(|p| p.api.as_deref())
        .unwrap_or("");

    if provider_id == "google" || api_id.contains("gemini") {
        sanitize_gemini(input_schema)
    } else {
        input_schema
    }
}

fn sanitize_gemini(obj: serde_json::Value) -> serde_json::Value {
    use serde_json::{json, Map, Value};

    #[derive(Debug, Default, Deserialize)]
    struct GeminiSchemaNodeWire {
        #[serde(rename = "type", default)]
        schema_type: Option<String>,
        #[serde(rename = "enum", default)]
        enum_values: Option<Vec<Value>>,
        #[serde(default)]
        properties: Option<Map<String, Value>>,
        #[serde(default)]
        required: Option<Vec<Value>>,
        #[serde(default)]
        items: Option<Value>,
    }

    fn gemini_schema_node_wire(map: &Map<String, Value>) -> GeminiSchemaNodeWire {
        serde_json::from_value::<GeminiSchemaNodeWire>(Value::Object(map.clone()))
            .unwrap_or_default()
    }

    match obj {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => obj,
        Value::Array(arr) => Value::Array(arr.into_iter().map(sanitize_gemini).collect()),
        Value::Object(map) => {
            let mut result = Map::new();

            for (key, value) in map {
                if key == "enum" {
                    if let Value::Array(ref enum_vals) = value {
                        // Convert all enum values to strings
                        let string_vals: Vec<Value> = enum_vals
                            .iter()
                            .map(|v| match v {
                                Value::String(s) => Value::String(s.clone()),
                                other => Value::String(other.to_string()),
                            })
                            .collect();
                        result.insert(key, Value::Array(string_vals));
                    } else {
                        result.insert(key, value);
                    }
                } else if value.is_object() || value.is_array() {
                    result.insert(key, sanitize_gemini(value));
                } else {
                    result.insert(key, value);
                }
            }

            let wire = gemini_schema_node_wire(&result);

            // If we have integer/number type with enum, change to string.
            if wire
                .enum_values
                .as_ref()
                .is_some_and(|enum_values| !enum_values.is_empty())
                && wire
                    .schema_type
                    .as_deref()
                    .is_some_and(|schema_type| matches!(schema_type, "integer" | "number"))
            {
                result.insert("type".to_string(), Value::String("string".to_string()));
            }

            // Filter required array to only include fields in properties
            if wire.schema_type.as_deref() == Some("object") {
                if let (Some(props), Some(required)) =
                    (wire.properties.as_ref(), wire.required.as_ref())
                {
                    let filtered: Vec<Value> = required
                        .iter()
                        .filter(|r| {
                            if let Value::String(field) = r {
                                props.contains_key(field)
                            } else {
                                false
                            }
                        })
                        .cloned()
                        .collect();
                    result.insert("required".to_string(), Value::Array(filtered));
                }
            }

            // Handle array items
            if wire.schema_type.as_deref() == Some("array") {
                if wire.items.is_none() || wire.items.as_ref().is_some_and(Value::is_null) {
                    result.insert("items".to_string(), json!({}));
                }
                // Ensure items has at least a type if it's an empty object
                if let Some(Value::Object(items)) = result.get_mut("items") {
                    if !items.contains_key("type") {
                        items.insert("type".to_string(), Value::String("string".to_string()));
                    }
                }
            }

            // Remove properties/required from non-object types
            if let Some(schema_type) = wire.schema_type.as_deref() {
                if schema_type != "object" {
                    result.remove("properties");
                    result.remove("required");
                }
            }

            Value::Object(result)
        }
    }
}

// ---------------------------------------------------------------------------
// provider_options_map (matches TS providerOptions())
// ---------------------------------------------------------------------------

/// Convert provider options to the format expected by the SDK.
/// For gateway, splits options across gateway and upstream provider namespaces.
/// For other providers, wraps under the SDK key.
pub fn provider_options_map(
    model: &models::ModelInfo,
    opts: HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    if opts.is_empty() {
        return opts;
    }

    let npm = model
        .provider
        .as_ref()
        .and_then(|p| p.npm.as_deref())
        .unwrap_or("");
    let api_id = model
        .provider
        .as_ref()
        .and_then(|p| p.api.as_deref())
        .unwrap_or("");
    let provider_id = model.id.to_lowercase();

    if npm == "@ai-sdk/gateway" {
        // Gateway providerOptions are split across two namespaces:
        // - `gateway`: gateway-native routing/caching controls
        // - `<upstream slug>`: provider-specific model options
        let i = api_id.find('/');
        let raw_slug = if let Some(pos) = i {
            if pos > 0 {
                Some(&api_id[..pos])
            } else {
                None
            }
        } else {
            None
        };
        let slug = raw_slug.map(|s| slug_override(s).unwrap_or(s));

        let gateway = opts.get("gateway").cloned();
        let rest: HashMap<String, serde_json::Value> =
            opts.into_iter().filter(|(k, _)| k != "gateway").collect();
        let has_rest = !rest.is_empty();

        let mut result: HashMap<String, serde_json::Value> = HashMap::new();
        if let Some(gw) = gateway.clone() {
            result.insert("gateway".to_string(), gw);
        }

        if has_rest {
            if let Some(slug) = slug {
                result.insert(
                    slug.to_string(),
                    serde_json::to_value(&rest).unwrap_or_default(),
                );
            } else if let Some(ref gw) = gateway {
                if gw.is_object() {
                    let mut merged = gw.clone();
                    if let Some(obj) = merged.as_object_mut() {
                        for (k, v) in &rest {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                    result.insert("gateway".to_string(), merged);
                } else {
                    result.insert(
                        "gateway".to_string(),
                        serde_json::to_value(&rest).unwrap_or_default(),
                    );
                }
            } else {
                result.insert(
                    "gateway".to_string(),
                    serde_json::to_value(&rest).unwrap_or_default(),
                );
            }
        }

        return result;
    }

    let key = sdk_key(npm)
        .map(|s: &str| s.to_string())
        .unwrap_or_else(|| provider_id.clone());
    let mut result = HashMap::new();
    result.insert(key, serde_json::json!(opts));
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
