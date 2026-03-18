use rocode_provider::bootstrap::{
    bootstrap_config_from_raw, create_registry_from_bootstrap_config,
};
use std::collections::HashMap;

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

    // With no explicit config or auth store entries, the custom-loader-backed
    // opencode provider should always autoload.
    //
    // Note: The registry may still include additional providers when the test
    // process inherits provider credentials via environment variables.
    assert!(
        registered_ids.iter().any(|id| id == "opencode"),
        "expected opencode provider to be registered; got {registered_ids:?}"
    );
}
