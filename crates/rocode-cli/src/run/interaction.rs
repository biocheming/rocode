// ── CLI interactive question handler ─────────────────────────────────

async fn cli_ask_question(
    questions: Vec<rocode_tool::QuestionDef>,
    observed_topology: Arc<Mutex<CliObservedExecutionTopology>>,
    frontend_projection: Arc<Mutex<CliFrontendProjection>>,
    prompt_session_slot: Arc<std::sync::Mutex<Option<Arc<PromptSession>>>>,
    terminal_surface: Option<Arc<CliTerminalSurface>>,
    spinner_guard: SpinnerGuard,
) -> Result<Vec<Vec<String>>, rocode_tool::ToolError> {
    spinner_guard.pause();
    let style = CliStyle::detect();
    let prompt_session = prompt_session_slot
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned());
    let already_suspended = terminal_surface
        .as_ref()
        .is_some_and(|surface| surface.prompt_suspended.load(Ordering::Relaxed));
    if !already_suspended {
        if let Some(prompt_session) = prompt_session.as_ref() {
            let _ = prompt_session.suspend();
        }
    }

    {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
        let _ = stdout.flush();
    }

    if let Ok(mut topology) = observed_topology.lock() {
        topology.start_question(questions.len());
    }
    let mut all_answers = Vec::with_capacity(questions.len());

    for question in &questions {
        cli_frontend_set_phase(
            &frontend_projection,
            CliFrontendPhase::Waiting,
            Some(
                question
                    .header
                    .clone()
                    .unwrap_or_else(|| "question".to_string()),
            ),
        );
        let options: Vec<SelectOption> = question
            .options
            .iter()
            .map(|option| SelectOption {
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect();

        let question_text = question.question.clone();
        let question_header = question.header.clone();
        let question_multiple = question.multiple;
        let style_clone = style.clone();
        let result = tokio::task::spawn_blocking(move || {
            tracing::info!(
                question = %question_text,
                options_count = options.len(),
                multiple = question_multiple,
                style_color = style_clone.color,
                "CLI question: presenting selector"
            );
            if options.is_empty() {
                prompt_free_text(&question_text, question_header.as_deref(), &style_clone)
            } else if question_multiple {
                interactive_multi_select(
                    &question_text,
                    question_header.as_deref(),
                    &options,
                    &style_clone,
                )
            } else {
                interactive_select(
                    &question_text,
                    question_header.as_deref(),
                    &options,
                    &style_clone,
                )
            }
        })
        .await
        .unwrap_or_else(|error| Err(io::Error::other(format!("Selector task panicked: {}", error))));

        match result {
            Ok(SelectResult::Selected(choices)) => {
                all_answers.push(choices);
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Busy,
                    Some("assistant response".to_string()),
                );
            }
            Ok(SelectResult::Other(text)) => {
                all_answers.push(vec![text]);
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Busy,
                    Some("assistant response".to_string()),
                );
            }
            Ok(SelectResult::Cancelled) => {
                if let Ok(mut topology) = observed_topology.lock() {
                    topology.finish_question("cancelled");
                }
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Failed,
                    Some("question cancelled".to_string()),
                );
                if let Some(prompt_session) = prompt_session.as_ref() {
                    let _ = prompt_session.resume();
                }
                if let Some(surface) = terminal_surface.as_ref() {
                    surface.prompt_suspended.store(false, Ordering::Relaxed);
                }
                spinner_guard.resume();
                return Err(rocode_tool::ToolError::ExecutionError(
                    "User cancelled the question".to_string(),
                ));
            }
            Err(error) => {
                if let Ok(mut topology) = observed_topology.lock() {
                    topology.finish_question("failed");
                }
                cli_frontend_set_phase(
                    &frontend_projection,
                    CliFrontendPhase::Failed,
                    Some("question failed".to_string()),
                );
                if let Some(prompt_session) = prompt_session.as_ref() {
                    let _ = prompt_session.resume();
                }
                if let Some(surface) = terminal_surface.as_ref() {
                    surface.prompt_suspended.store(false, Ordering::Relaxed);
                }
                spinner_guard.resume();
                return Err(rocode_tool::ToolError::ExecutionError(format!(
                    "Interactive prompt error: {}",
                    error
                )));
            }
        }
    }

    if let Ok(mut topology) = observed_topology.lock() {
        topology.finish_question("answered");
    }
    cli_frontend_set_phase(
        &frontend_projection,
        CliFrontendPhase::Busy,
        Some("assistant response".to_string()),
    );
    if let Some(prompt_session) = prompt_session.as_ref() {
        let _ = prompt_session.resume();
    }
    if let Some(surface) = terminal_surface.as_ref() {
        surface.prompt_suspended.store(false, Ordering::Relaxed);
    }
    spinner_guard.resume();
    Ok(all_answers)
}

fn prompt_free_text(
    question: &str,
    header: Option<&str>,
    style: &CliStyle,
) -> io::Result<SelectResult> {
    println!();
    if let Some(header) = header {
        println!("  {} {}", style.bold_cyan(style.bullet()), style.bold(header));
    }
    println!("  {}", question);
    print!("  {} ", style.bold_cyan("›"));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_string();

    if answer.is_empty() {
        Ok(SelectResult::Cancelled)
    } else {
        Ok(SelectResult::Other(answer))
    }
}
