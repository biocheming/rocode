fn cli_is_terminal_stage_status(status: Option<&str>) -> bool {
    matches!(status, Some("done" | "blocked" | "cancelled"))
}

fn cli_set_root_server_session(runtime: &mut CliExecutionRuntime, session_id: String) {
    runtime.server_session_id = Some(session_id.clone());
    if let Ok(mut related) = runtime.related_session_ids.lock() {
        related.clear();
        related.insert(session_id);
    }
    if let Ok(mut root) = runtime.root_session_transcript.lock() {
        root.clear();
    }
    if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
        transcripts.clear();
    }
    if let Ok(mut accumulators) = runtime.stream_accumulators.lock() {
        accumulators.clear();
    }
    if let Ok(mut render_states) = runtime.render_states.lock() {
        render_states.clear();
    }
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = None;
    }
    cli_set_view_label(runtime, None);
}

fn cli_render_session_block(
    runtime: &CliExecutionRuntime,
    session_id: &str,
    block: &OutputBlock,
    style: &CliStyle,
) -> String {
    let key = cli_canonical_session_id(runtime, session_id);
    let show_thinking = runtime.show_thinking.load(Ordering::SeqCst);
    let accumulators = match runtime.stream_accumulators.lock() {
        Ok(accumulators) => accumulators,
        Err(_) => return render_cli_block_rich(block, style),
    };
    let Some(accumulator) = accumulators.get(&key) else {
        return render_cli_block_rich(block, style);
    };
    let mut render_states = match runtime.render_states.lock() {
        Ok(states) => states,
        Err(_) => return render_cli_block_rich(block, style),
    };
    let state = render_states.entry(key).or_default();
    render_terminal_stream_block_semantic(state, accumulator, block, style, show_thinking)
}

fn cli_canonical_session_id(runtime: &CliExecutionRuntime, session_id: &str) -> String {
    if !session_id.is_empty() {
        return session_id.to_string();
    }

    runtime
        .server_session_id
        .clone()
        .unwrap_or_else(|| "__root__".to_string())
}

fn cli_observe_terminal_stream_block(
    runtime: &CliExecutionRuntime,
    session_id: &str,
    block_id: Option<&str>,
    block: &OutputBlock,
) {
    let key = cli_canonical_session_id(runtime, session_id);
    if let Ok(mut accumulators) = runtime.stream_accumulators.lock() {
        accumulators
            .entry(key)
            .or_insert_with(TerminalStreamAccumulator::new)
            .apply_output_block(block_id, block);
    }
}

fn cli_tracks_related_session(runtime: &CliExecutionRuntime, session_id: &str) -> bool {
    if session_id.is_empty() {
        return true;
    }
    runtime
        .related_session_ids
        .lock()
        .map(|related| related.contains(session_id))
        .unwrap_or(false)
}

fn cli_track_child_session(runtime: &CliExecutionRuntime, parent_id: &str, child_id: &str) -> bool {
    if parent_id.is_empty() || child_id.is_empty() {
        return false;
    }
    let mut inserted = false;
    if let Ok(mut related) = runtime.related_session_ids.lock() {
        if related.contains(parent_id) {
            inserted = related.insert(child_id.to_string());
        }
    }
    if inserted {
        if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
            transcripts.entry(child_id.to_string()).or_default();
        }
    }
    inserted
}

fn cli_untrack_child_session(
    runtime: &CliExecutionRuntime,
    parent_id: &str,
    child_id: &str,
) -> bool {
    if parent_id.is_empty() || child_id.is_empty() {
        return false;
    }
    runtime
        .related_session_ids
        .lock()
        .map(|mut related| related.contains(parent_id) && related.remove(child_id))
        .unwrap_or(false)
}

fn cli_cache_child_session_rendered(
    runtime: &CliExecutionRuntime,
    session_id: &str,
    rendered: &str,
) {
    if let Ok(mut transcripts) = runtime.child_session_transcripts.lock() {
        transcripts
            .entry(session_id.to_string())
            .or_default()
            .append_rendered(rendered);
    }
}

fn cli_cache_root_session_block(
    runtime: &CliExecutionRuntime,
    block: &OutputBlock,
    style: &CliStyle,
) {
    let rendered = cli_render_session_block(runtime, "", block, style);
    cli_cache_root_session_rendered(runtime, &rendered);
}

fn cli_cache_root_session_rendered(runtime: &CliExecutionRuntime, rendered: &str) {
    if let Ok(mut transcript) = runtime.root_session_transcript.lock() {
        transcript.append_rendered(rendered);
    }
}

fn cli_capture_visible_root_transcript(runtime: &CliExecutionRuntime) {
    let snapshot = runtime
        .frontend_projection
        .lock()
        .ok()
        .map(|projection| projection.transcript.clone());
    if let Some(snapshot) = snapshot {
        if let Ok(mut root) = runtime.root_session_transcript.lock() {
            *root = snapshot;
        }
    }
}

fn cli_focused_session_id(runtime: &CliExecutionRuntime) -> Option<String> {
    runtime
        .focused_session_id
        .lock()
        .ok()
        .and_then(|focused| focused.clone())
}

fn cli_is_root_focused(runtime: &CliExecutionRuntime) -> bool {
    cli_focused_session_id(runtime).is_none()
}

fn cli_replace_visible_transcript(
    runtime: &CliExecutionRuntime,
    transcript: CliRetainedTranscript,
) -> io::Result<()> {
    if let Some(surface) = runtime.terminal_surface.as_ref() {
        surface.replace_transcript(transcript)
    } else {
        if let Ok(mut projection) = runtime.frontend_projection.lock() {
            projection.transcript = transcript;
            projection.scroll_offset = 0;
        }
        Ok(())
    }
}

fn cli_short_session_id(session_id: &str) -> &str {
    &session_id[..session_id.len().min(8)]
}

fn cli_set_view_label(runtime: &CliExecutionRuntime, label: Option<String>) {
    if let Ok(mut projection) = runtime.frontend_projection.lock() {
        projection.view_label = label;
    }
    cli_refresh_prompt(runtime);
}

fn cli_ordered_child_session_ids(runtime: &CliExecutionRuntime) -> Vec<String> {
    let root_session_id = runtime.server_session_id.as_deref();
    let attached_ids = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();

    let mut child_ids = BTreeSet::new();
    for session_id in &attached_ids {
        if root_session_id != Some(session_id.as_str()) {
            child_ids.insert(session_id.clone());
        }
    }
    for session_id in transcripts.keys() {
        child_ids.insert(session_id.clone());
    }

    child_ids.into_iter().collect()
}

fn cli_list_child_sessions(runtime: &CliExecutionRuntime) {
    let style = CliStyle::detect();
    let attached_ids = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();
    let focused = cli_focused_session_id(runtime);

    let mut lines = Vec::new();
    let child_ids = cli_ordered_child_session_ids(runtime);
    if child_ids.is_empty() {
        lines.push("No child sessions have been observed for this run yet.".to_string());
        lines.push("When scheduler agents fork, they will appear here.".to_string());
    } else {
        for session_id in child_ids {
            let transcript = transcripts.get(&session_id);
            let attached = attached_ids.contains(&session_id);
            let focus_marker = if focused.as_deref() == Some(session_id.as_str()) {
                "● focused"
            } else {
                "○ cached"
            };
            let status = if attached { "attached" } else { "detached" };
            let line_count = transcript.map(|item| item.line_count()).unwrap_or(0);
            lines.push(format!(
                "{}  {}  [{} · {} lines]",
                focus_marker, session_id, status, line_count
            ));
            if let Some(summary) = transcript
                .and_then(|item| item.last_line())
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                lines.push(format!("    {}", truncate_text(summary, 88)));
            }
        }
    }

    let footer = match focused {
        Some(child_id) => format!(
            "/child next · /child prev · /child focus <id> · /child back · now viewing {}",
            child_id
        ),
        None => "/child next · /child prev · /child focus <id> · /child back".to_string(),
    };
    let _ = print_cli_list_on_surface(
        Some(runtime),
        "Child Sessions",
        Some(&footer),
        &lines,
        &style,
    );
}

fn cli_focus_child_session(runtime: &CliExecutionRuntime, requested_id: &str) -> io::Result<bool> {
    let requested_id = requested_id.trim();
    if requested_id.is_empty() {
        return Ok(false);
    }

    let transcripts = runtime
        .child_session_transcripts
        .lock()
        .map(|map| map.clone())
        .unwrap_or_default();
    let related = runtime
        .related_session_ids
        .lock()
        .map(|ids| ids.clone())
        .unwrap_or_default();
    let root_session_id = runtime.server_session_id.as_deref();

    let mut candidates = BTreeSet::new();
    for session_id in related {
        if root_session_id != Some(session_id.as_str()) {
            candidates.insert(session_id);
        }
    }
    for session_id in transcripts.keys() {
        candidates.insert(session_id.clone());
    }

    let target = if candidates.contains(requested_id) {
        Some(requested_id.to_string())
    } else {
        let mut prefix_matches = candidates
            .into_iter()
            .filter(|candidate| candidate.starts_with(requested_id))
            .collect::<Vec<_>>();
        if prefix_matches.len() == 1 {
            prefix_matches.pop()
        } else {
            None
        }
    };

    let Some(target_id) = target else {
        return Ok(false);
    };

    let Some(transcript) = transcripts.get(&target_id).cloned() else {
        return Ok(false);
    };

    if cli_is_root_focused(runtime) {
        cli_capture_visible_root_transcript(runtime);
    }
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = Some(target_id.clone());
    }
    cli_set_view_label(
        runtime,
        Some(format!("view child {}", cli_short_session_id(&target_id))),
    );
    cli_replace_visible_transcript(runtime, transcript)?;
    Ok(true)
}

fn cli_cycle_child_session(
    runtime: &CliExecutionRuntime,
    forward: bool,
) -> io::Result<Option<(String, usize, usize)>> {
    let child_ids = cli_ordered_child_session_ids(runtime);
    if child_ids.is_empty() {
        return Ok(None);
    }

    let focused = cli_focused_session_id(runtime);
    let next_index = match focused
        .as_deref()
        .and_then(|current| child_ids.iter().position(|id| id == current))
    {
        Some(index) if forward => (index + 1) % child_ids.len(),
        Some(index) => (index + child_ids.len() - 1) % child_ids.len(),
        None if forward => 0,
        None => child_ids.len() - 1,
    };
    let target_id = child_ids[next_index].clone();
    if !cli_focus_child_session(runtime, &target_id)? {
        return Ok(None);
    }
    Ok(Some((target_id, next_index + 1, child_ids.len())))
}

fn cli_focus_root_session(runtime: &CliExecutionRuntime) -> io::Result<bool> {
    if cli_is_root_focused(runtime) {
        return Ok(false);
    }
    let transcript = runtime
        .root_session_transcript
        .lock()
        .map(|item| item.clone())
        .unwrap_or_default();
    if let Ok(mut focused) = runtime.focused_session_id.lock() {
        *focused = None;
    }
    cli_set_view_label(runtime, None);
    cli_replace_visible_transcript(runtime, transcript)?;
    Ok(true)
}

fn cli_session_update_requires_refresh(source: Option<&str>) -> bool {
    matches!(
        source,
        Some(
            "prompt.final"
                | "stream.final"
                | "prompt.completed"
                | "session.title.set"
                | "prompt.done"
        )
    )
}

#[cfg(test)]
fn cli_active_stage_context_lines(
    stage: Option<&SchedulerStageBlock>,
    style: &CliStyle,
) -> Vec<String> {
    let Some(stage) = stage else {
        return Vec::new();
    };

    let max_width = usize::from(style.width).saturating_sub(8).clamp(24, 96);
    let header = if let (Some(index), Some(total)) = (stage.stage_index, stage.stage_total) {
        format!("Stage: {} [{}/{}]", stage.title, index, total)
    } else {
        format!("Stage: {}", stage.title)
    };

    let mut summary = Vec::new();
    if let Some(step) = stage.step {
        summary.push(format!("step {step}"));
    }
    if let Some(status) = stage.status.as_deref().filter(|value| !value.is_empty()) {
        summary.push(status.to_string());
    }
    if let Some(waiting_on) = stage
        .waiting_on
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        summary.push(format!("waiting on {waiting_on}"));
    }
    summary.push(format!(
        "tokens {}/{}",
        stage
            .prompt_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".to_string()),
        stage
            .completion_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".to_string())
    ));

    let mut lines = vec![
        truncate_display(&header, max_width),
        truncate_display(&format!("Status: {}", summary.join(" · ")), max_width),
    ];
    if let Some(focus) = stage.focus.as_deref().filter(|value| !value.is_empty()) {
        lines.push(truncate_display(&format!("Focus: {focus}"), max_width));
    }
    if let Some(last_event) = stage
        .last_event
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(truncate_display(&format!("Last: {last_event}"), max_width));
    }
    if let Some(ref child_id) = stage.child_session_id {
        lines.push(truncate_display(&format!("Child: {child_id}"), max_width));
    }
    lines
}

fn cli_attach_interactive_handles(
    runtime: &mut CliExecutionRuntime,
    handles: CliInteractiveHandles,
) {
    runtime.terminal_surface = Some(handles.terminal_surface);
    runtime.prompt_chrome = Some(handles.prompt_chrome.clone());
    runtime.prompt_session = Some(handles.prompt_session.clone());
    if let Ok(mut slot) = runtime.prompt_session_slot.lock() {
        *slot = Some(handles.prompt_session.clone());
    }
    runtime.queued_inputs = handles.queued_inputs;
    runtime.busy_flag = handles.busy_flag;
    runtime.exit_requested = handles.exit_requested;
    runtime.active_abort = handles.active_abort;
    handles.prompt_chrome.update_from_runtime(runtime);
    cli_refresh_prompt(runtime);
}

async fn cli_trigger_abort(handle: CliActiveAbortHandle) -> bool {
    match handle {
        CliActiveAbortHandle::Server {
            api_client,
            session_id,
        } => match api_client.abort_session(&session_id).await {
            Ok(result) => result
                .get("aborted")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            Err(e) => {
                tracing::error!("Failed to abort server session: {}", e);
                false
            }
        },
    }
}

async fn cli_execute_new_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    match api_client
        .create_session(None, runtime.resolved_scheduler_profile_name.clone())
        .await
    {
        Ok(new_session) => {
            let new_sid = new_session.id.clone();
            cli_set_root_server_session(runtime, new_sid.clone());

            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                proj.token_stats = CliSessionTokenStats::default();
            }

            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title(format!(
                    "New session created: {}",
                    &new_sid[..new_sid.len().min(8)]
                ))),
                repl_style,
            );

            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&new_sid)).await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to create new session: {}",
                    error
                ))),
                repl_style,
            );
        }
    }
}

async fn cli_execute_fork_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    let Some(session_id) = runtime.server_session_id.clone() else {
        let _ = print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::warning("No active server session to fork.")),
            repl_style,
        );
        return;
    };

    match api_client.fork_session(&session_id, None).await {
        Ok(forked) => {
            cli_set_root_server_session(runtime, forked.id.clone());
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title(format!("Forked session: {}", forked.id))),
                repl_style,
            );
            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&forked.id))
                .await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to fork session: {}",
                    error
                ))),
                repl_style,
            );
        }
    }
}

async fn cli_execute_compact_session_action(
    runtime: &mut CliExecutionRuntime,
    api_client: &CliApiClient,
    repl_style: &CliStyle,
) {
    let Some(session_id) = runtime.server_session_id.clone() else {
        let _ = print_block(
            Some(runtime),
            OutputBlock::Status(StatusBlock::warning("No server session to compact.")),
            repl_style,
        );
        return;
    };

    match api_client.compact_session(&session_id).await {
        Ok(_) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::title("Session compacted successfully.")),
                repl_style,
            );
            if let Ok(mut proj) = runtime.frontend_projection.lock() {
                proj.token_stats = CliSessionTokenStats::default();
            }
            cli_refresh_server_info(api_client, &runtime.frontend_projection, Some(&session_id))
                .await;
        }
        Err(error) => {
            let _ = print_block(
                Some(runtime),
                OutputBlock::Status(StatusBlock::error(format!(
                    "Failed to compact session: {}",
                    error
                ))),
                repl_style,
            );
        }
    }
}

fn cli_frontend_set_phase(
    frontend_projection: &Arc<Mutex<CliFrontendProjection>>,
    phase: CliFrontendPhase,
    active_label: Option<String>,
) {
    if let Ok(mut projection) = frontend_projection.lock() {
        projection.phase = phase;
        if active_label.is_some() {
            projection.active_label = active_label;
        }
    }
}

fn cli_frontend_clear(runtime: &CliExecutionRuntime) {
    if let Ok(mut projection) = runtime.frontend_projection.lock() {
        projection.phase = CliFrontendPhase::Idle;
        projection.active_label = None;
        projection.active_stage = None;
    }
}

fn cli_frontend_observe_block(
    frontend_projection: &Arc<Mutex<CliFrontendProjection>>,
    block: &OutputBlock,
) {
    let Ok(mut projection) = frontend_projection.lock() else {
        return;
    };
    match block {
        OutputBlock::SchedulerStage(stage) => {
            projection.phase = match stage.status.as_deref() {
                Some("waiting") | Some("blocked") => CliFrontendPhase::Waiting,
                Some("cancelling") => CliFrontendPhase::Cancelling,
                Some("cancelled") | Some("done") => projection.phase,
                _ => CliFrontendPhase::Busy,
            };
            projection.active_label = Some(cli_stage_activity_label(stage));
        }
        OutputBlock::Tool(tool) => {
            projection.phase = CliFrontendPhase::Busy;
            projection.active_label = Some(format!("tool {}", tool.name));
        }
        OutputBlock::SessionEvent(event) if event.event == "question" => {
            projection.phase = CliFrontendPhase::Waiting;
            projection.active_label = Some("question".to_string());
        }
        OutputBlock::Message(message)
            if message.role == OutputMessageRole::Assistant
                && matches!(message.phase, MessagePhase::Start | MessagePhase::Delta) =>
        {
            projection.phase = CliFrontendPhase::Busy;
            projection.active_label = Some("assistant response".to_string());
        }
        _ => {}
    }
}

fn cli_stage_activity_label(stage: &SchedulerStageBlock) -> String {
    let mut parts = Vec::new();
    if let (Some(index), Some(total)) = (stage.stage_index, stage.stage_total) {
        parts.push(format!("stage {index}/{total}"));
    } else {
        parts.push("stage".to_string());
    }
    parts.push(stage.stage.clone());
    if let Some(step) = stage.step {
        parts.push(format!("step {step}"));
    }
    parts.join(" · ")
}

fn cli_scheduler_stage_snapshot_key(stage: &SchedulerStageBlock) -> String {
    let decision_title = stage
        .decision
        .as_ref()
        .map(|decision| decision.title.clone())
        .unwrap_or_default();
    format!(
        "{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}",
        stage.stage_index.unwrap_or_default(),
        stage.stage,
        stage.status,
        stage.step,
        stage.waiting_on,
        stage.last_event,
        stage.prompt_tokens,
        stage.completion_tokens,
        decision_title,
        stage.activity.as_deref().unwrap_or_default()
    )
}

fn cli_should_emit_scheduler_stage_block(
    snapshots: &Arc<Mutex<HashMap<String, String>>>,
    stage: &SchedulerStageBlock,
) -> bool {
    let stage_id = stage.stage_id.clone().unwrap_or_else(|| {
        format!(
            "{}:{}",
            stage.stage_index.unwrap_or_default(),
            stage.stage.as_str()
        )
    });
    let snapshot = cli_scheduler_stage_snapshot_key(stage);
    let Ok(mut cache) = snapshots.lock() else {
        return true;
    };
    match cache.get(&stage_id) {
        Some(existing) if existing == &snapshot => false,
        _ => {
            cache.insert(stage_id, snapshot);
            true
        }
    }
}

#[cfg(test)]
fn extend_wrapped_lines(out: &mut Vec<String>, text: &str, width: usize) {
    if text.is_empty() {
        out.push(String::new());
        return;
    }
    let wrapped = wrap_display_text(text, width.max(1));
    if wrapped.is_empty() {
        out.push(String::new());
    } else {
        out.extend(wrapped);
    }
}

#[cfg(test)]
fn cli_fit_lines(lines: &[String], width: usize, rows: usize, tail: bool) -> Vec<String> {
    let mut wrapped = Vec::new();
    for line in lines {
        extend_wrapped_lines(&mut wrapped, line, width);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    if wrapped.len() > rows {
        if tail {
            wrapped.split_off(wrapped.len().saturating_sub(rows))
        } else {
            wrapped.truncate(rows);
            wrapped
        }
    } else {
        wrapped.resize(rows, String::new());
        wrapped
    }
}

#[cfg(test)]
fn cli_box_line(text: &str, inner_width: usize, style: &CliStyle) -> String {
    let content = pad_right_display(text, inner_width, ' ');
    if style.color {
        format!("{} {} {}", style.cyan("│"), content, style.cyan("│"))
    } else {
        format!("│ {} │", content)
    }
}

#[cfg(test)]
fn cli_render_box(
    title: &str,
    footer: Option<&str>,
    lines: &[String],
    outer_width: usize,
    style: &CliStyle,
) -> Vec<String> {
    let inner_width = outer_width.saturating_sub(4).max(1);
    let chrome_width = inner_width + 2;
    let header_content = pad_right_display(
        &truncate_display(&format!(" {} ", title.trim()), chrome_width),
        chrome_width,
        '─',
    );
    let header = if style.color {
        format!(
            "{}{}{}",
            style.cyan("╭"),
            style.bold_cyan(&header_content),
            style.cyan("╮")
        )
    } else {
        format!("╭{}╮", header_content)
    };

    let footer_text = footer.unwrap_or("");
    let footer_content = if footer_text.is_empty() {
        "─".repeat(chrome_width)
    } else {
        pad_right_display(
            &truncate_display(&format!(" {} ", footer_text.trim()), chrome_width),
            chrome_width,
            '─',
        )
    };
    let footer = if style.color {
        format!(
            "{}{}{}",
            style.cyan("╰"),
            style.dim(&footer_content),
            style.cyan("╯")
        )
    } else {
        format!("╰{}╯", footer_content)
    };

    let mut rendered = Vec::with_capacity(lines.len() + 2);
    rendered.push(header);
    rendered.extend(
        lines
            .iter()
            .map(|line| cli_box_line(line, inner_width, style)),
    );
    rendered.push(footer);
    rendered
}

#[cfg(test)]
fn cli_join_columns(
    left: &[String],
    left_width: usize,
    right: &[String],
    right_width: usize,
    gap: usize,
) -> Vec<String> {
    let blank_left = " ".repeat(left_width);
    let blank_right = " ".repeat(right_width);
    let height = left.len().max(right.len());
    let mut rows = Vec::with_capacity(height);
    for index in 0..height {
        let left_line = left.get(index).map(String::as_str).unwrap_or(&blank_left);
        let right_line = right.get(index).map(String::as_str).unwrap_or(&blank_right);
        rows.push(format!("{}{}{}", left_line, " ".repeat(gap), right_line));
    }
    rows
}

#[cfg(test)]
fn cli_terminal_rows() -> usize {
    crossterm::terminal::size()
        .map(|(_, rows)| usize::from(rows))
        .unwrap_or(28)
}

#[cfg(test)]
fn cli_sidebar_lines(
    projection: &CliFrontendProjection,
    topology: &CliObservedExecutionTopology,
) -> Vec<String> {
    let phase = match projection.phase {
        CliFrontendPhase::Idle => "idle",
        CliFrontendPhase::Busy => "busy",
        CliFrontendPhase::Waiting => "waiting",
        CliFrontendPhase::Cancelling => "cancelling",
        CliFrontendPhase::Failed => "error",
    };
    let mut lines = vec![
        format!("Phase: {}", phase),
        format!(
            "Queue: {}",
            if projection.queue_len == 0 {
                "empty".to_string()
            } else {
                projection.queue_len.to_string()
            }
        ),
    ];
    if let Some(active) = projection
        .active_label
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("Activity: {active}"));
    }
    if topology.active {
        lines.push("Execution: active".to_string());
    } else {
        lines.push("Execution: idle".to_string());
    }
    if let Some(active_stage_id) = topology.active_stage_id.as_deref() {
        if let Some(node) = topology.nodes.get(active_stage_id) {
            lines.push(format!("Node: {}", node.label));
            lines.push(format!("Status: {}", node.status));
            if let Some(waiting_on) = node.waiting_on.as_deref() {
                lines.push(format!("Waiting: {waiting_on}"));
            }
            if let Some(recent_event) = node.recent_event.as_deref() {
                lines.push(format!("Last: {recent_event}"));
            }
        }
    }

    // ── Context (token usage + cost) ────────────────────────────
    let ts = &projection.token_stats;
    if ts.total_tokens > 0 {
        lines.push(String::new());
        lines.push("─ Context ─".to_string());
        lines.push(format!("Tokens: {}", format_token_count(ts.total_tokens)));
        lines.push(format!("Cost:   ${:.4}", ts.total_cost));
    }

    // ── MCP servers ─────────────────────────────────────────────
    if !projection.mcp_servers.is_empty() {
        let connected = projection
            .mcp_servers
            .iter()
            .filter(|s| s.status == "connected")
            .count();
        let errored = projection
            .mcp_servers
            .iter()
            .filter(|s| s.status == "failed" || s.status == "error")
            .count();
        lines.push(String::new());
        lines.push(format!("─ MCP ({} active, {} err) ─", connected, errored));
        for server in &projection.mcp_servers {
            let indicator = match server.status.as_str() {
                "connected" => "●",
                "failed" | "error" => "✗",
                "needs_auth" | "needs auth" => "?",
                _ => "○",
            };
            lines.push(format!("{} {} [{}]", indicator, server.name, server.status));
            if let Some(ref err) = server.error {
                lines.push(format!("  ↳ {}", err));
            }
        }
    }

    // ── LSP servers ─────────────────────────────────────────────
    if !projection.lsp_servers.is_empty() {
        lines.push(String::new());
        lines.push(format!("─ LSP ({}) ─", projection.lsp_servers.len()));
        for server in &projection.lsp_servers {
            lines.push(format!("● {}", server));
        }
    }

    lines.push(String::new());
    lines.push("/help · /model · /preset".to_string());
    lines.push("/child · /abort · /sidebar".to_string());
    lines.push("/status · /compact · /new".to_string());
    lines
}

/// Format a token count for display (e.g., 1234 → "1,234", 1234567 → "1.2M").
fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
fn cli_active_stage_panel_lines(
    stage: Option<&SchedulerStageBlock>,
    style: &CliStyle,
) -> Vec<String> {
    let Some(stage) = stage else {
        return vec![
            "No active stage. Running work will appear here in-place.".to_string(),
            "Transcript stays on the left; live execution stays here.".to_string(),
            String::new(),
            "Queued prompts remain editable in the input box below.".to_string(),
            "Use /abort to stop the active execution boundary.".to_string(),
        ];
    };

    let mut lines = cli_active_stage_context_lines(Some(stage), style);
    if let Some(activity) = stage.activity.as_deref().filter(|value| !value.is_empty()) {
        lines.push(format!("Activity: {}", activity.replace('\n', " · ")));
    }
    let mut available = Vec::new();
    if let Some(count) = stage.available_skill_count {
        available.push(format!("skills {}", count));
    }
    if let Some(count) = stage.available_agent_count {
        available.push(format!("agents {}", count));
    }
    if let Some(count) = stage.available_category_count {
        available.push(format!("categories {}", count));
    }
    if !available.is_empty() {
        lines.push(format!("Available: {}", available.join(" · ")));
    }
    if !stage.active_skills.is_empty() {
        lines.push(format!("Active skills: {}", stage.active_skills.join(", ")));
    }
    if stage.total_agent_count > 0 {
        lines.push(format!(
            "Agents: [{}/{}]",
            stage.done_agent_count, stage.total_agent_count
        ));
    }
    if let Some(ref child_id) = stage.child_session_id {
        lines.push(format!("→ Child session: {}", child_id));
    }
    lines
}

#[cfg(test)]
fn cli_messages_footer(
    transcript: &CliRetainedTranscript,
    width: usize,
    max_rows: usize,
    scroll_offset: usize,
) -> String {
    let total = transcript.total_rows(width);
    if total <= max_rows {
        return "retained transcript".to_string();
    }
    if scroll_offset == 0 {
        format!("↑ /up to scroll · {} lines total", total)
    } else {
        let max_offset = total.saturating_sub(max_rows);
        let clamped = scroll_offset.min(max_offset);
        let position = max_offset.saturating_sub(clamped);
        format!("line {}/{} · /up /down /bottom", position + 1, total,)
    }
}

#[cfg(test)]
fn cli_render_retained_layout(
    mode: &str,
    model: &str,
    directory: &str,
    projection: &CliFrontendProjection,
    topology: &CliObservedExecutionTopology,
    style: &CliStyle,
) -> Vec<String> {
    let total_width = usize::from(style.width.saturating_sub(1)).clamp(60, 160);
    let terminal_rows = cli_terminal_rows().max(20);
    let gap = 1usize;

    // Session header — compact single-line with session title
    let session_title = projection
        .session_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(untitled)");
    let mut header_parts = vec![
        truncate_display(session_title, 32),
        mode.to_string(),
        model.to_string(),
    ];
    if let Some(view_label) = projection
        .view_label
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        header_parts.push(view_label.to_string());
    }
    header_parts.push(truncate_display(directory, 24));
    let header_lines = vec![format!("> {}", header_parts.join(" · "))];
    let header_box = cli_render_box("ROCode", None, &header_lines, total_width, style);

    // ── Adaptive layout: compute actual content sizes, then allocate rows ──
    //
    // Fixed chrome overhead (lines consumed by box borders):
    //   header_box:   3 lines (top border + 1 content + bottom border)
    //   messages_box: 2 lines (top border + bottom border)
    //   active_box:   2 lines (top border + bottom border) when expanded, 3 when collapsed
    //   prompt:       ~8 lines (rendered separately by PromptFrame, not counted in screen_lines)
    //
    // Remaining rows after chrome are split between messages content and active content,
    // with active getting exactly what it needs (clamped) and messages getting the rest.

    let active_inner_width = total_width.saturating_sub(4).max(1);

    // Compute active panel's natural content height
    let (active_content_lines, active_chrome) = if projection.active_collapsed {
        // Collapsed: single label line + 2 chrome = 3 total
        (Vec::new(), 3usize) // content handled separately below
    } else {
        let raw_lines = cli_active_stage_panel_lines(projection.active_stage.as_ref(), style);
        // Wrap lines to actual width to get true row count
        let mut wrapped_count = 0usize;
        for line in &raw_lines {
            wrapped_count += 1.max(
                (display_width(line) + active_inner_width.saturating_sub(1))
                    / active_inner_width.max(1),
            );
        }
        let natural_rows = if raw_lines.is_empty() {
            1
        } else {
            wrapped_count
        };
        (raw_lines, 2 + natural_rows.clamp(2, 12)) // chrome(2) + content(2..12)
    };

    // Total chrome = header(3) + messages chrome(2) + active chrome + prompt overhead(~8)
    let prompt_overhead = 8usize; // header + ~6 visible rows + footer
    let total_chrome = 3 + 2 + active_chrome + prompt_overhead;
    let sidebar_overhead = if projection.sidebar_collapsed { 3 } else { 0 };

    // Messages get whatever remains after chrome and active content
    let body_rows = terminal_rows.saturating_sub(total_chrome).max(4) + sidebar_overhead;

    let mut screen = Vec::new();
    screen.extend(header_box);

    if projection.sidebar_collapsed {
        // Full-width Messages only, no sidebar column
        let messages_inner = total_width.saturating_sub(4).max(1);
        let transcript_lines = projection.transcript.viewport_lines(
            messages_inner,
            body_rows,
            projection.scroll_offset,
        );
        let messages_footer = cli_messages_footer(
            &projection.transcript,
            messages_inner,
            body_rows,
            projection.scroll_offset,
        );
        let messages_box = cli_render_box(
            "Messages",
            Some(&messages_footer),
            &transcript_lines,
            total_width,
            style,
        );
        screen.extend(messages_box);
    } else {
        let right_width = (if total_width >= 128 { 38 } else { 32 })
            .min(total_width.saturating_sub(29 + gap))
            .max(24);
        let left_width = total_width.saturating_sub(right_width + gap);
        let left_inner = left_width.saturating_sub(4).max(1);
        let right_inner = right_width.saturating_sub(4).max(1);
        let transcript_lines =
            projection
                .transcript
                .viewport_lines(left_inner, body_rows, projection.scroll_offset);
        let messages_footer = cli_messages_footer(
            &projection.transcript,
            left_inner,
            body_rows,
            projection.scroll_offset,
        );
        let sidebar_lines = cli_fit_lines(
            &cli_sidebar_lines(projection, topology),
            right_inner,
            body_rows,
            false,
        );
        let messages_box = cli_render_box(
            "Messages",
            Some(&messages_footer),
            &transcript_lines,
            left_width,
            style,
        );
        let sidebar_box = cli_render_box("Sidebar", None, &sidebar_lines, right_width, style);
        let body = cli_join_columns(&messages_box, left_width, &sidebar_box, right_width, gap);
        screen.extend(body);
    }

    if projection.active_collapsed {
        // Single collapsed bar
        let collapsed_label = if let Some(stage) = projection.active_stage.as_ref() {
            format!(
                "▸ {} (collapsed — /active to expand)",
                truncate_display(&stage.title, total_width.saturating_sub(48).max(12)),
            )
        } else {
            "▸ No active stage (/active to expand)".to_string()
        };
        let active_box = cli_render_box("Active", None, &[collapsed_label], total_width, style);
        screen.extend(active_box);
    } else {
        // Use actual content height (already clamped 2..12 during budget computation)
        let active_rows = active_chrome.saturating_sub(2); // remove chrome to get content rows
        let active_lines = cli_fit_lines(
            &active_content_lines,
            active_inner_width,
            active_rows,
            false,
        );
        let active_box = cli_render_box("Active", None, &active_lines, total_width, style);
        screen.extend(active_box);
    }

    screen
}
