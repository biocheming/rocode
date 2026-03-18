use super::*;

use rocode_core::contracts::mcp::McpConnectionStatusWire;

impl App {
    pub(super) fn refresh_status_dialog(&mut self) {
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
                    McpConnectionStatus::Connected => McpConnectionStatusWire::Connected.as_str(),
                    McpConnectionStatus::Disconnected => {
                        McpConnectionStatusWire::Disconnected.as_str()
                    }
                    McpConnectionStatus::Failed => McpConnectionStatusWire::Failed.as_str(),
                    McpConnectionStatus::NeedsAuth => "needs authentication",
                    McpConnectionStatus::NeedsClientRegistration => "needs client ID",
                    McpConnectionStatus::Disabled => McpConnectionStatusWire::Disabled.as_str(),
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
            status_blocks.extend(self.recovery_status_blocks(&session_id));
        }
        let lines = status_blocks
            .into_iter()
            .map(status_line_from_block)
            .collect::<Vec<_>>();
        self.status_dialog.set_status_lines(lines);
    }

    pub(super) fn execution_status_blocks(&self, session_id: &str) -> Vec<StatusBlock> {
        let Some(client) = self.context.get_api_client() else {
            return vec![
                StatusBlock::title("Execution Topology"),
                StatusBlock::muted("- API unavailable"),
            ];
        };

        let topology = match client.get_session_executions(session_id) {
            Ok(topology) => topology,
            Err(error) => {
                return vec![
                    StatusBlock::title("Execution Topology"),
                    StatusBlock::error(format!("- Failed to load: {}", error)),
                ];
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
                    AgentTaskStatus::Pending => ("◯", task.status.kind().as_str().to_string()),
                    AgentTaskStatus::Running { step } => {
                        let steps = task
                            .max_steps
                            .map(|m| format!("{}/{}", step, m))
                            .unwrap_or(format!("{}/?", step));
                        ("◐", format!("{}  {}", task.status.kind().as_str(), steps))
                    }
                    AgentTaskStatus::Completed { steps } => {
                        ("●", format!("{}  {}", task.status.kind().as_str(), steps))
                    }
                    AgentTaskStatus::Cancelled => ("✗", task.status.kind().as_str().to_string()),
                    AgentTaskStatus::Failed { .. } => ("✗", task.status.kind().as_str().to_string()),
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
                    AgentTaskStatus::Pending => (task.status.kind().as_str().to_string(), String::new()),
                    AgentTaskStatus::Running { step } => {
                        let steps = task
                            .max_steps
                            .map(|m| format!(" (step {}/{})", step, m))
                            .unwrap_or(format!(" (step {}/?)", step));
                        (task.status.kind().as_str().to_string(), steps)
                    }
                    AgentTaskStatus::Completed { steps } => {
                        (
                            task.status.kind().as_str().to_string(),
                            format!(" ({} steps)", steps),
                        )
                    }
                    AgentTaskStatus::Cancelled => (task.status.kind().as_str().to_string(), String::new()),
                    AgentTaskStatus::Failed { error } => {
                        (
                            format!("{}: {}", task.status.kind().as_str(), error),
                            String::new(),
                        )
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
