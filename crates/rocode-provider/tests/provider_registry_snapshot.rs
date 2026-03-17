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

    // With no explicit config/auth, the custom-loader-backed `opencode` provider
    // must always be available.
    //
    // Other providers may also appear if the test environment has credentials
    // set via environment variables (for example on developer machines).
    assert!(
        registered_ids.contains(&"opencode".to_string()),
        "expected registry to include `opencode`, got: {registered_ids:?}"
    );
}
