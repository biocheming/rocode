use super::*;
use rocode_command::ResolvedUiCommand;
use serde::Deserialize;

fn mode_matches_action_argument(mode: &Agent, action_id: UiActionId, value: &str) -> bool {
    let needle = value.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }

    let mode_kind = match mode.kind {
        ModeKind::Agent => "agent",
        ModeKind::Preset => "preset",
        ModeKind::Profile => "profile",
    };

    let in_scope = match action_id {
        UiActionId::OpenAgentList => matches!(mode.kind, ModeKind::Agent),
        UiActionId::OpenPresetList => matches!(mode.kind, ModeKind::Preset | ModeKind::Profile),
        UiActionId::OpenModeList => true,
        _ => false,
    };
    if !in_scope {
        return false;
    }

    mode.name.eq_ignore_ascii_case(&needle)
        || format!("{mode_kind}:{}", mode.name).eq_ignore_ascii_case(&needle)
}

fn session_matches_target(session: &crate::api::SessionInfo, target: &str) -> bool {
    let needle = target.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    session.id.eq_ignore_ascii_case(&needle)
        || session.id.to_ascii_lowercase().starts_with(&needle)
        || session.title.to_ascii_lowercase().contains(&needle)
}

impl App {
    pub(super) fn execute_ui_action_invocation(
        &mut self,
        invocation: &ResolvedUiCommand,
    ) -> anyhow::Result<()> {
        let argument = invocation
            .argument
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        match invocation.action_id {
            UiActionId::OpenModelList => {
                let Some(model_ref) = argument else {
                    return self.execute_ui_action(invocation.action_id);
                };
                self.refresh_model_dialog();
                let (model_key, _) = parse_model_ref_selection(
                    model_ref,
                    &self.available_models,
                    &self.model_variants,
                );
                if !self.available_models.contains(&model_key) {
                    self.toast.show(
                        ToastVariant::Warning,
                        &format!("Unknown model: {}", model_ref),
                        2200,
                    );
                    return Ok(());
                }
                self.set_active_model_selection(
                    model_ref.to_string(),
                    provider_from_model(model_ref),
                );
                self.toast.show(
                    ToastVariant::Success,
                    &format!("Model set to {}", model_ref),
                    1800,
                );
                Ok(())
            }
            UiActionId::OpenThemeList => {
                let Some(theme_id) = argument else {
                    return self.execute_ui_action(invocation.action_id);
                };
                if !self.context.commit_theme_by_name(theme_id) {
                    self.toast.show(
                        ToastVariant::Warning,
                        &format!("Unknown theme: {}", theme_id),
                        2200,
                    );
                    return Ok(());
                }
                self.toast.show(
                    ToastVariant::Success,
                    &format!("Theme set to {}", theme_id),
                    1800,
                );
                Ok(())
            }
            UiActionId::OpenAgentList | UiActionId::OpenPresetList | UiActionId::OpenModeList => {
                let Some(mode_ref) = argument else {
                    return self.execute_ui_action(invocation.action_id);
                };
                self.refresh_agent_dialog();
                let Some(mode) = self
                    .agent_select
                    .agents()
                    .iter()
                    .find(|mode| mode_matches_action_argument(mode, invocation.action_id, mode_ref))
                    .cloned()
                else {
                    let label = match invocation.action_id {
                        UiActionId::OpenAgentList => "agent",
                        UiActionId::OpenPresetList => "preset",
                        _ => "mode",
                    };
                    self.toast.show(
                        ToastVariant::Warning,
                        &format!("Unknown {}: {}", label, mode_ref),
                        2200,
                    );
                    return Ok(());
                };
                apply_selected_mode(&self.context, &mode);
                self.sync_prompt_spinner_style();
                let label = match invocation.action_id {
                    UiActionId::OpenAgentList => "Agent",
                    UiActionId::OpenPresetList => "Preset",
                    _ => "Mode",
                };
                self.toast.show(
                    ToastVariant::Success,
                    &format!("{} set to {}", label, mode.name),
                    1800,
                );
                Ok(())
            }
            UiActionId::OpenSessionList => {
                let Some(target) = argument else {
                    return self.execute_ui_action(invocation.action_id);
                };
                match target {
                    "list" => self.execute_ui_action(invocation.action_id),
                    "new" => self.execute_ui_action(UiActionId::NewSession),
                    "fork" => {
                        self.handle_fork_session();
                        Ok(())
                    }
                    "compact" => {
                        self.handle_compact_session();
                        Ok(())
                    }
                    "delete" => {
                        let Some(session_id) = self.current_session_id() else {
                            self.toast.show(
                                ToastVariant::Warning,
                                "No active session to delete.",
                                2200,
                            );
                            return Ok(());
                        };
                        let Some(client) = self.context.get_api_client() else {
                            return Ok(());
                        };
                        match client.delete_session(&session_id) {
                            Ok(_) => {
                                if self.active_session_id.as_deref() == Some(session_id.as_str()) {
                                    self.context.navigate(Route::Home);
                                    self.active_session_id = None;
                                    self.session_view = None;
                                }
                                self.refresh_session_list_dialog();
                                self.toast.show(
                                    ToastVariant::Success,
                                    &format!("Session deleted: {}", session_id),
                                    2200,
                                );
                            }
                            Err(err) => {
                                self.toast.show(
                                    ToastVariant::Error,
                                    &format!("Failed to delete session: {}", err),
                                    3000,
                                );
                            }
                        }
                        Ok(())
                    }
                    _ => {
                        let Some(client) = self.context.get_api_client() else {
                            return Ok(());
                        };
                        match client.list_sessions_filtered(Some(target), Some(30)) {
                            Ok(sessions) => {
                                let Some(session) = sessions
                                    .into_iter()
                                    .find(|session| session_matches_target(session, target))
                                else {
                                    self.toast.show(
                                        ToastVariant::Warning,
                                        &format!("Session not found: {}", target),
                                        2200,
                                    );
                                    return Ok(());
                                };
                                self.context.navigate(Route::Session {
                                    session_id: session.id.clone(),
                                });
                                self.ensure_session_view(&session.id);
                                let _ = self.sync_session_from_server(&session.id);
                                self.toast.show(
                                    ToastVariant::Success,
                                    &format!("Session switched: {}", session.id),
                                    1800,
                                );
                                Ok(())
                            }
                            Err(err) => {
                                self.toast.show(
                                    ToastVariant::Error,
                                    &format!("Failed to load sessions: {}", err),
                                    3000,
                                );
                                Ok(())
                            }
                        }
                    }
                }
            }
            _ => self.execute_ui_action(invocation.action_id),
        }
    }

    pub(super) fn execute_ui_action(&mut self, action: UiActionId) -> anyhow::Result<()> {
        match action {
            UiActionId::AbortExecution => {
                if let Some(session_id) = &self.active_session_id {
                    if let Some(api) = self.context.get_api_client() {
                        match api.abort_session(session_id) {
                            Err(e) => {
                                self.toast.show(
                                    ToastVariant::Error,
                                    &format!("Failed to cancel run: {}", e),
                                    3000,
                                );
                            }
                            Ok(value) => {
                                #[derive(Debug, Deserialize, Default)]
                                struct AbortSessionResponse {
                                    #[serde(default)]
                                    target: Option<String>,
                                    #[serde(default)]
                                    stage: Option<String>,
                                }

                                let parsed = serde_json::from_value::<AbortSessionResponse>(value)
                                    .unwrap_or_default();
                                let message = match parsed.target.as_deref() {
                                    Some("stage") => {
                                        let stage =
                                            parsed.stage.as_deref().unwrap_or("current stage");
                                        format!("Stage cancellation requested: {}", stage)
                                    }
                                    Some(_) | None => "Run cancellation requested".to_string(),
                                };
                                self.toast.show(ToastVariant::Info, &message, 3000);
                            }
                        }
                    }
                }
            }
            UiActionId::SubmitPrompt => self.submit_prompt()?,
            UiActionId::ClearPrompt => self.prompt.clear(),
            UiActionId::PasteClipboard => self.paste_clipboard_to_prompt(),
            UiActionId::CopyPrompt => self.copy_prompt_to_clipboard(),
            UiActionId::CutPrompt => self.cut_prompt_to_clipboard(),
            UiActionId::HistoryPrevious => self.prompt.history_previous_entry(),
            UiActionId::HistoryNext => self.prompt.history_next_entry(),
            UiActionId::ToggleSidebar => self.context.toggle_sidebar(),
            UiActionId::ToggleHeader => self.context.toggle_header(),
            UiActionId::ToggleScrollbar => self.context.toggle_scrollbar(),
            UiActionId::OpenSessionList => {
                self.refresh_session_list_dialog();
                self.session_list_dialog
                    .open(self.active_session_id.as_deref());
            }
            UiActionId::NavigateParentSession => {
                self.navigate_to_parent_session();
            }
            UiActionId::RenameSession => {
                self.open_session_rename_dialog();
            }
            UiActionId::ExportSession => {
                self.open_session_export_dialog();
            }
            UiActionId::PromptStashPush => {
                if self.prompt.stash_current() {
                    self.alert_dialog.set_message("Prompt stashed.");
                    self.alert_dialog.open();
                } else {
                    self.alert_dialog
                        .set_message("Prompt is empty, nothing to stash.");
                    self.alert_dialog.open();
                }
            }
            UiActionId::PromptStashList => {
                self.open_prompt_stash_dialog();
            }
            UiActionId::PromptSkillList => {
                self.open_skill_list_dialog();
            }
            UiActionId::OpenThemeList => {
                self.refresh_theme_list_dialog();
                let current_theme = self.context.current_theme_name();
                self.theme_list_dialog.open(&current_theme);
            }
            UiActionId::CycleVariant => {
                self.cycle_model_variant();
            }
            UiActionId::ToggleAppearance => {
                let _ = self.context.toggle_theme_mode();
            }
            UiActionId::ViewStatus => {
                self.refresh_status_dialog();
                self.status_dialog.open();
            }
            UiActionId::ToggleMcp => {
                let _ = self.refresh_mcp_dialog();
                self.mcp_dialog.open();
            }
            UiActionId::ToggleTips => {
                self.context.toggle_tips_hidden();
            }
            UiActionId::OpenModelList => {
                self.refresh_model_dialog();
                self.model_select.open();
            }
            UiActionId::OpenModeList => {
                self.refresh_agent_dialog();
                self.agent_select.open();
            }
            UiActionId::OpenAgentList => {
                self.refresh_agent_dialog();
                self.agent_select.open();
            }
            UiActionId::OpenPresetList => {
                self.refresh_agent_dialog();
                self.agent_select.open();
            }
            UiActionId::NewSession => {
                self.context.navigate(Route::Home);
                self.active_session_id = None;
                self.session_view = None;
            }
            UiActionId::ShowHelp => {
                self.help_dialog.open();
            }
            UiActionId::ToggleCommandPalette => {
                self.sync_command_palette_labels();
                self.command_palette.open();
            }
            UiActionId::ToggleTimestamps => {
                self.context.toggle_timestamps();
            }
            UiActionId::ToggleThinking => {
                self.context.toggle_thinking();
            }
            UiActionId::ToggleToolDetails => {
                self.context.toggle_tool_details();
            }
            UiActionId::ToggleDensity => {
                self.context.toggle_message_density();
            }
            UiActionId::ToggleSemanticHighlight => {
                self.context.toggle_semantic_highlight();
            }
            UiActionId::ExternalEditor => {}
            UiActionId::ConnectProvider => {
                self.populate_provider_dialog();
                self.provider_dialog.open();
            }
            UiActionId::ShareSession => {
                self.handle_share_session();
            }
            UiActionId::UnshareSession => {
                self.handle_unshare_session();
            }
            UiActionId::ForkSession => {
                self.handle_fork_session();
            }
            UiActionId::CompactSession => {
                self.handle_compact_session();
            }
            UiActionId::Timeline => {
                self.handle_open_timeline();
            }
            UiActionId::Undo => {
                self.handle_undo();
            }
            UiActionId::Redo => {
                self.handle_redo();
            }
            UiActionId::CopySession => {
                self.handle_copy_session();
            }
            UiActionId::OpenStash => {
                self.open_prompt_stash_dialog();
            }
            UiActionId::OpenRecoveryList => {
                self.handle_show_recovery_actions();
            }
            UiActionId::OpenSkills => {
                self.open_skill_list_dialog();
            }
            UiActionId::ShowStatus => {
                self.refresh_status_dialog();
                self.status_dialog.open();
            }
            UiActionId::OpenMcpList => {
                let _ = self.refresh_mcp_dialog();
                self.mcp_dialog.open();
            }
            UiActionId::Exit => self.state = AppState::Exiting,
            UiActionId::ListTasks => {
                self.handle_list_tasks();
            }
        }

        Ok(())
    }

    pub(super) fn execute_typed_interactive_command(
        &mut self,
        command: InteractiveCommand,
    ) -> anyhow::Result<bool> {
        if let Some(invocation) = command.ui_action_invocation() {
            self.execute_ui_action_invocation(&invocation)?;
            return Ok(true);
        }

        match command {
            InteractiveCommand::ExecuteRecovery(selector) => {
                self.handle_execute_recovery_action(&selector);
            }
            InteractiveCommand::ListChildSessions
            | InteractiveCommand::FocusChildSession(_)
            | InteractiveCommand::FocusNextChildSession
            | InteractiveCommand::FocusPreviousChildSession
            | InteractiveCommand::BackToRootSession => {
                self.toast.show(
                    ToastVariant::Info,
                    "Child-session focus commands are currently CLI-only.",
                    2400,
                );
            }
            InteractiveCommand::ShowTask(id) => {
                self.handle_show_task(&id);
            }
            InteractiveCommand::KillTask(id) => {
                self.handle_kill_task(&id);
            }
            InteractiveCommand::ClearScreen => {
                // TUI doesn't need clear-screen — no-op
            }
            InteractiveCommand::ToggleActive
            | InteractiveCommand::ScrollUp
            | InteractiveCommand::ScrollDown
            | InteractiveCommand::ScrollBottom => {
                // Layout toggling / scrolling not applicable in TUI — TUI has its own layout
            }
            InteractiveCommand::InspectStage(_stage_id) => {
                // Stage inspection not yet wired in TUI — planned for inspector panel
            }
            InteractiveCommand::Unknown(_)
            | InteractiveCommand::Exit
            | InteractiveCommand::ShowHelp
            | InteractiveCommand::Abort
            | InteractiveCommand::ShowRecovery
            | InteractiveCommand::NewSession
            | InteractiveCommand::ShowStatus
            | InteractiveCommand::ListModels
            | InteractiveCommand::ListProviders
            | InteractiveCommand::ListThemes
            | InteractiveCommand::ListPresets
            | InteractiveCommand::ListSessions
            | InteractiveCommand::ParentSession
            | InteractiveCommand::ListTasks
            | InteractiveCommand::Compact
            | InteractiveCommand::Copy
            | InteractiveCommand::ListAgents
            | InteractiveCommand::ToggleSidebar
            | InteractiveCommand::SelectModel(_)
            | InteractiveCommand::SelectAgent(_)
            | InteractiveCommand::SelectPreset(_) => {
                // Ignore unknown commands in TUI
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(name: &str, kind: ModeKind) -> Agent {
        Agent {
            name: name.to_string(),
            description: String::new(),
            color: ratatui::style::Color::White,
            kind,
            orchestrator: None,
        }
    }

    #[test]
    fn mode_argument_matching_respects_action_scope() {
        let agent = mode("build", ModeKind::Agent);
        let preset = mode("atlas", ModeKind::Preset);
        let profile = mode("prometheus", ModeKind::Profile);

        assert!(mode_matches_action_argument(
            &agent,
            UiActionId::OpenAgentList,
            "build"
        ));
        assert!(mode_matches_action_argument(
            &agent,
            UiActionId::OpenModeList,
            "agent:build"
        ));
        assert!(!mode_matches_action_argument(
            &agent,
            UiActionId::OpenPresetList,
            "build"
        ));

        assert!(mode_matches_action_argument(
            &preset,
            UiActionId::OpenPresetList,
            "atlas"
        ));
        assert!(mode_matches_action_argument(
            &profile,
            UiActionId::OpenPresetList,
            "profile:prometheus"
        ));
        assert!(mode_matches_action_argument(
            &preset,
            UiActionId::OpenModeList,
            "preset:atlas"
        ));
    }

    #[test]
    fn session_target_matching_accepts_id_prefix_and_title() {
        let session = crate::api::SessionInfo {
            id: "sess_abc123".to_string(),
            slug: "sess_abc123".to_string(),
            project_id: "project".to_string(),
            directory: ".".to_string(),
            parent_id: None,
            title: "Atlas Planning".to_string(),
            version: "1".to_string(),
            time: crate::api::SessionTimeInfo {
                created: 0,
                updated: 0,
                compacting: None,
                archived: None,
            },
            revert: None,
            metadata: None,
        };

        assert!(session_matches_target(&session, "sess_abc123"));
        assert!(session_matches_target(&session, "sess_abc"));
        assert!(session_matches_target(&session, "planning"));
        assert!(!session_matches_target(&session, "oracle"));
    }
}
