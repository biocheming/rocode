use rocode_provider::bootstrap::{
    bootstrap_config_from_raw, create_registry_from_bootstrap_config,
};
use std::collections::HashMap;

#[test]
fn test_provider_registry_snapshot() {
    // Create minimal bootstrap config to trigger provider registration.
    //
    // This test runs in developer machines/CI where provider-related env vars
    // may be present (for example `ANTHROPIC_API_KEY`). To keep the snapshot
    // stable, restrict the enabled provider set to `opencode`.
    let config = bootstrap_config_from_raw(
        HashMap::new(),
        vec![],
        vec!["opencode".to_string()],
        None,
        None,
    );
    let auth_store = HashMap::new();
    let registry = create_registry_from_bootstrap_config(&config, &auth_store);

    // Extract registered provider IDs
    let mut registered_ids: Vec<String> =
        registry.list().iter().map(|p| p.id().to_string()).collect();
    registered_ids.sort();

    // With only `opencode` enabled, the custom-loader-backed opencode provider
    // should autoload. Other providers are filtered out even if env vars exist.
    assert_eq!(registered_ids, vec!["opencode".to_string()]);
}
