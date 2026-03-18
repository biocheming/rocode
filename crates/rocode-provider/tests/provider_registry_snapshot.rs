use rocode_provider::bootstrap::{
    bootstrap_config_from_raw, create_registry_from_bootstrap_config,
};
use std::collections::HashMap;

#[test]
fn test_provider_registry_snapshot() {
    // Create minimal bootstrap config to trigger provider registration
    // Hermetic snapshot: explicitly allowlist only the custom-loader-backed
    // `opencode` provider so local dev env vars (API keys) can't make this test
    // flaky by auto-registering additional providers.
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

    // With only `opencode` enabled, the registry should contain exactly one provider.
    assert_eq!(registered_ids, vec!["opencode".to_string()]);
}
