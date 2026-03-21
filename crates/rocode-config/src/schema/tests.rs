use super::*;

#[test]
fn merges_nested_structs_without_losing_existing_fields() {
    let mut base = Config {
        keybinds: Some(KeybindsConfig {
            submit: Some("enter".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let overlay = Config {
        keybinds: Some(KeybindsConfig {
            interrupt: Some("esc".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    base.merge(overlay);

    let merged = base.keybinds.unwrap();
    assert_eq!(merged.submit, Some("enter".to_string()));
    assert_eq!(merged.interrupt, Some("esc".to_string()));
}

#[test]
fn merges_maps_recursively_for_same_keys() {
    let mut base = Config {
        provider: Some(HashMap::from([(
            "openai".to_string(),
            ProviderConfig {
                base_url: Some("https://old".to_string()),
                models: Some(HashMap::from([(
                    "gpt-4o".to_string(),
                    ModelConfig {
                        api_key: Some("old-key".to_string()),
                        ..Default::default()
                    },
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    };

    let overlay = Config {
        provider: Some(HashMap::from([(
            "openai".to_string(),
            ProviderConfig {
                api_key: Some("new-provider-key".to_string()),
                models: Some(HashMap::from([(
                    "gpt-4o".to_string(),
                    ModelConfig {
                        model: Some("gpt-4o-2026".to_string()),
                        ..Default::default()
                    },
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    };

    base.merge(overlay);

    let provider = base.provider.unwrap().remove("openai").unwrap();
    assert_eq!(provider.base_url, Some("https://old".to_string()));
    assert_eq!(provider.api_key, Some("new-provider-key".to_string()));

    let model = provider.models.unwrap().remove("gpt-4o").unwrap();
    assert_eq!(model.api_key, Some("old-key".to_string()));
    assert_eq!(model.model, Some("gpt-4o-2026".to_string()));
}

#[test]
fn docs_config_merge_replaces_registry_path() {
    let mut base = Config {
        docs: Some(DocsConfig {
            context_docs_registry_path: Some("docs/base-registry.json".to_string()),
        }),
        ..Default::default()
    };

    let overlay = Config {
        docs: Some(DocsConfig {
            context_docs_registry_path: Some("docs/override-registry.json".to_string()),
        }),
        ..Default::default()
    };

    base.merge(overlay);

    assert_eq!(
        base.docs.and_then(|docs| docs.context_docs_registry_path),
        Some("docs/override-registry.json".to_string())
    );
}

#[test]
fn plugin_map_merge_and_instruction_arrays_append_unique() {
    let mut base = Config {
        plugin: HashMap::from([
            (
                "a".to_string(),
                PluginConfig {
                    plugin_type: "npm".to_string(),
                    package: Some("a".to_string()),
                    ..Default::default()
                },
            ),
            (
                "b".to_string(),
                PluginConfig {
                    plugin_type: "npm".to_string(),
                    package: Some("b".to_string()),
                    ..Default::default()
                },
            ),
        ]),
        instructions: vec!["one".to_string(), "two".to_string()],
        ..Default::default()
    };

    let overlay = Config {
        plugin: HashMap::from([
            (
                "b".to_string(),
                PluginConfig {
                    plugin_type: "npm".to_string(),
                    package: Some("b-updated".to_string()),
                    ..Default::default()
                },
            ),
            (
                "c".to_string(),
                PluginConfig {
                    plugin_type: "npm".to_string(),
                    package: Some("c".to_string()),
                    ..Default::default()
                },
            ),
        ]),
        instructions: vec!["two".to_string(), "three".to_string()],
        ..Default::default()
    };

    base.merge(overlay);

    // plugin map: 3 entries, "b" overwritten by overlay
    assert_eq!(base.plugin.len(), 3);
    assert_eq!(base.plugin["b"].package.as_deref(), Some("b-updated"));
    assert!(base.plugin.contains_key("c"));
    assert_eq!(
        base.instructions,
        vec!["one".to_string(), "two".to_string(), "three".to_string()]
    );
}

#[test]
fn provider_lists_follow_replace_semantics_instead_of_union() {
    let mut base = Config {
        disabled_providers: vec!["ethnopic".to_string()],
        enabled_providers: vec!["openai".to_string()],
        ..Default::default()
    };

    let overlay = Config {
        disabled_providers: vec!["google".to_string()],
        ..Default::default()
    };

    base.merge(overlay);

    assert_eq!(base.disabled_providers, vec!["google".to_string()]);
    assert_eq!(base.enabled_providers, vec!["openai".to_string()]);
}

#[test]
fn mcp_enabled_flag_overlay_keeps_existing_full_server_fields() {
    let mut base = Config {
        mcp: Some(HashMap::from([(
            "repo".to_string(),
            McpServerConfig::Full(Box::new(McpServer {
                command: vec!["node".to_string(), "mcp.js".to_string()],
                timeout: Some(3000),
                ..Default::default()
            })),
        )])),
        ..Default::default()
    };

    let overlay = Config {
        mcp: Some(HashMap::from([(
            "repo".to_string(),
            McpServerConfig::Enabled { enabled: false },
        )])),
        ..Default::default()
    };

    base.merge(overlay);

    let server = base.mcp.unwrap().remove("repo").unwrap();
    match server {
        McpServerConfig::Full(server) => {
            assert_eq!(
                server.command,
                vec!["node".to_string(), "mcp.js".to_string()]
            );
            assert_eq!(server.timeout, Some(3000));
            assert_eq!(server.enabled, Some(false));
        }
        McpServerConfig::Enabled { .. } => panic!("expected full MCP server config"),
    }
}

#[test]
fn agent_configs_support_dynamic_keys_and_deep_merge() {
    let mut base = Config {
        agent: Some(AgentConfigs {
            entries: HashMap::from([(
                "reviewer".to_string(),
                AgentConfig {
                    prompt: Some("old prompt".to_string()),
                    options: Some(HashMap::from([("a".to_string(), serde_json::json!(1))])),
                    ..Default::default()
                },
            )]),
        }),
        ..Default::default()
    };

    let overlay = Config {
        agent: Some(AgentConfigs {
            entries: HashMap::from([
                (
                    "reviewer".to_string(),
                    AgentConfig {
                        prompt: Some("new prompt".to_string()),
                        options: Some(HashMap::from([("b".to_string(), serde_json::json!(2))])),
                        ..Default::default()
                    },
                ),
                (
                    "research".to_string(),
                    AgentConfig {
                        mode: Some(AgentMode::Subagent),
                        ..Default::default()
                    },
                ),
            ]),
        }),
        ..Default::default()
    };

    base.merge(overlay);

    let agents = base.agent.unwrap().entries;
    let reviewer = agents.get("reviewer").unwrap();
    assert_eq!(reviewer.prompt.as_deref(), Some("new prompt"));
    let options = reviewer.options.as_ref().unwrap();
    assert_eq!(options.get("a"), Some(&serde_json::json!(1)));
    assert_eq!(options.get("b"), Some(&serde_json::json!(2)));
    assert!(agents.contains_key("research"));
}

#[test]
fn composition_skill_tree_deserializes_from_camel_case() {
    let config: Config = serde_json::from_value(serde_json::json!({
        "composition": {
            "skillTree": {
                "enabled": true,
                "separator": "\n--\n",
                "root": {
                    "node_id": "root",
                    "markdown_path": "docs/root.md",
                    "children": []
                }
            }
        }
    }))
    .expect("config should deserialize");

    let skill_tree = config
        .composition
        .as_ref()
        .and_then(|c| c.skill_tree.as_ref())
        .expect("composition skill tree should exist");
    assert_eq!(skill_tree.enabled, Some(true));
    assert_eq!(skill_tree.separator.as_deref(), Some("\n--\n"));
    assert_eq!(
        skill_tree.root.as_ref().map(|root| root.node_id.as_str()),
        Some("root")
    );
}

#[test]
fn composition_skill_tree_merge_replaces_root_and_separator() {
    let mut base = Config {
        composition: Some(CompositionConfig {
            skill_tree: Some(SkillTreeConfig {
                enabled: Some(true),
                separator: Some("old".to_string()),
                root: Some(SkillTreeNodeConfig {
                    node_id: "old".to_string(),
                    markdown_path: "docs/old.md".to_string(),
                    children: Vec::new(),
                }),
            }),
        }),
        ..Default::default()
    };

    let overlay = Config {
        composition: Some(CompositionConfig {
            skill_tree: Some(SkillTreeConfig {
                enabled: Some(false),
                separator: Some("new".to_string()),
                root: Some(SkillTreeNodeConfig {
                    node_id: "new".to_string(),
                    markdown_path: "docs/new.md".to_string(),
                    children: Vec::new(),
                }),
            }),
        }),
        ..Default::default()
    };

    base.merge(overlay);

    let merged = base
        .composition
        .as_ref()
        .and_then(|c| c.skill_tree.as_ref())
        .expect("merged skill tree should exist");
    assert_eq!(merged.enabled, Some(false));
    assert_eq!(merged.separator.as_deref(), Some("new"));
    assert_eq!(
        merged.root.as_ref().map(|root| root.markdown_path.as_str()),
        Some("docs/new.md")
    );
}

#[test]
fn scheduler_path_deserializes_from_camel_case() {
    let config: Config = serde_json::from_value(serde_json::json!({
        "schedulerPath": "./.rocode/scheduler/sisyphus.jsonc"
    }))
    .expect("config should deserialize");

    assert_eq!(
        config.scheduler_path.as_deref(),
        Some("./.rocode/scheduler/sisyphus.jsonc")
    );
}

#[test]
fn scheduler_path_merge_replaces_previous_value() {
    let mut base = Config {
        scheduler_path: Some("/base/scheduler.jsonc".to_string()),
        ..Default::default()
    };

    let overlay = Config {
        scheduler_path: Some("/override/scheduler.jsonc".to_string()),
        ..Default::default()
    };

    base.merge(overlay);

    assert_eq!(
        base.scheduler_path.as_deref(),
        Some("/override/scheduler.jsonc")
    );
}

#[test]
fn web_search_merge_replaces_previous_base_url() {
    let mut base = Config {
        web_search: Some(WebSearchConfig {
            base_url: Some("https://old.example".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let overlay = Config {
        web_search: Some(WebSearchConfig {
            base_url: Some("https://new.example".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    base.merge(overlay);

    assert_eq!(
        base.web_search
            .as_ref()
            .and_then(|config| config.base_url.as_deref()),
        Some("https://new.example")
    );
}

#[test]
fn web_search_merge_deep_merges_all_fields() {
    let mut base = Config {
        web_search: Some(WebSearchConfig {
            base_url: Some("https://mcp.exa.ai".to_string()),
            method: Some("web_search_exa".to_string()),
            default_search_type: Some("auto".to_string()),
            default_num_results: Some(8),
            options: Some({
                let mut m = std::collections::HashMap::new();
                m.insert("livecrawl".to_string(), serde_json::json!("fallback"));
                m.insert("region".to_string(), serde_json::json!("us"));
                m
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let overlay = Config {
        web_search: Some(WebSearchConfig {
            endpoint: Some("/v2/search".to_string()),
            default_search_type: Some("deep".to_string()),
            options: Some({
                let mut m = std::collections::HashMap::new();
                m.insert("livecrawl".to_string(), serde_json::json!("preferred"));
                m.insert("language".to_string(), serde_json::json!("zh"));
                m
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    base.merge(overlay);

    let ws = base.web_search.as_ref().unwrap();
    // base_url kept from base (overlay didn't set it)
    assert_eq!(ws.base_url.as_deref(), Some("https://mcp.exa.ai"));
    // endpoint set by overlay
    assert_eq!(ws.endpoint.as_deref(), Some("/v2/search"));
    // method kept from base
    assert_eq!(ws.method.as_deref(), Some("web_search_exa"));
    // default_search_type overridden by overlay
    assert_eq!(ws.default_search_type.as_deref(), Some("deep"));
    // default_num_results kept from base
    assert_eq!(ws.default_num_results, Some(8));
    // options: key-level merge
    let opts = ws.options.as_ref().unwrap();
    assert_eq!(opts.get("livecrawl").unwrap(), "preferred"); // overridden
    assert_eq!(opts.get("region").unwrap(), "us"); // kept from base
    assert_eq!(opts.get("language").unwrap(), "zh"); // added by overlay
}
