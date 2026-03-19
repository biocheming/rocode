fn cli_resolve_registry_ui_action(
    registry: &CommandRegistry,
    input: &str,
) -> Option<ResolvedUiCommand> {
    registry.resolve_ui_slash_input(input)
}

fn cli_normalize_model_ref(model_ref: &str) -> String {
    let trimmed = model_ref.trim();
    let (provider, model_id) = parse_model_and_provider(Some(trimmed.to_string()));
    match (provider, model_id) {
        (Some(provider), Some(model_id)) if !provider.is_empty() && !model_id.is_empty() => {
            format!("{provider}/{model_id}")
        }
        _ => trimmed.to_string(),
    }
}

async fn cli_prompt_action_text(
    runtime: &CliExecutionRuntime,
    header: Option<&str>,
    question: &str,
) -> anyhow::Result<Option<String>> {
    let prompt_session = runtime
        .prompt_session_slot
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned());
    let already_suspended = runtime
        .terminal_surface
        .as_ref()
        .map_or(false, |s| s.prompt_suspended.load(Ordering::Relaxed));
    if !already_suspended {
        if let Some(prompt_session) = prompt_session.as_ref() {
            let _ = prompt_session.suspend();
        }
    }

    {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(
            stdout,
            crossterm::cursor::Show,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::FromCursorDown)
        );
        let _ = stdout.flush();
    }

    let header = header.map(str::to_string);
    let question = question.to_string();
    let style = CliStyle::detect();
    let result =
        tokio::task::spawn_blocking(move || prompt_free_text(&question, header.as_deref(), &style))
            .await
            .map_err(|error| anyhow::anyhow!("prompt task failed: {}", error))?;

    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.resume();
    }
    if let Some(surface) = runtime.terminal_surface.as_ref() {
        surface.prompt_suspended.store(false, Ordering::Relaxed);
    }

    match result {
        Ok(SelectResult::Other(text)) => Ok(Some(text)),
        Ok(SelectResult::Cancelled) | Ok(SelectResult::Selected(_)) => Ok(None),
        Err(error) => Err(anyhow::anyhow!("prompt failed: {}", error)),
    }
}

async fn cli_execute_ui_action(
    action_id: UiActionId,
    argument: Option<&str>,
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    provider_registry: &ProviderRegistry,
    agent_registry: &AgentRegistry,
    current_dir: &Path,
    repl_style: &CliStyle,
) -> anyhow::Result<CliUiActionOutcome> {
    match action_id {
        UiActionId::AbortExecution => {
            let handle = { runtime.active_abort.lock().await.clone() };
            let Some(handle) = handle else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active run to abort. Use /abort while a response is running.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            if cli_trigger_abort(handle).await {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("Cancellation requested.")),
                    repl_style,
                );
            } else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::error("Failed to request cancellation.")),
                    repl_style,
                );
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::Exit => Ok(CliUiActionOutcome::Break),
        UiActionId::ShowHelp => {
            let style = CliStyle::detect();
            let rendered = render_help(&style);
            if let Some(surface) = runtime.terminal_surface.as_ref() {
                let _ = surface.print_text(&rendered);
            } else {
                print!("{}", rendered);
                let _ = io::stdout().flush();
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenRecoveryList => {
            cli_print_recovery_actions(runtime);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::NewSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_new_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::RenameSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to rename.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            let Some(next_title) = cli_prompt_action_text(
                runtime,
                Some("rename session"),
                "Enter a new title for the current session:",
            )
            .await?
            else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("Session rename cancelled.")),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client
                .update_session_title(&session_id, next_title.trim())
                .await
            {
                Ok(updated) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Session renamed: {}",
                            updated.title
                        ))),
                        repl_style,
                    );
                    cli_refresh_server_info(
                        api_client,
                        &runtime.frontend_projection,
                        Some(&session_id),
                    )
                    .await;
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to rename session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ShareSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning("No active server session to share.")),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.share_session(&session_id).await {
                Ok(shared) => {
                    let label = if shared.url.trim().is_empty() {
                        "Session shared.".to_string()
                    } else {
                        format!("Share link: {}", shared.url)
                    };
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(label)),
                        repl_style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to share session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::UnshareSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let Some(session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to unshare.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.unshare_session(&session_id).await {
                Ok(_) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title("Session unshared.")),
                        repl_style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to unshare session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ForkSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_fork_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::CompactSession => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_execute_compact_session_action(runtime, api_client, repl_style).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ShowStatus => {
            if argument.is_some() {
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();

            cli_refresh_server_info(
                api_client,
                &runtime.frontend_projection,
                runtime.server_session_id.as_deref(),
            )
            .await;

            let (phase, active_label, queue_len, token_stats, mcp_servers, lsp_servers) = runtime
                .frontend_projection
                .lock()
                .map(|projection| {
                    (
                        match projection.phase {
                            CliFrontendPhase::Idle => "idle",
                            CliFrontendPhase::Busy => "busy",
                            CliFrontendPhase::Waiting => "waiting",
                            CliFrontendPhase::Cancelling => "cancelling",
                            CliFrontendPhase::Failed => "failed",
                        }
                        .to_string(),
                        projection.active_label.clone(),
                        projection.queue_len,
                        projection.token_stats.clone(),
                        projection.mcp_servers.clone(),
                        projection.lsp_servers.clone(),
                    )
                })
                .unwrap_or_else(|_| {
                    (
                        "unknown".to_string(),
                        None,
                        0,
                        CliSessionTokenStats::default(),
                        Vec::new(),
                        Vec::new(),
                    )
                });
            let mut lines = vec![
                format!("Agent: {}", runtime.resolved_agent_name),
                format!("Model: {}", runtime.resolved_model_label),
                format!("Directory: {}", current_dir.display()),
                format!("Runtime: {}", phase),
            ];
            if let Some(ref profile) = runtime.resolved_scheduler_profile_name {
                lines.push(format!("Scheduler: {}", profile));
            }
            if let Some(active_label) = active_label.filter(|value| !value.trim().is_empty()) {
                lines.push(format!("Active: {}", active_label));
            }
            lines.push(format!("Queue: {}", queue_len));

            if token_stats.total_tokens > 0 {
                lines.push(String::new());
                lines.push(format!(
                    "Tokens: {} total",
                    format_token_count(token_stats.total_tokens)
                ));
                lines.push(format!(
                    "  Input:     {}",
                    format_token_count(token_stats.input_tokens)
                ));
                lines.push(format!(
                    "  Output:    {}",
                    format_token_count(token_stats.output_tokens)
                ));
                if token_stats.reasoning_tokens > 0 {
                    lines.push(format!(
                        "  Reasoning: {}",
                        format_token_count(token_stats.reasoning_tokens)
                    ));
                }
                if token_stats.cache_read_tokens > 0 {
                    lines.push(format!(
                        "  Cache R:   {}",
                        format_token_count(token_stats.cache_read_tokens)
                    ));
                }
                if token_stats.cache_write_tokens > 0 {
                    lines.push(format!(
                        "  Cache W:   {}",
                        format_token_count(token_stats.cache_write_tokens)
                    ));
                }
                lines.push(format!("Cost: ${:.4}", token_stats.total_cost));
            }

            if !mcp_servers.is_empty() {
                lines.push(String::new());
                lines.push("MCP Servers:".to_string());
                for server in &mcp_servers {
                    let detail = if server.tools > 0 {
                        format!(" ({} tools)", server.tools)
                    } else {
                        String::new()
                    };
                    lines.push(format!("  {} [{}]{}", server.name, server.status, detail));
                    if let Some(ref err) = server.error {
                        lines.push(format!("    ↳ {}", err));
                    }
                }
            }

            if !lsp_servers.is_empty() {
                lines.push(String::new());
                lines.push("LSP Servers:".to_string());
                for server in &lsp_servers {
                    lines.push(format!("  {}", server));
                }
            }

            if let Some(ref sid) = runtime.server_session_id {
                lines.push(String::new());
                lines.push(format!("Server: {}", api_client.base_url()));
                lines.push(format!("Session: {}", sid));
            }

            let _ =
                print_cli_list_on_surface(Some(runtime), "Session Status", None, &lines, &style);
            cli_print_execution_topology(&runtime.observed_topology, Some(runtime), &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenModelList => {
            if let Some(model_ref) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let normalized_model_ref = cli_normalize_model_ref(model_ref);
                let mut exists = false;
                for provider in provider_registry.list() {
                    for model in provider.models() {
                        if format!("{}/{}", provider.id(), model.id) == normalized_model_ref {
                            exists = true;
                            break;
                        }
                    }
                    if exists {
                        break;
                    }
                }
                if exists {
                    runtime.resolved_model_label = normalized_model_ref.clone();
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Model set to {}",
                            normalized_model_ref
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown model: {}",
                            model_ref
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            let mut lines = Vec::new();
            for p in provider_registry.list() {
                for m in p.models() {
                    lines.push(format!("{}/{}", p.id(), m.id));
                }
            }
            let _ =
                print_cli_list_on_surface(Some(runtime), "Available Models", None, &lines, &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenModeList => {
            if let Some(mode_ref) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                match api_client.list_execution_modes().await {
                    Ok(modes) => {
                        let normalized = mode_ref.to_ascii_lowercase();
                        let found = modes.into_iter().find(|mode| {
                            let key = format!("{}:{}", mode.kind, mode.id).to_ascii_lowercase();
                            key == normalized
                                || mode.id.to_ascii_lowercase() == normalized
                                || mode.name.to_ascii_lowercase() == normalized
                                || format!("{}:{}", mode.kind, mode.name).to_ascii_lowercase()
                                    == normalized
                        });
                        if let Some(mode) = found {
                            runtime.resolved_scheduler_profile_name = match mode.kind.as_str() {
                                "preset" | "profile" => Some(mode.id.clone()),
                                _ => None,
                            };
                            runtime.resolved_agent_name = if mode.kind == "agent" {
                                mode.id.clone()
                            } else {
                                runtime.resolved_agent_name.clone()
                            };
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Mode set to {}:{}",
                                    mode.kind, mode.id
                                ))),
                                repl_style,
                            );
                        } else {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::warning(format!(
                                    "Unknown mode: {}",
                                    mode_ref
                                ))),
                                repl_style,
                            );
                        }
                    }
                    Err(error) => {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::error(format!(
                                "Failed to load modes: {}",
                                error
                            ))),
                            repl_style,
                        );
                    }
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            match api_client.list_execution_modes().await {
                Ok(modes) => {
                    let lines = modes
                        .into_iter()
                        .filter(|mode| !mode.hidden.unwrap_or(false))
                        .map(|mode| {
                            let detail = mode
                                .description
                                .filter(|value| !value.trim().is_empty())
                                .unwrap_or_else(|| mode.kind.clone());
                            format!("{} [{}] — {}", mode.id, mode.kind, detail)
                        })
                        .collect::<Vec<_>>();
                    let _ = print_cli_list_on_surface(
                        Some(runtime),
                        "Available Modes",
                        None,
                        &lines,
                        &style,
                    );
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to load modes: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ConnectProvider => {
            let style = CliStyle::detect();
            let mut lines = Vec::new();
            for p in provider_registry.list() {
                let model_count = p.models().len();
                lines.push(format!(
                    "{} ({} model{})",
                    p.id(),
                    model_count,
                    if model_count != 1 { "s" } else { "" }
                ));
            }
            let _ = print_cli_list_on_surface(
                Some(runtime),
                "Configured Providers",
                None,
                &lines,
                &style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenThemeList => {
            if argument.is_some() {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "Theme switching is not yet supported in CLI mode.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            }
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning(
                    "Theme switching is not yet supported in CLI mode.",
                )),
                repl_style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenAgentList => {
            if let Some(agent_name) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let agents = agent_registry.list();
                let found = agents.iter().find(|info| info.name == agent_name);
                if let Some(info) = found {
                    runtime.resolved_agent_name = info.name.clone();
                    runtime.resolved_scheduler_profile_name = None;
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Agent set to {}",
                            info.name
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown agent: {}",
                            agent_name
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            let style = CliStyle::detect();
            let mut lines = Vec::new();
            for info in agent_registry.list() {
                let active = if info.name == runtime.resolved_agent_name {
                    " ← active".to_string()
                } else {
                    String::new()
                };
                let model_info = info
                    .model
                    .as_ref()
                    .map(|m| format!(" ({}/{})", m.provider_id, m.model_id))
                    .unwrap_or_default();
                lines.push(format!("{}{}{}", info.name, model_info, active));
            }
            let _ =
                print_cli_list_on_surface(Some(runtime), "Available Agents", None, &lines, &style);
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenPresetList => {
            if let Some(preset_name) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                let presets = cli_available_presets(&load_config(current_dir)?);
                if presets.iter().any(|preset| preset == preset_name) {
                    runtime.resolved_scheduler_profile_name = Some(preset_name.to_string());
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::title(format!(
                            "Preset set to {}",
                            preset_name
                        ))),
                        repl_style,
                    );
                } else {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(format!(
                            "Unknown preset: {}",
                            preset_name
                        ))),
                        repl_style,
                    );
                }
                return Ok(CliUiActionOutcome::Continue);
            }
            cli_list_presets(
                &load_config(current_dir)?,
                runtime.resolved_scheduler_profile_name.as_deref(),
                Some(runtime),
            );
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::OpenSessionList => {
            if let Some(target) = argument.map(str::trim).filter(|value| !value.is_empty()) {
                match target {
                    "list" => {}
                    "new" => {
                        cli_execute_new_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    "fork" => {
                        cli_execute_fork_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    "compact" => {
                        cli_execute_compact_session_action(runtime, api_client, repl_style).await;
                        return Ok(CliUiActionOutcome::Continue);
                    }
                    _ => match api_client.list_sessions(Some(target), Some(20)).await {
                        Ok(sessions) => {
                            if let Some(session) = sessions.into_iter().find(|session| {
                                session.id == target
                                    || session.id.starts_with(target)
                                    || session
                                        .title
                                        .to_ascii_lowercase()
                                        .contains(&target.to_ascii_lowercase())
                            }) {
                                cli_set_root_server_session(runtime, session.id.clone());
                                let _ = print_block(
                                    Some(runtime),
                                    OutputBlock::Status(StatusBlock::title(format!(
                                        "Session switched: {}",
                                        session.id
                                    ))),
                                    repl_style,
                                );
                                cli_refresh_server_info(
                                    api_client,
                                    &runtime.frontend_projection,
                                    Some(&session.id),
                                )
                                .await;
                                return Ok(CliUiActionOutcome::Continue);
                            }
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::warning(format!(
                                    "Session not found: {}",
                                    target
                                ))),
                                repl_style,
                            );
                            return Ok(CliUiActionOutcome::Continue);
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to load sessions: {}",
                                    error
                                ))),
                                repl_style,
                            );
                            return Ok(CliUiActionOutcome::Continue);
                        }
                    },
                }
            }
            cli_list_sessions(Some(runtime)).await;
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::NavigateParentSession => {
            let Some(current_session_id) = runtime.server_session_id.clone() else {
                let _ = print_block(
                    Some(runtime),
                    OutputBlock::Status(StatusBlock::warning(
                        "No active server session to navigate from.",
                    )),
                    repl_style,
                );
                return Ok(CliUiActionOutcome::Continue);
            };

            match api_client.get_session(&current_session_id).await {
                Ok(session) => {
                    let Some(parent_id) = session.parent_id else {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::warning(
                                "Current session has no parent.",
                            )),
                            repl_style,
                        );
                        return Ok(CliUiActionOutcome::Continue);
                    };

                    match api_client.get_session(&parent_id).await {
                        Ok(parent) => {
                            cli_set_root_server_session(runtime, parent.id.clone());
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::title(format!(
                                    "Switched to parent session: {}",
                                    &parent.id[..parent.id.len().min(8)]
                                ))),
                                repl_style,
                            );
                            cli_refresh_server_info(
                                api_client,
                                &runtime.frontend_projection,
                                Some(&parent.id),
                            )
                            .await;
                        }
                        Err(error) => {
                            let _ = print_block(
                                Some(runtime),
                                OutputBlock::Status(StatusBlock::error(format!(
                                    "Failed to load parent session: {}",
                                    error
                                ))),
                                repl_style,
                            );
                        }
                    }
                }
                Err(error) => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::error(format!(
                            "Failed to load current session: {}",
                            error
                        ))),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ListTasks => {
            cli_list_tasks(Some(runtime));
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::CopySession => {
            match cli_copy_target_transcript(runtime).filter(|text| !text.trim().is_empty()) {
                Some(text) => match Clipboard::write_text(&text) {
                    Ok(()) => {
                        let label = if cli_focused_session_id(runtime).is_some() {
                            "Focused session transcript copied to clipboard."
                        } else {
                            "Session transcript copied to clipboard."
                        };
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::title(label)),
                            repl_style,
                        );
                    }
                    Err(error) => {
                        let _ = print_block(
                            Some(runtime),
                            OutputBlock::Status(StatusBlock::error(format!(
                                "Failed to copy transcript to clipboard: {}",
                                error
                            ))),
                            repl_style,
                        );
                    }
                },
                None => {
                    let _ = print_block(
                        Some(runtime),
                        OutputBlock::Status(StatusBlock::warning(
                            "No transcript available for the current session view.",
                        )),
                        repl_style,
                    );
                }
            }
            Ok(CliUiActionOutcome::Continue)
        }
        UiActionId::ToggleSidebar => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::warning(
                    "CLI mode no longer keeps a persistent sidebar; use terminal scrollback and /status.",
                )),
                repl_style,
            );
            Ok(CliUiActionOutcome::Continue)
        }
        _ => Ok(CliUiActionOutcome::Continue),
    }
}
