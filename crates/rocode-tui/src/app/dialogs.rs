use super::*;

use rocode_core::contracts::mcp::McpConnectionStatusWire;

impl App {
    pub(super) fn has_open_dialog_layer(&self) -> bool {
        self.alert_dialog.is_open()
            || self.help_dialog.is_open()
            || self.status_dialog.is_open()
            || self.session_rename_dialog.is_open()
            || self.session_export_dialog.is_open()
            || self.prompt_stash_dialog.is_open()
            || self.skill_list_dialog.is_open()
            || self.slash_popup.is_open()
            || self.command_palette.is_open()
            || self.model_select.is_open()
            || self.agent_select.is_open()
            || self.session_list_dialog.is_open()
            || self.theme_list_dialog.is_open()
            || self.mcp_dialog.is_open()
            || self.timeline_dialog.is_open()
            || self.fork_dialog.is_open()
            || self.provider_dialog.is_open()
            || self.subagent_dialog.is_open()
            || self.tag_dialog.is_open()
            || self.recovery_action_dialog.is_open()
    }

    pub(super) fn close_top_dialog(&mut self) -> bool {
        if self.alert_dialog.is_open() {
            self.alert_dialog.close();
            return true;
        }
        if self.help_dialog.is_open() {
            self.help_dialog.close();
            return true;
        }
        if self.recovery_action_dialog.is_open() {
            self.recovery_action_dialog.close();
            return true;
        }
        if self.status_dialog.is_open() {
            self.status_dialog.close();
            return true;
        }
        if self.session_rename_dialog.is_open() {
            self.session_rename_dialog.close();
            return true;
        }
        if self.session_export_dialog.is_open() {
            self.session_export_dialog.close();
            return true;
        }
        if self.prompt_stash_dialog.is_open() {
            self.prompt_stash_dialog.close();
            return true;
        }
        if self.skill_list_dialog.is_open() {
            self.skill_list_dialog.close();
            return true;
        }
        if self.slash_popup.is_open() {
            self.slash_popup.close();
            return true;
        }
        if self.command_palette.is_open() {
            self.command_palette.close();
            return true;
        }
        if self.model_select.is_open() {
            self.model_select.close();
            return true;
        }
        if self.agent_select.is_open() {
            self.agent_select.close();
            return true;
        }
        if self.session_list_dialog.is_open() {
            self.session_list_dialog.close();
            return true;
        }
        if self.theme_list_dialog.is_open() {
            let initial = self.theme_list_dialog.initial_theme_id().to_string();
            let _ = self.context.set_theme_by_name(&initial);
            self.theme_list_dialog.close();
            return true;
        }
        if self.mcp_dialog.is_open() {
            self.mcp_dialog.close();
            return true;
        }
        if self.timeline_dialog.is_open() {
            self.timeline_dialog.close();
            return true;
        }
        if self.fork_dialog.is_open() {
            self.fork_dialog.close();
            return true;
        }
        if self.provider_dialog.is_open() {
            self.provider_dialog.close();
            return true;
        }
        if self.subagent_dialog.is_open() {
            self.subagent_dialog.close();
            return true;
        }
        if self.tool_call_cancel_dialog.is_open() {
            self.tool_call_cancel_dialog.close();
            return true;
        }
        if self.tag_dialog.is_open() {
            self.tag_dialog.close();
            return true;
        }
        false
    }

    pub(super) fn scroll_active_dialog(&mut self, up: bool) {
        if self.prompt_stash_dialog.is_open() {
            if up {
                self.prompt_stash_dialog.move_up();
            } else {
                self.prompt_stash_dialog.move_down();
            }
            return;
        }
        if self.skill_list_dialog.is_open() {
            if up {
                self.skill_list_dialog.move_up();
            } else {
                self.skill_list_dialog.move_down();
            }
            return;
        }
        if self.slash_popup.is_open() {
            if up {
                self.slash_popup.move_up();
            } else {
                self.slash_popup.move_down();
            }
            return;
        }
        if self.command_palette.is_open() {
            if up {
                self.command_palette.move_up();
            } else {
                self.command_palette.move_down();
            }
            return;
        }
        if self.model_select.is_open() {
            if up {
                self.model_select.move_up();
            } else {
                self.model_select.move_down();
            }
            return;
        }
        if self.agent_select.is_open() {
            if up {
                self.agent_select.move_up();
            } else {
                self.agent_select.move_down();
            }
            return;
        }
        if self.session_list_dialog.is_open() {
            if self.session_list_dialog.is_renaming() {
                return;
            }
            if up {
                self.session_list_dialog.move_up();
            } else {
                self.session_list_dialog.move_down();
            }
            return;
        }
        if self.theme_list_dialog.is_open() {
            if up {
                self.theme_list_dialog.move_up();
            } else {
                self.theme_list_dialog.move_down();
            }
            if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                let _ = self.context.set_theme_by_name(&theme_id);
            }
            return;
        }
        if self.mcp_dialog.is_open() {
            if up {
                self.mcp_dialog.move_up();
            } else {
                self.mcp_dialog.move_down();
            }
            return;
        }
        if self.timeline_dialog.is_open() {
            if up {
                self.timeline_dialog.move_up();
            } else {
                self.timeline_dialog.move_down();
            }
            return;
        }
        if self.fork_dialog.is_open() {
            if up {
                self.fork_dialog.move_up();
            } else {
                self.fork_dialog.move_down();
            }
            return;
        }
        if self.provider_dialog.is_open() {
            if up {
                self.provider_dialog.move_up();
            } else {
                self.provider_dialog.move_down();
            }
            return;
        }
        if self.subagent_dialog.is_open() {
            if up {
                self.subagent_dialog.scroll_up();
            } else {
                self.subagent_dialog.scroll_down(50);
            }
            return;
        }
        if self.tag_dialog.is_open() {
            if up {
                self.tag_dialog.move_up();
            } else {
                self.tag_dialog.move_down();
            }
        }
    }

    pub(super) fn handle_dialog_mouse(
        &mut self,
        mouse_event: &crossterm::event::MouseEvent,
    ) -> anyhow::Result<bool> {
        use crossterm::event::{MouseButton, MouseEventKind};

        if !self.has_open_dialog_layer() {
            return Ok(false);
        }

        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_active_dialog(true);
                self.event_caused_change = true;
                Ok(true)
            }
            MouseEventKind::ScrollDown => {
                self.scroll_active_dialog(false);
                self.event_caused_change = true;
                Ok(true)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.event_caused_change = self.close_top_dialog();
                Ok(true)
            }
            MouseEventKind::Moved
            | MouseEventKind::Down(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Up(_) => {
                self.event_caused_change = false;
                Ok(true)
            }
            _ => {
                self.event_caused_change = false;
                Ok(true)
            }
        }
    }

    pub(super) fn handle_dialog_key(&mut self, key: KeyEvent) -> anyhow::Result<bool> {
        if self.alert_dialog.is_open() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.alert_dialog.close();
                }
                KeyCode::Char('c')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && !self.alert_dialog.message().is_empty() =>
                {
                    let _ = Clipboard::write_text(self.alert_dialog.message());
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.help_dialog.is_open() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.help_dialog.close();
            }
            return Ok(true);
        }
        if self.status_dialog.is_open() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.status_dialog.close();
            }
            return Ok(true);
        }
        if self.session_rename_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.session_rename_dialog.close(),
                KeyCode::Backspace => self.session_rename_dialog.handle_backspace(),
                KeyCode::Enter => {
                    if let Some((session_id, title)) = self.session_rename_dialog.confirm() {
                        if let Some(client) = self.context.get_api_client() {
                            if let Err(err) = client.update_session_title(&session_id, &title) {
                                self.alert_dialog.set_message(&format!(
                                    "Failed to rename session `{}`:\n{}",
                                    session_id, err
                                ));
                                self.alert_dialog.open();
                            } else {
                                self.refresh_session_list_dialog();
                                let _ = self.sync_session_from_server(&session_id);
                            }
                        }
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.session_rename_dialog.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.session_export_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.session_export_dialog.close(),
                KeyCode::Backspace => self.session_export_dialog.handle_backspace(),
                KeyCode::Enter => {
                    if let Some(session_id) = self.session_export_dialog.session_id() {
                        let filename = self.session_export_dialog.filename().trim();
                        if filename.is_empty() {
                            self.alert_dialog
                                .set_message("Filename cannot be empty for export.");
                            self.alert_dialog.open();
                        } else {
                            match self.export_session_to_file(session_id, filename) {
                                Ok(path) => {
                                    self.alert_dialog.set_message(&format!(
                                        "Session exported to `{}`.",
                                        path.display()
                                    ));
                                    self.alert_dialog.open();
                                    self.session_export_dialog.close();
                                }
                                Err(err) => {
                                    self.alert_dialog.set_message(&format!(
                                        "Failed to export session:\n{}",
                                        err
                                    ));
                                    self.alert_dialog.open();
                                }
                            }
                        }
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(session_id) = self.session_export_dialog.session_id() {
                        match self.build_session_transcript(session_id) {
                            Some(text) => {
                                if let Err(err) = Clipboard::write_text(&text) {
                                    self.alert_dialog.set_message(&format!(
                                        "Failed to copy transcript to clipboard:\n{}",
                                        err
                                    ));
                                    self.alert_dialog.open();
                                } else {
                                    self.alert_dialog
                                        .set_message("Session transcript copied to clipboard.");
                                    self.alert_dialog.open();
                                }
                            }
                            None => {
                                self.alert_dialog
                                    .set_message("No transcript available for current session.");
                                self.alert_dialog.open();
                            }
                        }
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.session_export_dialog.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.tool_call_cancel_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.tool_call_cancel_dialog.close(),
                KeyCode::Up => self.tool_call_cancel_dialog.previous(),
                KeyCode::Down => self.tool_call_cancel_dialog.next(),
                KeyCode::Enter => {
                    if let Some(tool_call_id) = self.tool_call_cancel_dialog.selected() {
                        if let Some(session_id) = &self.active_session_id {
                            if let Some(api) = self.context.get_api_client() {
                                if let Err(e) = api.cancel_tool_call(session_id, &tool_call_id) {
                                    self.toast.show(
                                        ToastVariant::Error,
                                        &format!("Failed to cancel tool: {}", e),
                                        3000,
                                    );
                                } else {
                                    self.toast.show(
                                        ToastVariant::Info,
                                        "Tool cancellation requested",
                                        3000,
                                    );
                                }
                            }
                        }
                        self.tool_call_cancel_dialog.close();
                    }
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.prompt_stash_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.prompt_stash_dialog.close(),
                KeyCode::Up => self.prompt_stash_dialog.move_up(),
                KeyCode::Down => self.prompt_stash_dialog.move_down(),
                KeyCode::Backspace => self.prompt_stash_dialog.handle_backspace(),
                KeyCode::Enter => {
                    if let Some(index) = self.prompt_stash_dialog.selected_index() {
                        if self.prompt.load_stash(index) {
                            self.prompt_stash_dialog.close();
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(index) = self.prompt_stash_dialog.selected_index() {
                        if self.prompt.remove_stash(index) {
                            let entries = self
                                .prompt
                                .stash_entries()
                                .iter()
                                .cloned()
                                .map(|entry| StashItem {
                                    input: entry.input,
                                    created_at: entry.created_at,
                                })
                                .collect::<Vec<_>>();
                            self.prompt_stash_dialog.set_entries(entries);
                        }
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.prompt_stash_dialog.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.skill_list_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.skill_list_dialog.close(),
                KeyCode::Up => self.skill_list_dialog.move_up(),
                KeyCode::Down => self.skill_list_dialog.move_down(),
                KeyCode::Backspace => self.skill_list_dialog.handle_backspace(),
                KeyCode::Enter => {
                    if let Some(skill) = self.skill_list_dialog.selected_skill() {
                        self.prompt.set_input(format!("/{} ", skill));
                        self.skill_list_dialog.close();
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.skill_list_dialog.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.slash_popup.is_open() {
            match key.code {
                KeyCode::Esc => self.slash_popup.close(),
                KeyCode::Up => self.slash_popup.move_up(),
                KeyCode::Down => self.slash_popup.move_down(),
                KeyCode::Backspace => {
                    if !self.slash_popup.handle_backspace() {
                        self.slash_popup.close();
                    }
                }
                KeyCode::Enter => {
                    self.slash_popup.select_current();
                    if let Some(action) = self.slash_popup.take_action() {
                        self.execute_ui_action(action)?;
                    }
                }
                KeyCode::Char(' ')
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    // Space means the user wants to type arguments (e.g. /model foo).
                    // Close the popup and inject "/{query} " into the prompt so they
                    // can continue typing.  On Enter the full text goes through
                    // parse_interactive_command() which supports parameters.
                    let query = self.slash_popup.query().to_string();
                    self.slash_popup.close();
                    self.prompt.set_input(format!("/{query} "));
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.slash_popup.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.recovery_action_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.recovery_action_dialog.close(),
                KeyCode::Up => self.recovery_action_dialog.previous(),
                KeyCode::Down => self.recovery_action_dialog.next(),
                KeyCode::Enter => {
                    let selected = self.recovery_action_dialog.selected();
                    if let Some(selector) = selected.as_deref() {
                        self.handle_execute_recovery_action(selector);
                        self.recovery_action_dialog.close();
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.command_palette.is_open() {
            match key.code {
                KeyCode::Esc => self.command_palette.close(),
                KeyCode::Up => self.command_palette.move_up(),
                KeyCode::Down => self.command_palette.move_down(),
                KeyCode::Backspace => self.command_palette.handle_backspace(),
                KeyCode::Enter => {
                    let action = self.command_palette.selected_action();
                    self.command_palette.close();
                    if let Some(action) = action {
                        self.execute_ui_action(action)?;
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.command_palette.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.model_select.is_open() {
            match key.code {
                KeyCode::Esc => self.model_select.close(),
                KeyCode::Up => self.model_select.move_up(),
                KeyCode::Down => self.model_select.move_down(),
                KeyCode::Backspace => self.model_select.handle_backspace(),
                KeyCode::Enter => {
                    if let Some(model) = self.model_select.selected_model() {
                        let provider = model.provider.clone();
                        let id = model.id.clone();
                        let model_ref = format!("{}/{}", provider, id);
                        self.model_select.push_recent(&provider, &id);
                        self.model_select.set_current_model(Some(model_ref.clone()));
                        self.context.save_recent_models(self.model_select.recent());
                        self.set_active_model_selection(model_ref, Some(provider));
                    }
                    self.model_select.close();
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.model_select.handle_input(c);
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.agent_select.is_open() {
            match key.code {
                KeyCode::Esc => self.agent_select.close(),
                KeyCode::Up => self.agent_select.move_up(),
                KeyCode::Down => self.agent_select.move_down(),
                KeyCode::Enter => {
                    if let Some(agent) = self.agent_select.selected_agent() {
                        apply_selected_mode(&self.context, agent);
                    }
                    self.agent_select.close();
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.session_list_dialog.is_open() {
            if self.session_list_dialog.is_renaming() {
                match key.code {
                    KeyCode::Esc => self.session_list_dialog.cancel_rename(),
                    KeyCode::Backspace => self.session_list_dialog.handle_rename_backspace(),
                    KeyCode::Enter => {
                        if let Some((session_id, title)) = self.session_list_dialog.confirm_rename()
                        {
                            if let Some(client) = self.context.get_api_client() {
                                if let Err(err) = client.update_session_title(&session_id, &title) {
                                    self.alert_dialog.set_message(&format!(
                                        "Failed to rename session `{}`:\n{}",
                                        session_id, err
                                    ));
                                    self.alert_dialog.open();
                                } else {
                                    self.refresh_session_list_dialog();
                                    if self.active_session_id.as_deref()
                                        == Some(session_id.as_str())
                                    {
                                        let _ = self.sync_session_from_server(&session_id);
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        self.session_list_dialog.handle_rename_input(c);
                    }
                    _ => {}
                }
                return Ok(true);
            }

            match key.code {
                KeyCode::Esc => self.session_list_dialog.close(),
                KeyCode::Up => self.session_list_dialog.move_up(),
                KeyCode::Down => self.session_list_dialog.move_down(),
                KeyCode::Backspace => {
                    self.session_list_dialog.handle_backspace();
                    self.refresh_session_list_dialog();
                }
                KeyCode::Enter => {
                    let target = self.session_list_dialog.selected_session_id();
                    self.session_list_dialog.close();
                    if let Some(session_id) = target {
                        self.context.navigate(Route::Session {
                            session_id: session_id.clone(),
                        });
                        self.ensure_session_view(&session_id);
                        let _ = self.sync_session_from_server(&session_id);
                    }
                }
                KeyCode::Char('r') if self.matches_keybind("session_rename", key) => {
                    let _ = self.session_list_dialog.start_rename_selected();
                }
                KeyCode::Char('d') if self.matches_keybind("session_delete", key) => {
                    if let Some(state) = self.session_list_dialog.trigger_delete_selected() {
                        match state {
                            SessionDeleteState::Armed(_) => {}
                            SessionDeleteState::Confirmed(session_id) => {
                                if let Some(client) = self.context.get_api_client() {
                                    if let Err(err) = client.delete_session(&session_id) {
                                        self.alert_dialog.set_message(&format!(
                                            "Failed to delete session `{}`:\n{}",
                                            session_id, err
                                        ));
                                        self.alert_dialog.open();
                                    } else {
                                        if self.active_session_id.as_deref()
                                            == Some(session_id.as_str())
                                        {
                                            self.context.navigate(Route::Home);
                                            self.active_session_id = None;
                                            self.session_view = None;
                                        }
                                        self.refresh_session_list_dialog();
                                    }
                                }
                            }
                        }
                    }
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.session_list_dialog.handle_input(c);
                    self.refresh_session_list_dialog();
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.theme_list_dialog.is_open() {
            match key.code {
                KeyCode::Esc => {
                    let initial = self.theme_list_dialog.initial_theme_id().to_string();
                    let _ = self.context.set_theme_by_name(&initial);
                    self.theme_list_dialog.close();
                }
                KeyCode::Up => {
                    self.theme_list_dialog.move_up();
                    if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                        let _ = self.context.set_theme_by_name(&theme_id);
                    }
                }
                KeyCode::Down => {
                    self.theme_list_dialog.move_down();
                    if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                        let _ = self.context.set_theme_by_name(&theme_id);
                    }
                }
                KeyCode::Backspace => {
                    self.theme_list_dialog.handle_backspace();
                    if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                        let _ = self.context.set_theme_by_name(&theme_id);
                    }
                }
                KeyCode::Enter => {
                    if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                        let _ = self.context.commit_theme_by_name(&theme_id);
                    }
                    self.theme_list_dialog.close();
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.theme_list_dialog.handle_input(c);
                    if let Some(theme_id) = self.theme_list_dialog.selected_theme_id() {
                        let _ = self.context.set_theme_by_name(&theme_id);
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.mcp_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.mcp_dialog.close(),
                KeyCode::Up => self.mcp_dialog.move_up(),
                KeyCode::Down => self.mcp_dialog.move_down(),
                KeyCode::Char('r') => {
                    let _ = self.refresh_mcp_dialog();
                }
                KeyCode::Char('a') => {
                    if let Some(item) = self.mcp_dialog.selected_item() {
                        if let Some(client) = self.context.get_api_client() {
                            match client.start_mcp_auth(&item.name) {
                                Ok(auth) => {
                                    self.alert_dialog.set_message(&format!(
                                        "MCP `{}` auth started:\n{}\n\nComplete OAuth, then reconnect.",
                                        item.name, auth.authorization_url
                                    ));
                                    self.alert_dialog.open();
                                }
                                Err(err) => {
                                    self.alert_dialog.set_message(&format!(
                                        "Failed to start MCP auth `{}`:\n{}",
                                        item.name, err
                                    ));
                                    self.alert_dialog.open();
                                }
                            }
                            let _ = client.authenticate_mcp(&item.name);
                            let _ = self.refresh_mcp_dialog();
                        }
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(item) = self.mcp_dialog.selected_item() {
                        if let Some(client) = self.context.get_api_client() {
                            if let Err(err) = client.remove_mcp_auth(&item.name) {
                                self.alert_dialog.set_message(&format!(
                                    "Failed to clear MCP auth `{}`:\n{}",
                                    item.name, err
                                ));
                                self.alert_dialog.open();
                            }
                            let _ = self.refresh_mcp_dialog();
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(item) = self.mcp_dialog.selected_item() {
                        if let Some(client) = self.context.get_api_client() {
                            let result = if McpConnectionStatusWire::parse(&item.status)
                                == Some(McpConnectionStatusWire::Connected)
                            {
                                client.disconnect_mcp(&item.name)
                            } else {
                                client.connect_mcp(&item.name)
                            };
                            if let Err(err) = result {
                                self.alert_dialog.set_message(&format!(
                                    "Failed to toggle MCP `{}`:\n{}",
                                    item.name, err
                                ));
                                self.alert_dialog.open();
                            }
                            let _ = self.refresh_mcp_dialog();
                        }
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.timeline_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.timeline_dialog.close(),
                KeyCode::Up => self.timeline_dialog.move_up(),
                KeyCode::Down => self.timeline_dialog.move_down(),
                KeyCode::Enter => {
                    if let Some(msg_id) = self.timeline_dialog.selected_message_id() {
                        let msg_id = msg_id.to_string();
                        self.timeline_dialog.close();
                        if let Some(ref mut sv) = self.session_view {
                            sv.scroll_to_message(&msg_id);
                        }
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.fork_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.fork_dialog.close(),
                KeyCode::Up => self.fork_dialog.move_up(),
                KeyCode::Down => self.fork_dialog.move_down(),
                KeyCode::Enter => {
                    let session_id = self.fork_dialog.session_id().map(|s| s.to_string());
                    let msg_id = self
                        .fork_dialog
                        .selected_message_id()
                        .map(|s| s.to_string());
                    self.fork_dialog.close();
                    if let Some(sid) = session_id {
                        if let Some(client) = self.context.get_api_client() {
                            match client.fork_session(&sid, msg_id.as_deref()) {
                                Ok(new_session) => {
                                    self.cache_session_from_api(&new_session);
                                    self.context.navigate(Route::Session {
                                        session_id: new_session.id.clone(),
                                    });
                                    self.ensure_session_view(&new_session.id);
                                    let _ = self.sync_session_from_server(&new_session.id);
                                    self.alert_dialog.set_message(&format!(
                                        "Forked session created: {}",
                                        new_session.title
                                    ));
                                    self.alert_dialog.open();
                                }
                                Err(err) => {
                                    self.alert_dialog
                                        .set_message(&format!("Failed to fork session:\n{}", err));
                                    self.alert_dialog.open();
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            return Ok(true);
        }

        if self.provider_dialog.is_open() {
            if self.provider_dialog.is_input_mode() {
                // API key input mode
                match key.code {
                    KeyCode::Esc => self.provider_dialog.exit_input_mode(),
                    KeyCode::Backspace => {
                        self.provider_dialog.pop_char();
                    }
                    KeyCode::Enter => {
                        if let Some((provider_id, api_key)) = self.provider_dialog.pending_submit()
                        {
                            self.submit_provider_auth(&provider_id, &api_key);
                        }
                    }
                    KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.paste_clipboard_to_provider_dialog();
                    }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.provider_dialog.push_char(c);
                    }
                    _ => {}
                }
            } else {
                // Provider list selection mode
                match key.code {
                    KeyCode::Esc => self.provider_dialog.close(),
                    KeyCode::Up => self.provider_dialog.move_up(),
                    KeyCode::Down => self.provider_dialog.move_down(),
                    KeyCode::Enter => {
                        self.provider_dialog.enter_input_mode();
                    }
                    _ => {}
                }
            }
            return Ok(true);
        }

        if self.subagent_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.subagent_dialog.close(),
                KeyCode::Up => self.subagent_dialog.scroll_up(),
                KeyCode::Down => self.subagent_dialog.scroll_down(50),
                _ => {}
            }
            return Ok(true);
        }

        if self.tag_dialog.is_open() {
            match key.code {
                KeyCode::Esc => self.tag_dialog.close(),
                KeyCode::Up => self.tag_dialog.move_up(),
                KeyCode::Down => self.tag_dialog.move_down(),
                KeyCode::Char(' ') => self.tag_dialog.toggle_selection(),
                KeyCode::Enter => self.tag_dialog.close(),
                _ => {}
            }
            return Ok(true);
        }

        Ok(false)
    }
}
