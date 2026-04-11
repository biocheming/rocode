use super::*;

impl App {
    pub(super) fn submit_skill_create(&mut self) -> anyhow::Result<String> {
        let Some(session_id) = self.current_session_id() else {
            anyhow::bail!("Select or create a session before managing skills.");
        };
        let Some(client) = self.context.get_api_client() else {
            anyhow::bail!("No active API client.");
        };
        let Some((name, description, category, body)) = self.skill_list_dialog.create_payload()
        else {
            anyhow::bail!("Skill create draft is not active.");
        };
        if name.trim().is_empty() || description.trim().is_empty() || body.trim().is_empty() {
            anyhow::bail!("Name, description, and body are required.");
        }

        let response = client.manage_skill(&crate::api::SkillManageRequest {
            session_id,
            action: crate::api::SkillManageAction::Create,
            name: Some(name.clone()),
            new_name: None,
            description: Some(description),
            body: Some(body),
            content: None,
            category,
            directory_name: None,
            file_path: None,
        })?;
        self.skill_list_dialog.cancel_manage_mode();
        self.refresh_skill_list_dialog()?;
        Ok(format!("Created skill {}.", response.result.skill_name))
    }

    pub(super) fn submit_skill_edit(&mut self) -> anyhow::Result<String> {
        let Some(session_id) = self.current_session_id() else {
            anyhow::bail!("Select or create a session before managing skills.");
        };
        let Some(client) = self.context.get_api_client() else {
            anyhow::bail!("No active API client.");
        };
        let Some((name, content)) = self.skill_list_dialog.edit_payload() else {
            anyhow::bail!("Skill edit draft is not active.");
        };
        if content.trim().is_empty() {
            anyhow::bail!("Skill source cannot be empty.");
        }

        let response = client.manage_skill(&crate::api::SkillManageRequest {
            session_id,
            action: crate::api::SkillManageAction::Edit,
            name: Some(name.clone()),
            new_name: None,
            description: None,
            body: None,
            content: Some(content),
            category: None,
            directory_name: None,
            file_path: None,
        })?;
        self.skill_list_dialog.cancel_manage_mode();
        self.refresh_skill_list_dialog()?;
        Ok(format!("Saved skill {}.", response.result.skill_name))
    }

    pub(super) fn submit_skill_delete(&mut self) -> anyhow::Result<String> {
        let Some(session_id) = self.current_session_id() else {
            anyhow::bail!("Select or create a session before managing skills.");
        };
        let Some(client) = self.context.get_api_client() else {
            anyhow::bail!("No active API client.");
        };
        let Some(name) = self.skill_list_dialog.delete_payload() else {
            anyhow::bail!("Skill delete confirmation is not active.");
        };

        let response = client.manage_skill(&crate::api::SkillManageRequest {
            session_id,
            action: crate::api::SkillManageAction::Delete,
            name: Some(name),
            new_name: None,
            description: None,
            body: None,
            content: None,
            category: None,
            directory_name: None,
            file_path: None,
        })?;
        self.skill_list_dialog.cancel_manage_mode();
        self.refresh_skill_list_dialog()?;
        Ok(format!("Deleted skill {}.", response.result.skill_name))
    }

    pub(super) fn refresh_skill_list_detail(&mut self) -> anyhow::Result<()> {
        let Some(client) = self.context.get_api_client() else {
            self.skill_list_dialog.clear_detail();
            return Ok(());
        };
        let Some(skill_name) = self
            .skill_list_dialog
            .selected_skill()
            .map(|value| value.to_string())
        else {
            self.skill_list_dialog.clear_detail();
            return Ok(());
        };

        match client.get_skill_detail(&skill_name) {
            Ok(detail) => {
                self.skill_list_dialog.set_skill_detail(detail);
                Ok(())
            }
            Err(error) => {
                self.skill_list_dialog
                    .set_skill_detail_error(format!("Failed to load `{}`: {}", skill_name, error));
                Err(error)
            }
        }
    }

    pub(super) fn refresh_model_dialog(&mut self) {
        let Some(client) = self.context.get_api_client() else {
            self.context.set_has_connected_provider(false);
            return;
        };

        let Ok(providers) = client.get_all_providers() else {
            self.context.set_has_connected_provider(false);
            return;
        };
        let connected_provider_ids = providers.connected.iter().cloned().collect::<HashSet<_>>();
        let has_connected_provider = !connected_provider_ids.is_empty();
        self.context
            .set_has_connected_provider(has_connected_provider);

        let mut available_models = HashSet::new();
        let mut variant_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut models = Vec::new();
        let mut context_providers = Vec::new();
        for provider in providers.all {
            let provider_id = provider.id.clone();
            let provider_name = provider.name.clone();
            let mut provider_models = Vec::new();
            for model in provider.models {
                let model_id = model.id;
                let model_name = model.name;
                let model_context_window = model.context_window.unwrap_or(0);
                let model_ref = format!("{}/{}", provider_id, model_id);
                available_models.insert(model_ref.clone());
                let entry = variant_map.entry(model_ref).or_default();
                for variant in model.variants {
                    if !entry.iter().any(|value| value == &variant) {
                        entry.push(variant);
                    }
                }
                models.push(Model {
                    id: model_id.clone(),
                    name: model_name.clone(),
                    provider: provider_id.clone(),
                    context_window: model_context_window,
                });
                provider_models.push(crate::context::ModelInfo {
                    id: format!("{}/{}", provider_id, model_id),
                    name: model_name,
                    context_window: model_context_window,
                    max_output_tokens: 0,
                    supports_vision: false,
                    supports_tools: true,
                });
            }
            context_providers.push(crate::context::ProviderInfo {
                id: provider_id,
                name: provider_name,
                models: provider_models,
            });
        }
        *self.context.providers.write() = context_providers;
        models.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for variants in variant_map.values_mut() {
            variants.sort();
        }
        self.model_select.set_models(models);
        // Sync current model indicator
        let current_key = self.context.current_model.read().as_ref().map(|m| {
            let provider = self.context.current_provider.read();
            if let Some(ref p) = *provider {
                format!("{}/{}", p, m)
            } else {
                m.clone()
            }
        });
        self.model_select.set_current_model(current_key);
        // Restore persisted recent models
        self.model_select
            .set_recent(self.context.load_recent_models());
        self.available_models = available_models;
        self.model_variants = variant_map;
        self.model_variant_selection.retain(|model_key, variant| {
            let Some(available) = self.model_variants.get(model_key) else {
                return false;
            };
            match variant {
                Some(value) => available.iter().any(|item| item == value),
                None => true,
            }
        });
        self.sync_current_model_variant();

        let model_missing = self.context.current_model.read().is_none();
        if model_missing {
            // Try to restore the most recently used model that is still
            // available, mirroring TS local.tsx:175-179.
            let restored = self
                .model_select
                .recent()
                .iter()
                .find_map(|(provider, model_id)| {
                    let key = format!("{}/{}", provider, model_id);
                    if self.available_models.contains(&key) {
                        Some((key, provider.clone()))
                    } else {
                        None
                    }
                });
            if let Some((model_ref, provider)) = restored {
                self.set_active_model_selection(model_ref, Some(provider));
            } else if let Some((provider, model_id)) = providers.default_model.iter().next() {
                self.set_active_model_selection(
                    format!("{}/{}", provider, model_id),
                    Some(provider.clone()),
                );
            }
        }
    }

    pub(super) fn refresh_agent_dialog(&mut self) {
        let Some(client) = self.context.get_api_client() else {
            return;
        };

        let agents = match client.list_execution_modes() {
            Ok(agents) => agents,
            Err(error) => {
                self.toast.show(
                    ToastVariant::Error,
                    &format!("Failed to load modes: {}", error),
                    3000,
                );
                return;
            }
        };

        if agents.is_empty() {
            return;
        }

        let theme = self.context.theme.read().clone();
        let mut mode_names = Vec::new();
        let mapped = agents
            .into_iter()
            .filter(|mode| {
                if mode.kind == "agent" {
                    let is_subagent = mode.mode.as_deref() == Some("subagent");
                    let is_hidden = mode.hidden.unwrap_or(false);
                    !is_subagent && !is_hidden
                } else {
                    true
                }
            })
            .enumerate()
            .map(|(idx, mode)| {
                mode_names.push(mode.id.clone());
                map_execution_mode_to_dialog_option(&theme, idx, mode)
            })
            .collect::<Vec<_>>();
        self.agent_select.set_agents(mapped);
        let current = current_mode_label(&self.context).unwrap_or_default();
        if !current.trim().is_empty() && !mode_names.iter().any(|name| name == &current) {
            if let Some(first) = self.agent_select.agents().first() {
                apply_selected_mode(&self.context, first);
                self.sync_prompt_spinner_style();
            }
        }
        self.prompt.set_agent_suggestions(mode_names);
    }

    pub(super) fn cycle_agent(&mut self, direction: i8) {
        self.refresh_agent_dialog();

        let agents = self.agent_select.agents();
        if agents.is_empty() {
            return;
        }

        let current = current_mode_label(&self.context).unwrap_or_default();
        let current_index = agents
            .iter()
            .position(|agent| agent.name == current)
            .unwrap_or(0);

        let len = agents.len();
        let next_index = if direction >= 0 {
            (current_index + 1) % len
        } else if current_index == 0 {
            len - 1
        } else {
            current_index - 1
        };
        let next_agent = agents[next_index].clone();

        apply_selected_mode(&self.context, &next_agent);
        self.sync_prompt_spinner_style();
    }

    pub(super) fn refresh_session_list_dialog(&mut self) {
        let Some(client) = self.context.get_api_client() else {
            return;
        };

        let query = self.session_list_dialog.query().trim().to_string();
        let sessions_result = if query.is_empty() {
            client.list_sessions()
        } else {
            client.list_sessions_filtered(Some(&query), Some(30))
        };
        let Ok(sessions) = sessions_result else {
            return;
        };
        let status_map = client.get_session_status().unwrap_or_default();
        {
            let mut session_ctx = self.context.session.write();
            for session in &sessions {
                if let Some(status) = status_map.get(&session.id) {
                    session_ctx.set_status(&session.id, map_api_run_status(status));
                }
            }
        }

        let items = sessions
            .into_iter()
            .map(|session| SessionItem {
                is_busy: status_map.get(&session.id).map(|s| s.busy).unwrap_or(false),
                id: session.id,
                title: session.title,
                directory: session.directory,
                parent_id: session.parent_id,
                updated_at: session.time.updated,
            })
            .collect::<Vec<_>>();
        self.session_list_dialog.set_sessions(items);
    }

    pub(super) fn refresh_theme_list_dialog(&mut self) {
        let options = self
            .context
            .available_theme_names()
            .into_iter()
            .map(|name| ThemeOption {
                id: name.clone(),
                name: format_theme_option_label(&name),
            })
            .collect::<Vec<_>>();
        self.theme_list_dialog.set_options(options);
    }

    pub(super) fn refresh_skill_list_dialog(&mut self) -> anyhow::Result<()> {
        let Some(client) = self.context.get_api_client() else {
            self.skill_list_dialog.clear_detail();
            return Ok(());
        };
        let query = crate::api::SkillCatalogQuery {
            session_id: self.current_session_id(),
            ..Default::default()
        };
        let skills = client.list_skills(Some(&query))?;
        let suggestions = skills.iter().map(|skill| skill.name.clone()).collect();
        self.skill_list_dialog.set_skills(skills);
        self.prompt.set_skill_suggestions(suggestions);
        let _ = self.refresh_skill_list_detail();
        Ok(())
    }

    pub(super) fn refresh_lsp_status(&mut self) -> anyhow::Result<()> {
        let Some(client) = self.context.get_api_client() else {
            return Ok(());
        };
        let servers = client.get_lsp_servers()?;
        let statuses = servers
            .into_iter()
            .map(|id| crate::context::LspStatus {
                id,
                root: "-".to_string(),
                status: crate::context::LspConnectionStatus::Connected,
            })
            .collect::<Vec<_>>();
        *self.context.lsp_status.write() = statuses;
        Ok(())
    }

    pub(super) fn refresh_mcp_dialog(&mut self) -> anyhow::Result<()> {
        let Some(client) = self.context.get_api_client() else {
            return Ok(());
        };
        let servers = client.get_mcp_status()?;

        let mcp_items = servers
            .iter()
            .map(|server| McpItem {
                name: server.name.clone(),
                status: server.status.clone(),
                tools: server.tools,
                resources: server.resources,
                error: server.error.clone(),
            })
            .collect::<Vec<_>>();
        self.mcp_dialog.set_items(mcp_items);

        let statuses = servers
            .into_iter()
            .map(|server| {
                let status = map_mcp_status(&server);
                McpServerStatus {
                    name: server.name,
                    status,
                    error: server.error,
                }
            })
            .collect::<Vec<_>>();
        *self.context.mcp_servers.write() = statuses;
        Ok(())
    }
}
