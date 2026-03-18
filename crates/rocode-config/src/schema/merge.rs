use super::*;

trait DeepMerge {
    fn deep_merge(&mut self, other: Self);
}

fn merge_option_replace<T>(target: &mut Option<T>, source: Option<T>) {
    if let Some(value) = source {
        *target = Some(value);
    }
}

fn merge_option_deep<T: DeepMerge>(target: &mut Option<T>, source: Option<T>) {
    if let Some(source_value) = source {
        if let Some(target_value) = target {
            target_value.deep_merge(source_value);
        } else {
            *target = Some(source_value);
        }
    }
}

fn merge_map_deep_values<T: DeepMerge>(
    target: &mut HashMap<String, T>,
    source: HashMap<String, T>,
) {
    for (key, source_value) in source {
        if let Some(target_value) = target.get_mut(&key) {
            target_value.deep_merge(source_value);
        } else {
            target.insert(key, source_value);
        }
    }
}

fn merge_option_map_deep_values<T: DeepMerge>(
    target: &mut Option<HashMap<String, T>>,
    source: Option<HashMap<String, T>>,
) {
    if let Some(source_map) = source {
        if let Some(target_map) = target {
            merge_map_deep_values(target_map, source_map);
        } else {
            *target = Some(source_map);
        }
    }
}

fn merge_option_map_overwrite_values<T>(
    target: &mut Option<HashMap<String, T>>,
    source: Option<HashMap<String, T>>,
) {
    if let Some(source_map) = source {
        if let Some(target_map) = target {
            for (key, value) in source_map {
                target_map.insert(key, value);
            }
        } else {
            *target = Some(source_map);
        }
    }
}

fn merge_map_overwrite_values<T>(target: &mut HashMap<String, T>, source: HashMap<String, T>) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

fn merge_json_value(target: &mut serde_json::Value, source: serde_json::Value) {
    match (target, source) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                if let Some(target_value) = target_map.get_mut(&key) {
                    merge_json_value(target_value, source_value);
                } else {
                    target_map.insert(key, source_value);
                }
            }
        }
        (target_value, source_value) => *target_value = source_value,
    }
}

fn merge_option_json_map(
    target: &mut Option<HashMap<String, serde_json::Value>>,
    source: Option<HashMap<String, serde_json::Value>>,
) {
    if let Some(source_map) = source {
        if let Some(target_map) = target {
            for (key, source_value) in source_map {
                if let Some(target_value) = target_map.get_mut(&key) {
                    merge_json_value(target_value, source_value);
                } else {
                    target_map.insert(key, source_value);
                }
            }
        } else {
            *target = Some(source_map);
        }
    }
}

fn append_unique_keep_order(target: &mut Vec<String>, source: Vec<String>) {
    for item in source {
        if !target.contains(&item) {
            target.push(item);
        }
    }
}

impl DeepMerge for KeybindsConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.leader, other.leader);
        merge_option_replace(&mut self.app_exit, other.app_exit);
        merge_option_replace(&mut self.editor_open, other.editor_open);
        merge_option_replace(&mut self.theme_list, other.theme_list);
        merge_option_replace(&mut self.sidebar_toggle, other.sidebar_toggle);
        merge_option_replace(&mut self.scrollbar_toggle, other.scrollbar_toggle);
        merge_option_replace(&mut self.username_toggle, other.username_toggle);
        merge_option_replace(&mut self.status_view, other.status_view);
        merge_option_replace(&mut self.session_export, other.session_export);
        merge_option_replace(&mut self.session_new, other.session_new);
        merge_option_replace(&mut self.session_list, other.session_list);
        merge_option_replace(&mut self.session_timeline, other.session_timeline);
        merge_option_replace(&mut self.session_fork, other.session_fork);
        merge_option_replace(&mut self.session_rename, other.session_rename);
        merge_option_replace(&mut self.session_delete, other.session_delete);
        merge_option_replace(&mut self.stash_delete, other.stash_delete);
        merge_option_replace(&mut self.model_provider_list, other.model_provider_list);
        merge_option_replace(&mut self.model_favorite_toggle, other.model_favorite_toggle);
        merge_option_replace(&mut self.session_share, other.session_share);
        merge_option_replace(&mut self.session_unshare, other.session_unshare);
        merge_option_replace(&mut self.session_interrupt, other.session_interrupt);
        merge_option_replace(&mut self.session_compact, other.session_compact);
        merge_option_replace(&mut self.messages_page_up, other.messages_page_up);
        merge_option_replace(&mut self.messages_page_down, other.messages_page_down);
        merge_option_replace(&mut self.messages_line_up, other.messages_line_up);
        merge_option_replace(&mut self.messages_line_down, other.messages_line_down);
        merge_option_replace(&mut self.messages_half_page_up, other.messages_half_page_up);
        merge_option_replace(
            &mut self.messages_half_page_down,
            other.messages_half_page_down,
        );
        merge_option_replace(&mut self.messages_first, other.messages_first);
        merge_option_replace(&mut self.messages_last, other.messages_last);
        merge_option_replace(&mut self.messages_next, other.messages_next);
        merge_option_replace(&mut self.messages_previous, other.messages_previous);
        merge_option_replace(&mut self.messages_last_user, other.messages_last_user);
        merge_option_replace(&mut self.messages_copy, other.messages_copy);
        merge_option_replace(&mut self.messages_undo, other.messages_undo);
        merge_option_replace(&mut self.messages_redo, other.messages_redo);
        merge_option_replace(
            &mut self.messages_toggle_conceal,
            other.messages_toggle_conceal,
        );
        merge_option_replace(&mut self.tool_details, other.tool_details);
        merge_option_replace(&mut self.model_list, other.model_list);
        merge_option_replace(&mut self.model_cycle_recent, other.model_cycle_recent);
        merge_option_replace(
            &mut self.model_cycle_recent_reverse,
            other.model_cycle_recent_reverse,
        );
        merge_option_replace(&mut self.model_cycle_favorite, other.model_cycle_favorite);
        merge_option_replace(
            &mut self.model_cycle_favorite_reverse,
            other.model_cycle_favorite_reverse,
        );
        merge_option_replace(&mut self.command_list, other.command_list);
        merge_option_replace(&mut self.agent_list, other.agent_list);
        merge_option_replace(&mut self.agent_cycle, other.agent_cycle);
        merge_option_replace(&mut self.agent_cycle_reverse, other.agent_cycle_reverse);
        merge_option_replace(&mut self.variant_cycle, other.variant_cycle);
        merge_option_replace(&mut self.input_clear, other.input_clear);
        merge_option_replace(&mut self.input_paste, other.input_paste);
        merge_option_replace(&mut self.input_submit, other.input_submit);
        merge_option_replace(&mut self.input_newline, other.input_newline);
        merge_option_replace(&mut self.input_move_left, other.input_move_left);
        merge_option_replace(&mut self.input_move_right, other.input_move_right);
        merge_option_replace(&mut self.input_move_up, other.input_move_up);
        merge_option_replace(&mut self.input_move_down, other.input_move_down);
        merge_option_replace(&mut self.input_select_left, other.input_select_left);
        merge_option_replace(&mut self.input_select_right, other.input_select_right);
        merge_option_replace(&mut self.input_select_up, other.input_select_up);
        merge_option_replace(&mut self.input_select_down, other.input_select_down);
        merge_option_replace(&mut self.input_line_home, other.input_line_home);
        merge_option_replace(&mut self.input_line_end, other.input_line_end);
        merge_option_replace(
            &mut self.input_select_line_home,
            other.input_select_line_home,
        );
        merge_option_replace(&mut self.input_select_line_end, other.input_select_line_end);
        merge_option_replace(
            &mut self.input_visual_line_home,
            other.input_visual_line_home,
        );
        merge_option_replace(&mut self.input_visual_line_end, other.input_visual_line_end);
        merge_option_replace(
            &mut self.input_select_visual_line_home,
            other.input_select_visual_line_home,
        );
        merge_option_replace(
            &mut self.input_select_visual_line_end,
            other.input_select_visual_line_end,
        );
        merge_option_replace(&mut self.input_buffer_home, other.input_buffer_home);
        merge_option_replace(&mut self.input_buffer_end, other.input_buffer_end);
        merge_option_replace(
            &mut self.input_select_buffer_home,
            other.input_select_buffer_home,
        );
        merge_option_replace(
            &mut self.input_select_buffer_end,
            other.input_select_buffer_end,
        );
        merge_option_replace(&mut self.input_delete_line, other.input_delete_line);
        merge_option_replace(
            &mut self.input_delete_to_line_end,
            other.input_delete_to_line_end,
        );
        merge_option_replace(
            &mut self.input_delete_to_line_start,
            other.input_delete_to_line_start,
        );
        merge_option_replace(&mut self.input_backspace, other.input_backspace);
        merge_option_replace(&mut self.input_delete, other.input_delete);
        merge_option_replace(&mut self.input_undo, other.input_undo);
        merge_option_replace(&mut self.input_redo, other.input_redo);
        merge_option_replace(&mut self.input_word_forward, other.input_word_forward);
        merge_option_replace(&mut self.input_word_backward, other.input_word_backward);
        merge_option_replace(
            &mut self.input_select_word_forward,
            other.input_select_word_forward,
        );
        merge_option_replace(
            &mut self.input_select_word_backward,
            other.input_select_word_backward,
        );
        merge_option_replace(
            &mut self.input_delete_word_forward,
            other.input_delete_word_forward,
        );
        merge_option_replace(
            &mut self.input_delete_word_backward,
            other.input_delete_word_backward,
        );
        merge_option_replace(&mut self.history_previous, other.history_previous);
        merge_option_replace(&mut self.history_next, other.history_next);
        merge_option_replace(&mut self.session_child_cycle, other.session_child_cycle);
        merge_option_replace(
            &mut self.session_child_cycle_reverse,
            other.session_child_cycle_reverse,
        );
        merge_option_replace(&mut self.session_parent, other.session_parent);
        merge_option_replace(&mut self.terminal_suspend, other.terminal_suspend);
        merge_option_replace(&mut self.terminal_title_toggle, other.terminal_title_toggle);
        merge_option_replace(&mut self.tips_toggle, other.tips_toggle);
        merge_option_replace(&mut self.display_thinking, other.display_thinking);
        // Legacy fields
        merge_option_replace(&mut self.submit, other.submit);
        merge_option_replace(&mut self.cancel, other.cancel);
        merge_option_replace(&mut self.interrupt, other.interrupt);
    }
}

impl DeepMerge for TuiConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.mode, other.mode);
        merge_option_replace(&mut self.sidebar, other.sidebar);
        merge_option_replace(&mut self.scroll_speed, other.scroll_speed);
        merge_option_replace(&mut self.scroll_acceleration, other.scroll_acceleration);
        merge_option_replace(&mut self.diff_style, other.diff_style);
    }
}

impl DeepMerge for ServerConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.port, other.port);
        merge_option_replace(&mut self.hostname, other.hostname);
        merge_option_replace(&mut self.mdns, other.mdns);
        merge_option_replace(&mut self.mdns_domain, other.mdns_domain);
        merge_option_replace(&mut self.cors, other.cors);
    }
}

impl DeepMerge for CommandConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.name, other.name);
        merge_option_replace(&mut self.description, other.description);
        merge_option_replace(&mut self.template, other.template);
        merge_option_replace(&mut self.model, other.model);
        merge_option_replace(&mut self.agent, other.agent);
        merge_option_replace(&mut self.subtask, other.subtask);
    }
}

impl DeepMerge for SkillsConfig {
    fn deep_merge(&mut self, other: Self) {
        if !other.paths.is_empty() {
            self.paths = other.paths;
        }
        if !other.urls.is_empty() {
            self.urls = other.urls;
        }
    }
}

impl DeepMerge for DocsConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(
            &mut self.context_docs_registry_path,
            other.context_docs_registry_path,
        );
    }
}

impl DeepMerge for WatcherConfig {
    fn deep_merge(&mut self, other: Self) {
        if !other.ignore.is_empty() {
            self.ignore = other.ignore;
        }
    }
}

impl DeepMerge for AgentConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.name, other.name);
        merge_option_replace(&mut self.model, other.model);
        merge_option_replace(&mut self.variant, other.variant);
        merge_option_replace(&mut self.temperature, other.temperature);
        merge_option_replace(&mut self.top_p, other.top_p);
        merge_option_replace(&mut self.prompt, other.prompt);
        merge_option_replace(&mut self.disable, other.disable);
        merge_option_replace(&mut self.description, other.description);
        merge_option_replace(&mut self.mode, other.mode);
        merge_option_replace(&mut self.hidden, other.hidden);
        merge_option_json_map(&mut self.options, other.options);
        merge_option_replace(&mut self.color, other.color);
        merge_option_replace(&mut self.steps, other.steps);
        merge_option_replace(&mut self.max_tokens, other.max_tokens);
        merge_option_replace(&mut self.max_steps, other.max_steps);
        merge_option_deep(&mut self.permission, other.permission);
        merge_option_map_overwrite_values(&mut self.tools, other.tools);
    }
}

impl DeepMerge for AgentConfigs {
    fn deep_merge(&mut self, other: Self) {
        merge_map_deep_values(&mut self.entries, other.entries);
    }
}

impl DeepMerge for CompositionConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_deep(&mut self.skill_tree, other.skill_tree);
    }
}

impl DeepMerge for SkillTreeConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.enabled, other.enabled);
        merge_option_replace(&mut self.root, other.root);
        merge_option_replace(&mut self.separator, other.separator);
    }
}

impl DeepMerge for ModelConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.name, other.name);
        merge_option_replace(&mut self.model, other.model);
        merge_option_replace(&mut self.api_key, other.api_key);
        merge_option_replace(&mut self.base_url, other.base_url);
        merge_option_map_deep_values(&mut self.variants, other.variants);
    }
}

impl DeepMerge for ModelVariantConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.disabled, other.disabled);
        for (key, value) in other.extra {
            self.extra.insert(key, value);
        }
    }
}

impl DeepMerge for ProviderConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.name, other.name);
        merge_option_replace(&mut self.base, other.base);
        merge_option_replace(&mut self.api_key, other.api_key);
        merge_option_replace(&mut self.base_url, other.base_url);
        merge_option_map_deep_values(&mut self.models, other.models);
        merge_option_json_map(&mut self.options, other.options);
        merge_option_replace(&mut self.npm, other.npm);
        if !other.whitelist.is_empty() {
            self.whitelist = other.whitelist;
        }
        if !other.blacklist.is_empty() {
            self.blacklist = other.blacklist;
        }
    }
}

impl DeepMerge for McpServer {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.server_type, other.server_type);
        if !other.command.is_empty() {
            self.command = other.command;
        }
        merge_option_map_overwrite_values(&mut self.environment, other.environment);
        merge_option_replace(&mut self.url, other.url);
        merge_option_replace(&mut self.enabled, other.enabled);
        merge_option_replace(&mut self.timeout, other.timeout);
        merge_option_map_overwrite_values(&mut self.headers, other.headers);
        merge_option_replace(&mut self.oauth, other.oauth);
        // Legacy fields
        if !other.args.is_empty() {
            self.args = other.args;
        }
        merge_option_map_overwrite_values(&mut self.env, other.env);
        merge_option_replace(&mut self.client_id, other.client_id);
        merge_option_replace(&mut self.authorization_url, other.authorization_url);
    }
}

impl DeepMerge for McpServerConfig {
    fn deep_merge(&mut self, other: Self) {
        match other {
            McpServerConfig::Enabled { enabled } => match self {
                McpServerConfig::Enabled {
                    enabled: target_enabled,
                } => *target_enabled = enabled,
                McpServerConfig::Full(target_server) => target_server.enabled = Some(enabled),
            },
            McpServerConfig::Full(mut source_server) => match self {
                McpServerConfig::Full(target_server) => target_server.deep_merge(*source_server),
                McpServerConfig::Enabled { enabled } => {
                    if source_server.enabled.is_none() {
                        source_server.enabled = Some(*enabled);
                    }
                    *self = McpServerConfig::Full(source_server);
                }
            },
        }
    }
}

impl DeepMerge for FormatterEntry {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.disabled, other.disabled);
        if !other.command.is_empty() {
            self.command = other.command;
        }
        merge_option_map_overwrite_values(&mut self.environment, other.environment);
        if !other.extensions.is_empty() {
            self.extensions = other.extensions;
        }
    }
}

impl DeepMerge for FormatterConfig {
    fn deep_merge(&mut self, other: Self) {
        match other {
            FormatterConfig::Disabled(value) => *self = FormatterConfig::Disabled(value),
            FormatterConfig::Enabled(source_map) => match self {
                FormatterConfig::Disabled(_) => *self = FormatterConfig::Enabled(source_map),
                FormatterConfig::Enabled(target_map) => {
                    merge_map_deep_values(target_map, source_map);
                }
            },
        }
    }
}

impl DeepMerge for LspServerConfig {
    fn deep_merge(&mut self, other: Self) {
        if !other.command.is_empty() {
            self.command = other.command;
        }
        if !other.extensions.is_empty() {
            self.extensions = other.extensions;
        }
        merge_option_replace(&mut self.disabled, other.disabled);
        merge_option_map_overwrite_values(&mut self.env, other.env);
        merge_option_json_map(&mut self.initialization, other.initialization);
    }
}

impl DeepMerge for LspConfig {
    fn deep_merge(&mut self, other: Self) {
        match other {
            LspConfig::Disabled(value) => *self = LspConfig::Disabled(value),
            LspConfig::Enabled(source_map) => match self {
                LspConfig::Disabled(_) => *self = LspConfig::Enabled(source_map),
                LspConfig::Enabled(target_map) => {
                    merge_map_deep_values(target_map, source_map);
                }
            },
        }
    }
}

impl DeepMerge for PermissionConfig {
    fn deep_merge(&mut self, other: Self) {
        for (key, value) in other.rules {
            self.rules.insert(key, value);
        }
    }
}

impl DeepMerge for EnterpriseConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.url, other.url);
        merge_option_replace(&mut self.managed_config_dir, other.managed_config_dir);
    }
}

impl DeepMerge for CompactionConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.auto, other.auto);
        merge_option_replace(&mut self.prune, other.prune);
        merge_option_replace(&mut self.reserved, other.reserved);
    }
}

impl DeepMerge for ExperimentalConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.disable_paste_summary, other.disable_paste_summary);
        merge_option_replace(&mut self.batch_tool, other.batch_tool);
        merge_option_replace(&mut self.open_telemetry, other.open_telemetry);
        if !other.primary_tools.is_empty() {
            self.primary_tools = other.primary_tools;
        }
        merge_option_replace(&mut self.continue_loop_on_deny, other.continue_loop_on_deny);
        merge_option_replace(&mut self.mcp_timeout, other.mcp_timeout);
    }
}

impl DeepMerge for WebSearchConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.base_url, other.base_url);
        merge_option_replace(&mut self.endpoint, other.endpoint);
        merge_option_replace(&mut self.method, other.method);
        merge_option_replace(&mut self.default_search_type, other.default_search_type);
        merge_option_replace(&mut self.default_num_results, other.default_num_results);
        merge_option_map_overwrite_values(&mut self.options, other.options);
    }
}

impl DeepMerge for UiPreferencesConfig {
    fn deep_merge(&mut self, other: Self) {
        merge_option_replace(&mut self.theme, other.theme);
        merge_option_replace(&mut self.web_theme, other.web_theme);
        merge_option_replace(&mut self.web_mode, other.web_mode);
        merge_option_replace(&mut self.show_header, other.show_header);
        merge_option_replace(&mut self.show_scrollbar, other.show_scrollbar);
        merge_option_replace(&mut self.tips_hidden, other.tips_hidden);
        merge_option_replace(&mut self.show_timestamps, other.show_timestamps);
        merge_option_replace(&mut self.show_thinking, other.show_thinking);
        merge_option_replace(&mut self.show_tool_details, other.show_tool_details);
        merge_option_replace(&mut self.message_density, other.message_density);
        merge_option_replace(&mut self.semantic_highlight, other.semantic_highlight);
        if !other.recent_models.is_empty() {
            self.recent_models = other.recent_models;
        }
    }
}

impl Config {
    pub fn merge(&mut self, other: Config) {
        merge_option_replace(&mut self.schema, other.schema);
        merge_option_replace(&mut self.theme, other.theme);
        merge_option_deep(&mut self.keybinds, other.keybinds);
        merge_option_replace(&mut self.log_level, other.log_level);
        merge_option_deep(&mut self.tui, other.tui);
        merge_option_deep(&mut self.server, other.server);
        merge_option_map_deep_values(&mut self.command, other.command);
        merge_option_deep(&mut self.skills, other.skills);
        merge_option_deep(&mut self.docs, other.docs);
        merge_option_replace(&mut self.scheduler_path, other.scheduler_path);
        merge_option_replace(&mut self.task_category_path, other.task_category_path);
        merge_map_overwrite_values(&mut self.skill_paths, other.skill_paths);
        merge_option_deep(&mut self.watcher, other.watcher);
        merge_option_replace(&mut self.snapshot, other.snapshot);
        merge_option_replace(&mut self.share, other.share);
        merge_option_replace(&mut self.autoshare, other.autoshare);
        merge_option_replace(&mut self.autoupdate, other.autoupdate);
        merge_option_replace(&mut self.model, other.model);
        merge_option_replace(&mut self.small_model, other.small_model);
        merge_option_replace(&mut self.default_agent, other.default_agent);
        merge_option_replace(&mut self.username, other.username);
        merge_option_deep(&mut self.mode, other.mode);
        merge_option_deep(&mut self.agent, other.agent);
        merge_option_deep(&mut self.composition, other.composition);
        merge_option_map_deep_values(&mut self.provider, other.provider);
        merge_option_map_deep_values(&mut self.mcp, other.mcp);
        merge_option_deep(&mut self.formatter, other.formatter);
        merge_option_deep(&mut self.lsp, other.lsp);
        merge_option_replace(&mut self.layout, other.layout);
        merge_option_deep(&mut self.ui_preferences, other.ui_preferences);
        merge_option_deep(&mut self.permission, other.permission);
        merge_option_map_overwrite_values(&mut self.tools, other.tools);
        merge_option_deep(&mut self.web_search, other.web_search);
        merge_option_deep(&mut self.enterprise, other.enterprise);
        merge_option_deep(&mut self.compaction, other.compaction);
        merge_option_deep(&mut self.experimental, other.experimental);
        merge_option_map_overwrite_values(&mut self.env, other.env);
        merge_map_overwrite_values(&mut self.plugin_paths, other.plugin_paths);

        // Merge plugin map: other's entries override self's by key
        for (key, config) in other.plugin {
            self.plugin.insert(key, config);
        }
        append_unique_keep_order(&mut self.instructions, other.instructions);

        if !other.disabled_providers.is_empty() {
            self.disabled_providers = other.disabled_providers;
        }
        if !other.enabled_providers.is_empty() {
            self.enabled_providers = other.enabled_providers;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_merge_deep_merges_ui_preferences() {
        let mut config = Config {
            ui_preferences: Some(UiPreferencesConfig {
                theme: Some("opencode@dark".to_string()),
                web_theme: Some("midnight".to_string()),
                show_header: Some(true),
                recent_models: vec![UiRecentModelConfig {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        config.merge(Config {
            ui_preferences: Some(UiPreferencesConfig {
                web_mode: Some("agent:atlas".to_string()),
                show_header: Some(false),
                show_thinking: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        });

        let ui = config.ui_preferences.expect("ui preferences");
        assert_eq!(ui.theme.as_deref(), Some("opencode@dark"));
        assert_eq!(ui.web_theme.as_deref(), Some("midnight"));
        assert_eq!(ui.web_mode.as_deref(), Some("agent:atlas"));
        assert_eq!(ui.show_header, Some(false));
        assert_eq!(ui.show_thinking, Some(true));
        assert_eq!(
            ui.recent_models,
            vec![UiRecentModelConfig {
                provider: "openai".to_string(),
                model: "gpt-5".to_string(),
            }]
        );
    }

    #[test]
    fn config_merge_replaces_recent_models_when_supplied() {
        let mut config = Config {
            ui_preferences: Some(UiPreferencesConfig {
                recent_models: vec![UiRecentModelConfig {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        config.merge(Config {
            ui_preferences: Some(UiPreferencesConfig {
                recent_models: vec![UiRecentModelConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4".to_string(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        });

        let ui = config.ui_preferences.expect("ui preferences");
        assert_eq!(
            ui.recent_models,
            vec![UiRecentModelConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4".to_string(),
            }]
        );
    }
}
