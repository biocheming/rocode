use super::*;

impl App {
    pub(super) fn open_overview_status_dialog(&mut self) {
        self.status_dialog_view = StatusDialogView::Overview;
        self.refresh_active_status_dialog();
        self.status_dialog.open();
    }

    pub(super) fn open_runtime_status_dialog(&mut self) -> bool {
        if self.render_runtime_status_dialog() {
            self.status_dialog_view = StatusDialogView::Runtime;
            self.status_dialog.open();
            true
        } else {
            false
        }
    }

    pub(super) fn open_usage_status_dialog(&mut self) -> bool {
        if self.render_usage_status_dialog() {
            self.status_dialog_view = StatusDialogView::Usage;
            self.status_dialog.open();
            true
        } else {
            false
        }
    }

    pub(super) fn open_events_status_dialog(&mut self, raw_filter: Option<&str>) -> bool {
        let Some(session_id) = self.current_session_id() else {
            self.toast.show(
                ToastVariant::Warning,
                "No active session available for /events.",
                2400,
            );
            return false;
        };
        let Some(client) = self.context.get_api_client() else {
            self.toast
                .show(ToastVariant::Error, "API unavailable for /events.", 2400);
            return false;
        };

        let command = rocode_command::interactive::parse_events_browser_command(raw_filter);
        let remembered = match &self.status_dialog_view {
            StatusDialogView::Events(state) if state.session_id == session_id => {
                Some(state.clone())
            }
            _ => None,
        };

        let (filter, offset, preserve_previous_state, empty_page_message) = match command {
            rocode_command::interactive::InteractiveEventsCommand::ShowCurrent => {
                if let Some(state) = remembered.as_ref() {
                    (state.filter.clone(), state.offset, false, None)
                } else {
                    (rocode_command::interactive::default_events_browser_query(), 0, false, None)
                }
            }
            rocode_command::interactive::InteractiveEventsCommand::ShowFiltered {
                filter,
                page,
            } => (
                filter.clone(),
                rocode_command::interactive::events_browser_offset_for_page(&filter, page),
                false,
                (page > 1).then(|| {
                    format!(
                        "Requested page {} has no events for the current filter. Use /events first, prev, or reduce page.",
                        page
                    )
                }),
            ),
            rocode_command::interactive::InteractiveEventsCommand::JumpPage(page) => {
                let filter = remembered
                    .as_ref()
                    .map(|state| state.filter.clone())
                    .unwrap_or_else(rocode_command::interactive::default_events_browser_query);
                (
                    filter.clone(),
                    rocode_command::interactive::events_browser_offset_for_page(&filter, page),
                    false,
                    (page > 1).then(|| {
                        format!(
                            "Requested page {} has no events for the current filter. Use /events first, prev, or change filters.",
                            page
                        )
                    }),
                )
            }
            rocode_command::interactive::InteractiveEventsCommand::NextPage => {
                if let Some(state) = remembered.as_ref() {
                    (
                        state.filter.clone(),
                        state.offset.saturating_add(
                            rocode_command::interactive::events_browser_page_size(&state.filter),
                        ),
                        true,
                        None,
                    )
                } else {
                    (rocode_command::interactive::default_events_browser_query(), 0, false, None)
                }
            }
            rocode_command::interactive::InteractiveEventsCommand::PreviousPage => {
                if let Some(state) = remembered.as_ref() {
                    let step =
                        rocode_command::interactive::events_browser_page_size(&state.filter);
                    (
                        state.filter.clone(),
                        state.offset.saturating_sub(step),
                        false,
                        None,
                    )
                } else {
                    (rocode_command::interactive::default_events_browser_query(), 0, false, None)
                }
            }
            rocode_command::interactive::InteractiveEventsCommand::FirstPage => {
                if let Some(state) = remembered.as_ref() {
                    (state.filter.clone(), 0, false, None)
                } else {
                    (rocode_command::interactive::default_events_browser_query(), 0, false, None)
                }
            }
            rocode_command::interactive::InteractiveEventsCommand::Clear => {
                (rocode_command::interactive::default_events_browser_query(), 0, false, None)
            }
        };

        let query = tui_events_query(&filter, offset);
        match client.get_session_events(&session_id, &query) {
            Ok(events) => {
                if events.is_empty() && offset > 0 {
                    self.toast.show(
                        ToastVariant::Warning,
                        &empty_page_message.unwrap_or_else(|| {
                            if preserve_previous_state {
                                "No more events for the current filter. Use prev or change filters."
                                    .to_string()
                            } else {
                                "That event page is empty for the current filter. Use first, prev, or adjust filters."
                                    .to_string()
                            }
                        }),
                        2800,
                    );
                    return false;
                }

                let page_size = rocode_command::interactive::events_browser_page_size(&filter);
                let page_index =
                    rocode_command::interactive::events_browser_page_for_offset(&filter, offset);
                let can_go_prev = offset > 0;
                let can_go_next = events.len() >= page_size;
                let mut lines = vec![
                    StatusLine::title("Session Events"),
                    StatusLine::normal(format!("Session: {}", session_id)),
                    StatusLine::muted(format!(
                        "Page {} · {} · {}",
                        page_index,
                        tui_events_window_label(offset, events.len()),
                        tui_events_filter_label(&filter)
                    )),
                ];
                lines.extend(tui_event_status_lines(&events));
                if events.is_empty() {
                    lines.push(StatusLine::muted("No matching events."));
                }
                lines.push(StatusLine::muted(String::new()));
                lines.push(StatusLine::muted(format!(
                    "Navigation: {}{}{}",
                    if can_go_prev { "prev" } else { "first page" },
                    if can_go_next { " · next" } else { "" },
                    " · page <n> · clear"
                )));

                self.status_dialog_view = StatusDialogView::Events(TuiEventsBrowserState {
                    session_id,
                    filter,
                    offset,
                });
                self.status_dialog.set_title("Events");
                self.status_dialog.set_footer_hint(Some(
                    "Esc close · ←/p prev · →/n next · Home/0 first · c clear".to_string(),
                ));
                self.status_dialog.set_status_lines(lines);
                true
            }
            Err(error) => {
                self.toast.show(
                    ToastVariant::Error,
                    &format!("Failed to load session events: {}", error),
                    3000,
                );
                false
            }
        }
    }

    pub(super) fn refresh_active_status_dialog(&mut self) {
        match self.status_dialog_view.clone() {
            StatusDialogView::Overview => self.refresh_status_dialog(),
            StatusDialogView::Runtime => {
                let _ = self.render_runtime_status_dialog();
            }
            StatusDialogView::Usage => {
                let _ = self.render_usage_status_dialog();
            }
            StatusDialogView::Events(_) => {
                let _ = self.open_events_status_dialog(None);
            }
        }
    }

    pub(super) fn refresh_status_dialog(&mut self) {
        self.status_dialog.set_title("Status");
        self.status_dialog.set_footer_hint(None);
        let formatters = self
            .context
            .get_api_client()
            .and_then(|client| client.get_formatters().ok())
            .unwrap_or_default();
        let route_label = match self.context.current_route() {
            Route::Home => "home".to_string(),
            Route::Session { session_id } => format!("session ({})", session_id),
            Route::Settings => "settings".to_string(),
            Route::Help => "help".to_string(),
        };
        let session_ctx = self.context.session.read();
        let mcp_servers = self.context.mcp_servers.read();
        let lsp_status = self.context.lsp_status.read();
        let connected_mcp = mcp_servers
            .iter()
            .filter(|s| matches!(s.status, McpConnectionStatus::Connected))
            .count();
        let mut status_blocks = vec![
            StatusBlock::title("Runtime"),
            StatusBlock::normal(format!("Route: {}", route_label)),
            StatusBlock::normal(format!(
                "Directory: {}",
                self.context.directory.read().as_str()
            )),
            StatusBlock::normal(format!("Mode: {}", {
                current_mode_label(&self.context).unwrap_or_else(|| "auto".to_string())
            })),
            StatusBlock::normal(format!("Model: {}", self.current_model_label())),
            StatusBlock::normal(format!(
                "Theme: {}",
                format_theme_option_label(&self.context.current_theme_name())
            )),
            StatusBlock::normal(format!("Loaded sessions: {}", session_ctx.sessions.len())),
            StatusBlock::muted(""),
            StatusBlock::title(format!(
                "MCP Servers ({}, connected: {})",
                mcp_servers.len(),
                connected_mcp
            )),
        ];
        if mcp_servers.is_empty() {
            status_blocks.push(StatusBlock::muted("- No MCP servers"));
        } else {
            for server in mcp_servers.iter() {
                let status_text = match server.status {
                    McpConnectionStatus::Connected => "connected",
                    McpConnectionStatus::Disconnected => "disconnected",
                    McpConnectionStatus::Failed => "failed",
                    McpConnectionStatus::NeedsAuth => "needs authentication",
                    McpConnectionStatus::NeedsClientRegistration => "needs client ID",
                    McpConnectionStatus::Disabled => "disabled",
                };
                let base = format!("- {}: {}", server.name, status_text);
                match server.status {
                    McpConnectionStatus::Connected => {
                        status_blocks.push(StatusBlock::success(base))
                    }
                    McpConnectionStatus::NeedsAuth
                    | McpConnectionStatus::NeedsClientRegistration => {
                        status_blocks.push(StatusBlock::warning(base))
                    }
                    McpConnectionStatus::Failed => {
                        let text = if let Some(error) = &server.error {
                            format!("{} ({})", base, error)
                        } else {
                            base
                        };
                        status_blocks.push(StatusBlock::error(text));
                    }
                    _ => status_blocks.push(StatusBlock::muted(base)),
                }
            }
        }

        status_blocks.push(StatusBlock::muted(""));
        status_blocks.push(StatusBlock::title(format!(
            "LSP Servers ({})",
            lsp_status.len()
        )));
        if lsp_status.is_empty() {
            status_blocks.push(StatusBlock::muted("- No LSP servers"));
        } else {
            for server in lsp_status.iter() {
                status_blocks.push(StatusBlock::success(format!("- {}", server.id)));
            }
        }

        status_blocks.push(StatusBlock::muted(""));
        status_blocks.push(StatusBlock::title(format!(
            "Formatters ({})",
            formatters.len()
        )));
        if formatters.is_empty() {
            status_blocks.push(StatusBlock::muted("- No formatters"));
        } else {
            for formatter in formatters {
                status_blocks.push(StatusBlock::success(format!("- {}", formatter)));
            }
        }
        if let Route::Session { session_id } = self.context.current_route() {
            status_blocks.push(StatusBlock::muted(""));
            status_blocks.extend(self.execution_status_blocks(&session_id));
            status_blocks.push(StatusBlock::muted(""));
            status_blocks.extend(self.session_telemetry_status_blocks());
            status_blocks.push(StatusBlock::muted(""));
            status_blocks.extend(self.recovery_status_blocks(&session_id));
        }
        let lines = status_blocks
            .into_iter()
            .map(status_line_from_block)
            .collect::<Vec<_>>();
        self.status_dialog.set_status_lines(lines);
    }

    pub(super) fn execution_status_blocks(&self, session_id: &str) -> Vec<StatusBlock> {
        let topology = match self.context.execution_topology.read().clone() {
            Some(topology) => topology,
            None => {
                let Some(client) = self.context.get_api_client() else {
                    return vec![
                        StatusBlock::title("Execution Topology"),
                        StatusBlock::muted("- API unavailable"),
                    ];
                };
                match client.get_session_telemetry(session_id) {
                    Ok(telemetry) => {
                        let topology = telemetry.topology.clone();
                        self.context.apply_session_telemetry_snapshot(telemetry);
                        topology
                    }
                    Err(error) => {
                        return vec![
                            StatusBlock::title("Execution Topology"),
                            StatusBlock::error(format!("- Failed to load: {}", error)),
                        ];
                    }
                }
            }
        };

        let mut blocks = vec![StatusBlock::title(format!(
            "Execution Topology (active: {}, running: {}, waiting: {}, cancelling: {}, retry: {})",
            topology.active_count,
            topology.running_count,
            topology.waiting_count,
            topology.cancelling_count,
            topology.retry_count
        ))];

        if topology.roots.is_empty() {
            blocks.push(StatusBlock::muted("- No active executions"));
            return blocks;
        }

        for (index, root) in topology.roots.iter().enumerate() {
            append_execution_status_node(&mut blocks, root, "", index + 1 == topology.roots.len());
        }

        blocks
    }

    pub(super) fn session_telemetry_status_blocks(&self) -> Vec<StatusBlock> {
        let runtime = self.context.session_runtime.read().clone();
        let usage = self.context.session_usage.read().clone();
        let stages = self.context.stage_summaries.read().clone();
        let Some(runtime) = runtime else {
            return vec![
                StatusBlock::title("Session Telemetry"),
                StatusBlock::muted("- Telemetry snapshot not loaded"),
            ];
        };

        let mut blocks = vec![StatusBlock::title(format!(
            "Session Telemetry ({})",
            format_run_status(&runtime.run_status)
        ))];

        if let Some(stage_id) = runtime.active_stage_id.as_deref() {
            blocks.push(StatusBlock::normal(format!(
                "Active stage: {} ({} active)",
                stage_id, runtime.active_stage_count
            )));
        } else {
            blocks.push(StatusBlock::muted(format!(
                "- No active stage ({})",
                runtime.active_stage_count
            )));
        }

        if let Some(usage) = usage.as_ref() {
            blocks.push(StatusBlock::normal(format!(
                "Usage: in {} out {} reasoning {} cache {}/{} cost ${:.4}",
                usage.input_tokens,
                usage.output_tokens,
                usage.reasoning_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
                usage.total_cost
            )));
        }

        if let Some(active_stage_id) = runtime.active_stage_id.as_deref() {
            if let Some(stage) = stages
                .iter()
                .find(|stage| stage.stage_id == active_stage_id)
            {
                blocks.extend(active_stage_status_blocks(stage));
            }
        }

        if !stages.is_empty() {
            blocks.push(StatusBlock::title(format!(
                "Stage Summaries ({})",
                stages.len()
            )));
            for stage in stages.iter().take(5) {
                blocks.push(StatusBlock::normal(format_stage_summary_line(stage)));
            }
            if stages.len() > 5 {
                blocks.push(StatusBlock::muted(format!(
                    "- {} more stage summaries",
                    stages.len() - 5
                )));
            }
        }

        blocks
    }

    fn render_runtime_status_dialog(&mut self) -> bool {
        let Some(session_id) = self.current_session_id() else {
            self.toast.show(
                ToastVariant::Warning,
                "No active session available for /runtime.",
                2400,
            );
            return false;
        };

        let Some(client) = self.context.get_api_client() else {
            self.toast
                .show(ToastVariant::Error, "API unavailable for /runtime.", 2400);
            return false;
        };

        match client.get_session_telemetry(&session_id) {
            Ok(telemetry) => {
                self.context
                    .apply_session_telemetry_snapshot(telemetry.clone());
                let lines = tui_runtime_status_lines(&session_id, &telemetry);
                self.status_dialog.set_title("Runtime");
                self.status_dialog.set_footer_hint(Some(
                    "Esc close · /events [stage=<id>] for raw event log".to_string(),
                ));
                self.status_dialog.set_status_lines(lines);
                true
            }
            Err(error) => {
                self.toast.show(
                    ToastVariant::Error,
                    &format!("Failed to load runtime telemetry: {}", error),
                    3000,
                );
                false
            }
        }
    }

    fn render_usage_status_dialog(&mut self) -> bool {
        let Some(session_id) = self.current_session_id() else {
            self.toast.show(
                ToastVariant::Warning,
                "No active session available for /usage.",
                2400,
            );
            return false;
        };

        let Some(client) = self.context.get_api_client() else {
            self.toast
                .show(ToastVariant::Error, "API unavailable for /usage.", 2400);
            return false;
        };

        match client.get_session_telemetry(&session_id) {
            Ok(telemetry) => {
                self.context
                    .apply_session_telemetry_snapshot(telemetry.clone());
                let lines = tui_usage_status_lines(&session_id, &telemetry);
                self.status_dialog.set_title("Usage");
                self.status_dialog.set_footer_hint(Some(
                    "Esc close · values come from /session/{id}/telemetry".to_string(),
                ));
                self.status_dialog.set_status_lines(lines);
                true
            }
            Err(error) => {
                self.toast.show(
                    ToastVariant::Error,
                    &format!("Failed to load session usage: {}", error),
                    3000,
                );
                false
            }
        }
    }

    pub(super) fn handle_status_dialog_key(&mut self, key: KeyEvent) -> bool {
        if !self.status_dialog.is_open() {
            return false;
        }

        if !matches!(self.status_dialog_view, StatusDialogView::Events(_)) {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.status_dialog.close();
            }
            return true;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.status_dialog.close(),
            KeyCode::Left | KeyCode::PageUp => {
                let _ = self.open_events_status_dialog(Some("prev"));
            }
            KeyCode::Right | KeyCode::PageDown => {
                let _ = self.open_events_status_dialog(Some("next"));
            }
            KeyCode::Home => {
                let _ = self.open_events_status_dialog(Some("first"));
            }
            KeyCode::Char('p')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let _ = self.open_events_status_dialog(Some("prev"));
            }
            KeyCode::Char('n')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let _ = self.open_events_status_dialog(Some("next"));
            }
            KeyCode::Char('0')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let _ = self.open_events_status_dialog(Some("first"));
            }
            KeyCode::Char('c')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let _ = self.open_events_status_dialog(Some("clear"));
            }
            _ => {}
        }
        true
    }

    pub(super) fn recovery_status_blocks(&self, session_id: &str) -> Vec<StatusBlock> {
        let Some(client) = self.context.get_api_client() else {
            return vec![
                StatusBlock::title("Recovery Protocol"),
                StatusBlock::muted("- API unavailable"),
            ];
        };

        let recovery = match client.get_session_recovery(session_id) {
            Ok(recovery) => recovery,
            Err(error) => {
                return vec![
                    StatusBlock::title("Recovery Protocol"),
                    StatusBlock::error(format!("- Failed to load: {}", error)),
                ];
            }
        };

        recovery_status_blocks_from_protocol(&recovery)
    }

    // ── Agent task handlers ──────────────────────────────────────────────

    pub(super) fn handle_list_tasks(&mut self) {
        let tasks = global_task_registry().list();
        let now = Utc::now().timestamp();
        let mut blocks = vec![StatusBlock::title("Agent Tasks")];
        if tasks.is_empty() {
            blocks.push(StatusBlock::muted("No agent tasks"));
        } else {
            for task in &tasks {
                let (icon, status_str) = match &task.status {
                    AgentTaskStatus::Pending => ("◯", "pending".to_string()),
                    AgentTaskStatus::Running { step } => {
                        let steps = task
                            .max_steps
                            .map(|m| format!("{}/{}", step, m))
                            .unwrap_or(format!("{}/?", step));
                        ("◐", format!("running  {}", steps))
                    }
                    AgentTaskStatus::Completed { steps } => ("●", format!("done     {}", steps)),
                    AgentTaskStatus::Cancelled => ("✗", "cancelled".to_string()),
                    AgentTaskStatus::Failed { .. } => ("✗", "failed".to_string()),
                };
                let elapsed = now - task.started_at;
                let elapsed_str = if elapsed < 60 {
                    format!("{}s ago", elapsed)
                } else {
                    format!("{}m ago", elapsed / 60)
                };
                let line = format!(
                    "{}  {}  {:<20} {:<16} {}",
                    icon, task.id, task.agent_name, status_str, elapsed_str
                );
                let block = if task.status.is_terminal() {
                    StatusBlock::muted(line)
                } else {
                    StatusBlock::normal(line)
                };
                blocks.push(block);
            }
            let running = tasks
                .iter()
                .filter(|t| matches!(t.status, AgentTaskStatus::Running { .. }))
                .count();
            let done = tasks.iter().filter(|t| t.status.is_terminal()).count();
            blocks.push(StatusBlock::muted(format!(
                "{} running, {} finished",
                running, done
            )));
        }
        let lines = blocks
            .into_iter()
            .map(status_line_from_block)
            .collect::<Vec<_>>();
        self.status_dialog.set_status_lines(lines);
        self.status_dialog.open();
    }

    pub(super) fn handle_show_task(&mut self, id: &str) {
        let now = Utc::now().timestamp();
        match global_task_registry().get(id) {
            Some(task) => {
                let (status_label, step_info) = match &task.status {
                    AgentTaskStatus::Pending => ("pending".to_string(), String::new()),
                    AgentTaskStatus::Running { step } => {
                        let steps = task
                            .max_steps
                            .map(|m| format!(" (step {}/{})", step, m))
                            .unwrap_or(format!(" (step {}/?)", step));
                        ("running".to_string(), steps)
                    }
                    AgentTaskStatus::Completed { steps } => {
                        ("completed".to_string(), format!(" ({} steps)", steps))
                    }
                    AgentTaskStatus::Cancelled => ("cancelled".to_string(), String::new()),
                    AgentTaskStatus::Failed { error } => {
                        (format!("failed: {}", error), String::new())
                    }
                };
                let elapsed = now - task.started_at;
                let elapsed_str = if elapsed < 60 {
                    format!("{}s ago", elapsed)
                } else {
                    format!("{}m ago", elapsed / 60)
                };
                let mut blocks = vec![
                    StatusBlock::title(format!("Task {} — {}", task.id, task.agent_name)),
                    StatusBlock::normal(format!("Status: {}{}", status_label, step_info)),
                    StatusBlock::normal(format!("Started: {}", elapsed_str)),
                    StatusBlock::normal(format!("Prompt: {}", task.prompt)),
                ];
                if !task.output_tail.is_empty() {
                    blocks.push(StatusBlock::muted(""));
                    blocks.push(StatusBlock::title("Recent output"));
                    for line in &task.output_tail {
                        blocks.push(StatusBlock::muted(format!("  {}", line)));
                    }
                }
                let lines = blocks
                    .into_iter()
                    .map(status_line_from_block)
                    .collect::<Vec<_>>();
                self.status_dialog.set_status_lines(lines);
                self.status_dialog.open();
            }
            None => {
                self.toast.show(
                    ToastVariant::Error,
                    &format!("Task \"{}\" not found", id),
                    2500,
                );
            }
        }
    }

    pub(super) fn handle_kill_task(&mut self, id: &str) {
        match rocode_orchestrator::global_lifecycle().cancel_task(id) {
            Ok(()) => {
                self.toast.show(
                    ToastVariant::Success,
                    &format!("Task {} cancelled", id),
                    2000,
                );
            }
            Err(err) => {
                self.toast.show(ToastVariant::Error, &err, 2500);
            }
        }
    }
}

fn format_run_status(status: &crate::api::SessionRunStatusKind) -> &'static str {
    match status {
        crate::api::SessionRunStatusKind::Idle => "idle",
        crate::api::SessionRunStatusKind::Running => "running",
        crate::api::SessionRunStatusKind::WaitingOnTool => "waiting_on_tool",
        crate::api::SessionRunStatusKind::WaitingOnUser => "waiting_on_user",
        crate::api::SessionRunStatusKind::Cancelling => "cancelling",
    }
}

fn tui_runtime_status_lines(
    session_id: &str,
    telemetry: &crate::api::SessionTelemetrySnapshot,
) -> Vec<StatusLine> {
    let runtime = &telemetry.runtime;
    let topology = &telemetry.topology;
    let mut lines = vec![
        StatusLine::title("Runtime Telemetry"),
        StatusLine::normal(format!("Session: {}", session_id)),
        StatusLine::normal(format!(
            "Run status: {}",
            format_run_status(&runtime.run_status)
        )),
        StatusLine::normal(format!(
            "Topology: active {} · running {} · waiting {} · cancelling {} · retry {} · done {}",
            topology.active_count,
            topology.running_count,
            topology.waiting_count,
            topology.cancelling_count,
            topology.retry_count,
            topology.done_count
        )),
        StatusLine::normal(format!("Stages observed: {}", telemetry.stages.len())),
    ];

    if let Some(active_stage_id) = runtime.active_stage_id.as_deref() {
        if let Some(stage) = telemetry
            .stages
            .iter()
            .find(|stage| stage.stage_id == active_stage_id)
        {
            lines.push(StatusLine::muted(String::new()));
            lines.push(StatusLine::title(format!(
                "Active Stage ({})",
                stage.stage_name
            )));
            lines.push(StatusLine::normal(format!(
                "Status: {}",
                format_stage_status(stage.status.clone())
            )));
            if let Some(waiting_on) = stage.waiting_on.as_deref() {
                lines.push(StatusLine::warning(format!("Waiting on: {}", waiting_on)));
            }
            if let Some(last_event) = stage.last_event.as_deref() {
                lines.push(StatusLine::muted(format!("Last event: {}", last_event)));
            }
            if let Some(budget) = stage.skill_tree_budget {
                lines.push(StatusLine::normal(format!(
                    "Skill tree budget: {}{}",
                    budget,
                    if stage.skill_tree_truncated.unwrap_or(false) {
                        " (truncated)"
                    } else {
                        ""
                    }
                )));
            }
        }
    }

    if !telemetry.stages.is_empty() {
        lines.push(StatusLine::muted(String::new()));
        lines.push(StatusLine::title(format!(
            "Stage Summaries ({})",
            telemetry.stages.len()
        )));
        for stage in &telemetry.stages {
            lines.push(StatusLine::normal(format_stage_runtime_line(stage)));
            if let Some(last_event) = stage.last_event.as_deref() {
                lines.push(StatusLine::muted(format!("  last-event {}", last_event)));
            }
            if let Some(focus) = stage.focus.as_deref() {
                lines.push(StatusLine::muted(format!("  focus {}", focus)));
            }
        }
    }

    lines.push(StatusLine::muted(String::new()));
    if runtime.active_tools.is_empty() {
        lines.push(StatusLine::muted("Active tools: none"));
    } else {
        lines.push(StatusLine::title(format!(
            "Active Tools ({})",
            runtime.active_tools.len()
        )));
        for tool in &runtime.active_tools {
            lines.push(StatusLine::normal(format!(
                "- {} · {}",
                tool.tool_name, tool.tool_call_id
            )));
        }
    }

    if let Some(question) = runtime.pending_question.as_ref() {
        lines.push(StatusLine::muted(String::new()));
        lines.push(StatusLine::warning(format!(
            "Pending question: {}",
            question.request_id
        )));
    }
    if let Some(permission) = runtime.pending_permission.as_ref() {
        lines.push(StatusLine::warning(format!(
            "Pending permission: {}",
            permission.permission_id
        )));
    }

    if !runtime.child_sessions.is_empty() {
        lines.push(StatusLine::muted(String::new()));
        lines.push(StatusLine::title(format!(
            "Child Sessions ({})",
            runtime.child_sessions.len()
        )));
        for child in &runtime.child_sessions {
            lines.push(StatusLine::normal(format!(
                "- {} ← {}",
                child.child_id, child.parent_id
            )));
        }
    }

    lines
}

fn tui_usage_status_lines(
    session_id: &str,
    telemetry: &crate::api::SessionTelemetrySnapshot,
) -> Vec<StatusLine> {
    let usage = &telemetry.usage;
    let mut lines = vec![
        StatusLine::title("Session Usage"),
        StatusLine::normal(format!("Session: {}", session_id)),
        StatusLine::normal(format!(
            "Input {} · Output {} · Reasoning {}",
            usage.input_tokens, usage.output_tokens, usage.reasoning_tokens
        )),
        StatusLine::normal(format!(
            "Cache read {} · Cache write {} · Cost ${:.4}",
            usage.cache_read_tokens, usage.cache_write_tokens, usage.total_cost
        )),
    ];

    if !telemetry.stages.is_empty() {
        lines.push(StatusLine::muted(String::new()));
        lines.push(StatusLine::title(format!(
            "Stage Usage ({})",
            telemetry.stages.len()
        )));
        for stage in &telemetry.stages {
            lines.push(StatusLine::normal(format_stage_usage_summary_line(stage)));
        }
    }

    lines
}

fn tui_events_query(
    input: &rocode_command::interactive::InteractiveEventsQuery,
    offset: usize,
) -> crate::api::SessionEventsQuery {
    crate::api::SessionEventsQuery {
        stage_id: input.stage_id.clone(),
        execution_id: input.execution_id.clone(),
        event_type: input.event_type.clone(),
        since: input.since,
        limit: input.limit,
        offset: Some(offset),
    }
}

fn tui_events_filter_label(input: &rocode_command::interactive::InteractiveEventsQuery) -> String {
    let mut parts = Vec::new();
    if let Some(stage_id) = input.stage_id.as_deref() {
        parts.push(format!("stage={stage_id}"));
    }
    if let Some(execution_id) = input.execution_id.as_deref() {
        parts.push(format!("exec={execution_id}"));
    }
    if let Some(event_type) = input.event_type.as_deref() {
        parts.push(format!("type={event_type}"));
    }
    if let Some(since) = input.since {
        parts.push(format!("since={since}"));
    }
    parts.push(format!(
        "limit={}",
        rocode_command::interactive::events_browser_page_size(input)
    ));
    parts.join(" · ")
}

fn tui_events_window_label(offset: usize, count: usize) -> String {
    if count == 0 {
        return "items 0".to_string();
    }
    format!("items {}-{}", offset + 1, offset + count)
}

fn tui_event_payload_summary(payload: &serde_json::Value) -> Option<String> {
    match payload {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.trim().to_string()),
        value => serde_json::to_string(value).ok(),
    }
    .filter(|text| !text.is_empty())
    .map(|text| {
        let compact = text.replace('\n', " ");
        if compact.chars().count() > 140 {
            let truncated = compact.chars().take(137).collect::<String>();
            format!("{}...", truncated)
        } else {
            compact
        }
    })
}

fn tui_event_status_lines(
    events: &[rocode_command::stage_protocol::StageEvent],
) -> Vec<StatusLine> {
    let mut lines = Vec::new();
    for event in events {
        let ts = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(event.ts)
            .map(|value| value.with_timezone(&chrono::Local))
            .map(|value| value.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| event.ts.to_string());
        let mut headline = format!("{} · {} · {:?}", ts, event.event_type, event.scope);
        if let Some(stage_id) = event.stage_id.as_deref() {
            headline.push_str(&format!(" · stage {}", stage_id));
        }
        if let Some(execution_id) = event.execution_id.as_deref() {
            headline.push_str(&format!(" · exec {}", execution_id));
        }
        lines.push(StatusLine::normal(headline));
        if let Some(payload) = tui_event_payload_summary(&event.payload) {
            lines.push(StatusLine::muted(format!("  {}", payload)));
        }
    }
    lines
}

fn format_stage_status(status: rocode_command::stage_protocol::StageStatus) -> &'static str {
    match status {
        rocode_command::stage_protocol::StageStatus::Running => "running",
        rocode_command::stage_protocol::StageStatus::Waiting => "waiting",
        rocode_command::stage_protocol::StageStatus::Done => "done",
        rocode_command::stage_protocol::StageStatus::Cancelled => "cancelled",
        rocode_command::stage_protocol::StageStatus::Cancelling => "cancelling",
        rocode_command::stage_protocol::StageStatus::Blocked => "blocked",
        rocode_command::stage_protocol::StageStatus::Retrying => "retrying",
    }
}

fn format_stage_summary_line(stage: &rocode_command::stage_protocol::StageSummary) -> String {
    let mut suffix = Vec::new();
    if let (Some(index), Some(total)) = (stage.index, stage.total) {
        suffix.push(format!("{}/{}", index, total));
    }
    if let (Some(step), Some(step_total)) = (stage.step, stage.step_total) {
        suffix.push(format!("step {}/{}", step, step_total));
    }
    if let Some(waiting_on) = stage.waiting_on.as_deref() {
        suffix.push(format!("waiting {}", waiting_on));
    }
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" [{}]", suffix.join(" · "))
    };
    format!(
        "- {} ({}){}",
        stage.stage_name,
        format_stage_status(stage.status),
        suffix
    )
}

fn format_stage_runtime_line(stage: &rocode_command::stage_protocol::StageSummary) -> String {
    let mut parts = vec![format!(
        "- {} ({})",
        stage.stage_name,
        format_stage_status(stage.status.clone())
    )];
    if let (Some(index), Some(total)) = (stage.index, stage.total) {
        parts.push(format!("{}/{}", index, total));
    }
    if let (Some(step), Some(step_total)) = (stage.step, stage.step_total) {
        parts.push(format!("step {}/{}", step, step_total));
    }
    if let Some(waiting_on) = stage.waiting_on.as_deref() {
        parts.push(format!("waiting {}", waiting_on));
    }
    if let Some(retry_attempt) = stage.retry_attempt {
        parts.push(format!("retry {}", retry_attempt));
    }
    if stage.active_agent_count > 0 {
        parts.push(format!("agents {}", stage.active_agent_count));
    }
    if stage.active_tool_count > 0 {
        parts.push(format!("tools {}", stage.active_tool_count));
    }
    if stage.child_session_count > 0 {
        parts.push(format!("child {}", stage.child_session_count));
    }
    if let Some(budget) = stage.skill_tree_budget {
        parts.push(format!(
            "budget {}{}",
            budget,
            if stage.skill_tree_truncated.unwrap_or(false) {
                " truncated"
            } else {
                ""
            }
        ));
    }
    if let Some(tokens) = stage.estimated_context_tokens {
        parts.push(format!("ctx {}", tokens));
    }
    parts.join(" · ")
}

fn format_stage_usage_summary_line(stage: &rocode_command::stage_protocol::StageSummary) -> String {
    let mut parts = vec![format!(
        "- {} ({})",
        stage.stage_name,
        format_stage_status(stage.status.clone())
    )];
    if let Some(prompt_tokens) = stage.prompt_tokens {
        parts.push(format!("in {}", prompt_tokens));
    }
    if let Some(completion_tokens) = stage.completion_tokens {
        parts.push(format!("out {}", completion_tokens));
    }
    if let Some(reasoning_tokens) = stage.reasoning_tokens.filter(|value| *value > 0) {
        parts.push(format!("reason {}", reasoning_tokens));
    }
    if let Some(cache_read_tokens) = stage.cache_read_tokens.filter(|value| *value > 0) {
        parts.push(format!("cache-r {}", cache_read_tokens));
    }
    if let Some(cache_write_tokens) = stage.cache_write_tokens.filter(|value| *value > 0) {
        parts.push(format!("cache-w {}", cache_write_tokens));
    }
    if let Some(budget) = stage.skill_tree_budget {
        parts.push(format!(
            "budget {}{}",
            budget,
            if stage.skill_tree_truncated.unwrap_or(false) {
                " truncated"
            } else {
                ""
            }
        ));
    }
    if let Some(waiting_on) = stage.waiting_on.as_deref() {
        parts.push(format!("waiting {}", waiting_on));
    }
    if let Some(retry_attempt) = stage.retry_attempt {
        parts.push(format!("retry {}", retry_attempt));
    }
    parts.join(" · ")
}

fn active_stage_status_blocks(
    stage: &rocode_command::stage_protocol::StageSummary,
) -> Vec<StatusBlock> {
    let mut blocks = vec![StatusBlock::title(format!(
        "Active Stage Detail ({})",
        stage.stage_name
    ))];
    if let Some(waiting_on) = stage.waiting_on.as_deref() {
        blocks.push(StatusBlock::warning(format!("Waiting on: {}", waiting_on)));
    }
    if let Some(last_event) = stage.last_event.as_deref() {
        blocks.push(StatusBlock::muted(format!("Last event: {}", last_event)));
    }
    if let Some(budget) = stage.skill_tree_budget {
        blocks.push(StatusBlock::normal(format!(
            "Skill tree budget: {}{}",
            budget,
            stage
                .skill_tree_truncated
                .unwrap_or(false)
                .then_some(" (truncated)")
                .unwrap_or("")
        )));
    }
    if let Some(strategy) = stage.skill_tree_truncation_strategy.as_deref() {
        blocks.push(StatusBlock::muted(format!(
            "Truncation strategy: {}",
            strategy
        )));
    }
    if let Some(tokens) = stage.estimated_context_tokens {
        blocks.push(StatusBlock::muted(format!(
            "Estimated context tokens: {}",
            tokens
        )));
    }
    if let Some(prompt_tokens) = stage.prompt_tokens {
        blocks.push(StatusBlock::normal(format!(
            "Stage usage: in {} out {} reasoning {}",
            prompt_tokens,
            stage.completion_tokens.unwrap_or(0),
            stage.reasoning_tokens.unwrap_or(0)
        )));
    }
    blocks
}
