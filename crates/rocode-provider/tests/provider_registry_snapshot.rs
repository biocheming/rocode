use rocode_provider::bootstrap::{
    bootstrap_config_from_raw, create_registry_from_bootstrap_config,
};
use std::collections::{HashMap, HashSet};

#[test]
fn test_provider_registry_snapshot() {
    // Create minimal bootstrap config to trigger provider registration
    let config = bootstrap_config_from_raw(HashMap::new(), vec![], vec![], None, None);
    let auth_store = HashMap::new();
    let registry = create_registry_from_bootstrap_config(&config, &auth_store);

    // Extract registered provider IDs
    let mut registered_ids: Vec<String> =
        registry.list().iter().map(|p| p.id().to_string()).collect();
    registered_ids.sort();

    // Expected providers that should be registered with empty config
    // This is the baseline set that registers without explicit config/auth.
    // Full provider list (anthropic, openai, azure, bedrock, vertex, gitlab,
    // copilot, mistral, deepinfra, deepseek, xai, cohere, together, perplexity,
    // vercel) requires proper bootstrap config with auth credentials.
    // This test locks down the minimal registration set to prevent regression.
    let expected_providers: HashSet<&str> = ["google", "groq", "opencode", "openrouter"]
        .iter()
        .cloned()
        .collect();

    // Check that all expected providers are present
    let registered_set: HashSet<&str> = registered_ids.iter().map(|s| s.as_str()).collect();

    let missing: Vec<&str> = expected_providers
        .difference(&registered_set)
        .cloned()
        .collect();
    let unexpected: Vec<&str> = registered_set
        .difference(&expected_providers)
        .cloned()
        .collect();

    if !missing.is_empty() {
        panic!(
            "Missing expected providers: {:?}\nRegistered: {:?}",
            missing, registered_ids
        );
    }

    if !unexpected.is_empty() {
        eprintln!(
            "⚠️  Unexpected providers found: {:?}\nIf this is intentional, update the snapshot.",
            unexpected
        );
    }

    println!("✅ Provider registry snapshot validated");
    println!("   Registered providers: {:?}", registered_ids);
}
