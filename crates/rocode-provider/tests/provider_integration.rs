use rocode_provider::{
    ChatRequest, Message, ModelInfo, Protocol, ProviderConfig, ProviderInstance, ProviderRegistry,
};
use std::collections::HashMap;

fn create_test_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();

    // Anthropic via protocol
    let anthropic_models: HashMap<String, ModelInfo> = vec![
        ModelInfo {
            id: "claude-sonnet-4-20250514".to_string(),
            name: "Claude Sonnet 4".to_string(),
            provider: "anthropic".to_string(),
            context_window: 200000,
            max_input_tokens: None,
            max_output_tokens: 16000,
            supports_vision: true,
            supports_tools: true,
            cost_per_million_input: 3.0,
            cost_per_million_output: 15.0,
        },
        ModelInfo {
            id: "claude-3-5-sonnet-20241022".to_string(),
            name: "Claude 3.5 Sonnet".to_string(),
            provider: "anthropic".to_string(),
            context_window: 200000,
            max_input_tokens: None,
            max_output_tokens: 8192,
            supports_vision: true,
            supports_tools: true,
            cost_per_million_input: 3.0,
            cost_per_million_output: 15.0,
        },
    ]
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect();

    registry.register(ProviderInstance::new(
        "anthropic".to_string(),
        "Anthropic".to_string(),
        ProviderConfig::new("anthropic", "", "test-key"),
        rocode_provider::create_protocol_impl(Protocol::Anthropic),
        anthropic_models,
    ));

    // OpenAI via protocol
    let openai_models: HashMap<String, ModelInfo> = vec![ModelInfo {
        id: "gpt-4o".to_string(),
        name: "GPT-4o".to_string(),
        provider: "openai".to_string(),
        context_window: 128000,
        max_input_tokens: None,
        max_output_tokens: 16384,
        supports_vision: true,
        supports_tools: true,
        cost_per_million_input: 2.5,
        cost_per_million_output: 10.0,
    }]
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect();

    registry.register(ProviderInstance::new(
        "openai".to_string(),
        "OpenAI".to_string(),
        ProviderConfig::new("openai", "", "test-key"),
        rocode_provider::create_protocol_impl(Protocol::OpenAI),
        openai_models,
    ));

    // Google via protocol
    let google_models: HashMap<String, ModelInfo> = vec![ModelInfo {
        id: "gemini-2.0-flash".to_string(),
        name: "Gemini 2.0 Flash".to_string(),
        provider: "google".to_string(),
        context_window: 1_000_000,
        max_input_tokens: None,
        max_output_tokens: 8192,
        supports_vision: true,
        supports_tools: true,
        cost_per_million_input: 0.1,
        cost_per_million_output: 0.4,
    }]
    .into_iter()
    .map(|m| (m.id.clone(), m))
    .collect();

    registry.register(ProviderInstance::new(
        "google".to_string(),
        "Google AI".to_string(),
        ProviderConfig::new("google", "", "test-key"),
        rocode_provider::create_protocol_impl(Protocol::Google),
        google_models,
    ));

    registry
}

#[test]
fn test_registry_lists_providers() {
    let registry = create_test_registry();
    let providers = registry.list_providers();

    assert!(!providers.is_empty(), "Registry should have providers");

    let provider_ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
    assert!(
        provider_ids.contains(&"anthropic"),
        "Should have anthropic provider"
    );
    assert!(
        provider_ids.contains(&"openai"),
        "Should have openai provider"
    );
    assert!(
        provider_ids.contains(&"google"),
        "Should have google provider"
    );
}

#[test]
fn test_registry_lists_models() {
    let registry = create_test_registry();
    let models = registry.list_models();

    assert!(!models.is_empty(), "Registry should have models");

    let model_ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert!(
        model_ids.iter().any(|id| id.contains("claude")),
        "Should have Claude models"
    );
    assert!(
        model_ids.iter().any(|id| id.contains("gpt")),
        "Should have GPT models"
    );
}

#[test]
fn test_find_model_by_id() {
    let registry = create_test_registry();

    let result = registry.find_model("claude-3-5-sonnet-20241022");
    assert!(result.is_some(), "Should find claude-3-5-sonnet model");

    let (provider_id, model) = result.unwrap();
    assert_eq!(provider_id, "anthropic");
    assert!(model.supports_vision);
    assert!(model.supports_tools);
}

#[test]
fn test_provider_metadata() {
    let registry = create_test_registry();

    let anthropic = registry.get("anthropic");
    assert!(anthropic.is_some());
    let provider = anthropic.unwrap();

    assert_eq!(provider.id(), "anthropic");
    assert_eq!(provider.name(), "Anthropic");

    let models = provider.models();
    assert!(!models.is_empty());

    let claude = provider.get_model("claude-3-5-sonnet-20241022");
    assert!(claude.is_some());
}

#[test]
fn test_chat_request_builder() {
    let request = ChatRequest::new(
        "gpt-4o",
        vec![Message::system("You are helpful"), Message::user("Hello")],
    )
    .with_temperature(0.7)
    .with_max_tokens(1000)
    .with_stream(true);

    assert_eq!(request.model, "gpt-4o");
    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_tokens, Some(1000));
    assert_eq!(request.stream, Some(true));
}

#[test]
fn test_model_info_clone() {
    let model = ModelInfo {
        id: "test-model".to_string(),
        name: "Test Model".to_string(),
        provider: "test".to_string(),
        context_window: 128000,
        max_input_tokens: None,
        max_output_tokens: 4096,
        supports_vision: true,
        supports_tools: true,
        cost_per_million_input: 1.0,
        cost_per_million_output: 2.0,
    };

    let cloned = model.clone();
    assert_eq!(cloned.id, model.id);
    assert_eq!(cloned.context_window, model.context_window);
}

#[test]
fn test_all_providers_have_models() {
    let registry = create_test_registry();

    for provider in registry.list() {
        let models = provider.models();
        assert!(
            !models.is_empty(),
            "Provider {} should have models",
            provider.id()
        );
    }
}
