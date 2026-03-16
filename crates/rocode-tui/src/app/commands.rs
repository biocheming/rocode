use super::*;

impl App {
    pub(super) fn execute_ui_action(&mut self, action: UiActionId) -> anyhow::Result<()> {
        match action {
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
        match command {
            InteractiveCommand::Exit | InteractiveCommand::ShowHelp => {
                if let Some(action_id) = command.ui_action_id() {
                    self.execute_ui_action(action_id)?;
                }
            }
            InteractiveCommand::Abort => {
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
                                let message = value
                                    .get("target")
                                    .and_then(|value| value.as_str())
                                    .map(|target| match target {
                                        "stage" => {
                                            let stage = value
                                                .get("stage")
                                                .and_then(|value| value.as_str())
                                                .unwrap_or("current stage");
                                            format!("Stage cancellation requested: {}", stage)
                                        }
                                        _ => "Run cancellation requested".to_string(),
                                    })
                                    .unwrap_or_else(|| "Run cancellation requested".to_string());
                                self.toast.show(ToastVariant::Info, &message, 3000);
                            }
                        }
                    }
                }
            }
            InteractiveCommand::ShowRecovery => {
                if let Some(action_id) = command.ui_action_id() {
                    self.execute_ui_action(action_id)?;
                }
            }
            InteractiveCommand::ExecuteRecovery(selector) => {
                self.handle_execute_recovery_action(&selector);
            }
            InteractiveCommand::NewSession
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
            | InteractiveCommand::ToggleSidebar => {
                if let Some(action_id) = command.ui_action_id() {
                    self.execute_ui_action(action_id)?;
                }
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
            InteractiveCommand::SelectModel(model_ref) => {
                self.set_active_model_selection(model_ref.clone(), provider_from_model(&model_ref));
                self.toast.show(
                    ToastVariant::Success,
                    &format!("Model set to {}", model_ref),
                    1800,
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
            InteractiveCommand::SelectAgent(name) => {
                if let Some(mode) = self
                    .agent_select
                    .agents()
                    .iter()
                    .find(|mode| mode.kind == ModeKind::Agent && mode.name == name)
                {
                    apply_selected_mode(&self.context, mode);
                    self.toast.show(
                        ToastVariant::Success,
                        &format!("Agent set to {}", mode.name),
                        1800,
                    );
                }
            }
            InteractiveCommand::SelectPreset(name) => {
                if let Some(mode) = self.agent_select.agents().iter().find(|mode| {
                    matches!(mode.kind, ModeKind::Preset | ModeKind::Profile) && mode.name == name
                }) {
                    apply_selected_mode(&self.context, mode);
                    self.toast.show(
                        ToastVariant::Success,
                        &format!("Preset set to {}", mode.name),
                        1800,
                    );
                }
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
            InteractiveCommand::Unknown(_) => {
                // Ignore unknown commands in TUI
            }
        }

        Ok(true)
    }
}
