use futures::StreamExt;
use rocode_command::cli_style::CliStyle;
use rocode_command::output_blocks::{
    render_cli_block_rich, BlockTone, MessageBlock, MessagePhase, MessageRole, OutputBlock,
    QueueItemBlock, ReasoningBlock, SchedulerDecisionBlock, SchedulerDecisionField,
    SchedulerDecisionRenderSpec, SchedulerDecisionSection, SchedulerStageBlock, SessionEventBlock,
    SessionEventField, StatusBlock, ToolBlock, ToolPhase,
};
use rocode_config::schema::ShareMode;
use rocode_config::Config;
use rocode_core::contracts::events::ServerEventType;
use rocode_core::contracts::output_blocks::{
    keys as block_keys, scheduler_decision_keys as decision_keys,
    scheduler_decision_spec_keys as decision_spec_keys, scheduler_stage_keys as stage_keys,
    BlockToneWire, MessagePhaseWire, MessageRoleWire, OutputBlockKind, ToolPhaseWire,
};
use rocode_core::contracts::session::keys as session_keys;
use rocode_core::contracts::wire::{fields as wire_fields, keys as wire_keys};
use serde::Deserialize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::cli::RunOutputFormat;
use crate::util::{parse_bool_env, parse_http_json, server_url};

#[derive(Debug, Deserialize)]
struct RemoteSessionInfo {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteShareInfo {
    url: String,
}

pub(crate) struct RemoteAttachOptions {
    pub base_url: String,
    pub input: String,
    pub command: Option<String>,
    pub continue_last: bool,
    pub session: Option<String>,
    pub fork: bool,
    pub share: bool,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub variant: Option<String>,
    pub format: RunOutputFormat,
    pub title: Option<String>,
    pub show_thinking: bool,
}

const DEFAULT_SSE_EVENT_NAME: &str = "message";

fn remote_show_thinking_from_config(config: &Config) -> Option<bool> {
    config
        .ui_preferences
        .as_ref()
        .and_then(|ui| ui.show_thinking)
}

async fn fetch_remote_config(client: &reqwest::Client, base_url: &str) -> anyhow::Result<Config> {
    let config_endpoint = server_url(base_url, "/config");
    parse_http_json(client.get(config_endpoint).send().await?).await
}

pub(crate) fn parse_output_block(payload: &serde_json::Value) -> Option<OutputBlock> {
    let kind_raw = payload.get(block_keys::KIND)?.as_str()?;
    let kind = OutputBlockKind::parse(kind_raw)?;

    match kind {
        OutputBlockKind::Status => {
            let tone_raw = payload
                .get(block_keys::TONE)
                .and_then(|v| v.as_str())
                .unwrap_or(BlockToneWire::Normal.as_str());
            let tone = match BlockToneWire::parse(tone_raw) {
                Some(BlockToneWire::Title) => BlockTone::Title,
                Some(BlockToneWire::Muted) => BlockTone::Muted,
                Some(BlockToneWire::Success) => BlockTone::Success,
                Some(BlockToneWire::Warning) => BlockTone::Warning,
                Some(BlockToneWire::Error) => BlockTone::Error,
                _ => BlockTone::Normal,
            };
            let text = payload
                .get(block_keys::TEXT)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(OutputBlock::Status(StatusBlock { tone, text }))
        }
        OutputBlockKind::Message => {
            let role_raw = payload
                .get(block_keys::ROLE)
                .and_then(|v| v.as_str())
                .unwrap_or(MessageRoleWire::Assistant.as_str());
            let role = match MessageRoleWire::parse(role_raw) {
                Some(MessageRoleWire::User) => MessageRole::User,
                Some(MessageRoleWire::System) => MessageRole::System,
                _ => MessageRole::Assistant,
            };

            let phase_raw = payload
                .get(block_keys::PHASE)
                .and_then(|v| v.as_str())
                .unwrap_or(MessagePhaseWire::Delta.as_str());
            let phase = match MessagePhaseWire::parse(phase_raw) {
                Some(MessagePhaseWire::Start) => MessagePhase::Start,
                Some(MessagePhaseWire::End) => MessagePhase::End,
                Some(MessagePhaseWire::Full) => MessagePhase::Full,
                _ => MessagePhase::Delta,
            };

            let text = payload
                .get(block_keys::TEXT)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(OutputBlock::Message(MessageBlock { role, phase, text }))
        }
        OutputBlockKind::Tool => {
            let name = payload
                .get(block_keys::NAME)
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let phase_raw = payload
                .get(block_keys::PHASE)
                .and_then(|v| v.as_str())
                .unwrap_or(ToolPhaseWire::Running.as_str());
            let phase = match ToolPhaseWire::parse(phase_raw) {
                Some(ToolPhaseWire::Start) => ToolPhase::Start,
                Some(ToolPhaseWire::Done) => ToolPhase::Done,
                Some(ToolPhaseWire::Error) => ToolPhase::Error,
                _ => ToolPhase::Running,
            };
            let detail = payload
                .get(block_keys::DETAIL)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(OutputBlock::Tool(ToolBlock {
                name,
                phase,
                detail,
                structured: None,
            }))
        }
        OutputBlockKind::Reasoning => {
            let phase_raw = payload
                .get(block_keys::PHASE)
                .and_then(|v| v.as_str())
                .unwrap_or(MessagePhaseWire::Delta.as_str());
            let phase = match MessagePhaseWire::parse(phase_raw) {
                Some(MessagePhaseWire::Start) => MessagePhase::Start,
                Some(MessagePhaseWire::End) => MessagePhase::End,
                Some(MessagePhaseWire::Full) => MessagePhase::Full,
                _ => MessagePhase::Delta,
            };
            let text = payload
                .get(block_keys::TEXT)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(OutputBlock::Reasoning(ReasoningBlock { phase, text }))
        }
        OutputBlockKind::SessionEvent => Some(OutputBlock::SessionEvent(SessionEventBlock {
            event: payload
                .get(block_keys::EVENT)
                .and_then(|v| v.as_str())
                .unwrap_or("event")
                .to_string(),
            title: payload
                .get(block_keys::TITLE)
                .and_then(|v| v.as_str())
                .unwrap_or("Session Event")
                .to_string(),
            status: payload
                .get(block_keys::STATUS)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            summary: payload
                .get(block_keys::SUMMARY)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            fields: payload
                .get(block_keys::FIELDS)
                .and_then(|v| v.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|field| {
                            Some(SessionEventField {
                                label: field.get(block_keys::LABEL)?.as_str()?.to_string(),
                                value: field.get(block_keys::VALUE)?.as_str()?.to_string(),
                                tone: field
                                    .get(block_keys::TONE)
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            body: payload
                .get(block_keys::BODY)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })),
        OutputBlockKind::QueueItem => Some(OutputBlock::QueueItem(QueueItemBlock {
            position: payload
                .get(block_keys::POSITION)
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as usize,
            text: payload
                .get(block_keys::TEXT)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })),
        OutputBlockKind::SchedulerStage => {
            Some(OutputBlock::SchedulerStage(Box::new(SchedulerStageBlock {
                stage_id: payload
                    .get(stage_keys::STAGE_ID)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                profile: payload
                    .get(stage_keys::PROFILE)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                stage: payload
                    .get(stage_keys::STAGE)
                    .and_then(|v| v.as_str())
                    .unwrap_or("stage")
                    .to_string(),
                title: payload
                    .get(block_keys::TITLE)
                    .and_then(|v| v.as_str())
                    .unwrap_or("Scheduler Stage")
                    .to_string(),
                text: payload
                    .get(block_keys::TEXT)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                stage_index: payload
                    .get(stage_keys::STAGE_INDEX)
                    .and_then(|v| v.as_u64()),
                stage_total: payload
                    .get(stage_keys::STAGE_TOTAL)
                    .and_then(|v| v.as_u64()),
                step: payload.get(stage_keys::STEP).and_then(|v| v.as_u64()),
                status: payload
                    .get(block_keys::STATUS)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                focus: payload
                    .get(stage_keys::FOCUS)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                last_event: payload
                    .get(stage_keys::LAST_EVENT)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                waiting_on: payload
                    .get(stage_keys::WAITING_ON)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                activity: payload
                    .get(stage_keys::ACTIVITY)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                loop_budget: payload
                    .get(stage_keys::LOOP_BUDGET)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                available_skill_count: payload
                    .get(stage_keys::AVAILABLE_SKILL_COUNT)
                    .and_then(|v| v.as_u64()),
                available_agent_count: payload
                    .get(stage_keys::AVAILABLE_AGENT_COUNT)
                    .and_then(|v| v.as_u64()),
                available_category_count: payload
                    .get(stage_keys::AVAILABLE_CATEGORY_COUNT)
                    .and_then(|v| v.as_u64()),
                active_skills: payload
                    .get(stage_keys::ACTIVE_SKILLS)
                    .and_then(|v| v.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                active_agents: payload
                    .get(stage_keys::ACTIVE_AGENTS)
                    .and_then(|v| v.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                active_categories: payload
                    .get(stage_keys::ACTIVE_CATEGORIES)
                    .and_then(|v| v.as_array())
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                done_agent_count: payload
                    .get(stage_keys::DONE_AGENT_COUNT)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                total_agent_count: payload
                    .get(stage_keys::TOTAL_AGENT_COUNT)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                prompt_tokens: payload
                    .get(wire_fields::PROMPT_TOKENS)
                    .and_then(|v| v.as_u64()),
                completion_tokens: payload
                    .get(wire_fields::COMPLETION_TOKENS)
                    .and_then(|v| v.as_u64()),
                reasoning_tokens: payload
                    .get(stage_keys::REASONING_TOKENS)
                    .and_then(|v| v.as_u64()),
                cache_read_tokens: payload
                    .get(stage_keys::CACHE_READ_TOKENS)
                    .and_then(|v| v.as_u64()),
                cache_write_tokens: payload
                    .get(stage_keys::CACHE_WRITE_TOKENS)
                    .and_then(|v| v.as_u64()),
                decision: parse_scheduler_decision(payload.get(block_keys::DECISION)),
                child_session_id: payload
                    .get(stage_keys::CHILD_SESSION_ID)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })))
        }
        _ => None,
    }
}

fn parse_scheduler_decision(payload: Option<&serde_json::Value>) -> Option<SchedulerDecisionBlock> {
    let payload = payload?;
    Some(SchedulerDecisionBlock {
        kind: payload.get(decision_keys::KIND)?.as_str()?.to_string(),
        title: payload.get(decision_keys::TITLE)?.as_str()?.to_string(),
        spec: parse_scheduler_decision_spec(payload.get(decision_keys::SPEC))?,
        fields: payload
            .get(decision_keys::FIELDS)
            .and_then(|value| value.as_array())
            .map(|fields| {
                fields
                    .iter()
                    .filter_map(|field| {
                        Some(SchedulerDecisionField {
                            label: field.get(block_keys::LABEL)?.as_str()?.to_string(),
                            value: field.get(block_keys::VALUE)?.as_str()?.to_string(),
                            tone: field
                                .get(block_keys::TONE)
                                .and_then(|value| value.as_str())
                                .map(|value| value.to_string()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        sections: payload
            .get(decision_keys::SECTIONS)
            .and_then(|value| value.as_array())
            .map(|sections| {
                sections
                    .iter()
                    .filter_map(|section| {
                        Some(SchedulerDecisionSection {
                            title: section.get(block_keys::TITLE)?.as_str()?.to_string(),
                            body: section.get(block_keys::BODY)?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn parse_scheduler_decision_spec(
    payload: Option<&serde_json::Value>,
) -> Option<SchedulerDecisionRenderSpec> {
    let payload = payload?;
    Some(SchedulerDecisionRenderSpec {
        version: payload
            .get(decision_spec_keys::VERSION)?
            .as_str()?
            .to_string(),
        show_header_divider: payload
            .get(decision_spec_keys::SHOW_HEADER_DIVIDER)?
            .as_bool()?,
        field_order: payload
            .get(decision_spec_keys::FIELD_ORDER)?
            .as_str()?
            .to_string(),
        field_label_emphasis: payload
            .get(decision_spec_keys::FIELD_LABEL_EMPHASIS)?
            .as_str()?
            .to_string(),
        status_palette: payload
            .get(decision_spec_keys::STATUS_PALETTE)?
            .as_str()?
            .to_string(),
        section_spacing: payload
            .get(decision_spec_keys::SECTION_SPACING)?
            .as_str()?
            .to_string(),
        update_policy: payload
            .get(decision_spec_keys::UPDATE_POLICY)?
            .as_str()?
            .to_string(),
    })
}

pub(crate) async fn resolve_remote_session(
    client: &reqwest::Client,
    base_url: &str,
    continue_last: bool,
    session: Option<String>,
    fork: bool,
    title: Option<String>,
) -> anyhow::Result<String> {
    let base_id = if let Some(session_id) = session {
        Some(session_id)
    } else if continue_last {
        let list_endpoint = server_url(base_url, "/session?roots=true&limit=100");
        let sessions: Vec<RemoteSessionInfo> =
            parse_http_json(client.get(list_endpoint).send().await?).await?;
        sessions
            .into_iter()
            .find(|s| s.parent_id.is_none())
            .map(|s| s.id)
    } else {
        None
    };

    if let Some(base_id) = base_id {
        if fork {
            let fork_endpoint = server_url(base_url, &format!("/session/{}/fork", base_id));
            let forked: RemoteSessionInfo = parse_http_json(
                client
                    .post(fork_endpoint)
                    .json(&serde_json::json!({ "message_id": null }))
                    .send()
                    .await?,
            )
            .await?;
            return Ok(forked.id);
        }
        return Ok(base_id);
    }

    let create_endpoint = server_url(base_url, "/session");
    let created: RemoteSessionInfo = parse_http_json(
        client
            .post(create_endpoint)
            .json(&serde_json::json!({
                "title": title
            }))
            .send()
            .await?,
    )
    .await?;
    Ok(created.id)
}

pub(crate) async fn maybe_share_remote_session(
    client: &reqwest::Client,
    base_url: &str,
    session_id: &str,
    share_requested: bool,
) -> anyhow::Result<()> {
    let auto_share_env = std::env::var("ROCODE_AUTO_SHARE")
        .or_else(|_| std::env::var("OPENCODE_AUTO_SHARE"))
        .ok()
        .map(|v| parse_bool_env(&v))
        .unwrap_or(false);
    let config = fetch_remote_config(client, base_url).await?;
    let config_auto = matches!(config.share, Some(ShareMode::Auto));

    if !(share_requested || auto_share_env || config_auto) {
        return Ok(());
    }

    let share_endpoint = server_url(base_url, &format!("/session/{}/share", session_id));
    let shared: RemoteShareInfo =
        parse_http_json(client.post(share_endpoint).send().await?).await?;
    println!("~  {}", shared.url);
    Ok(())
}

pub(crate) async fn consume_remote_sse(
    response: reqwest::Response,
    client: &reqwest::Client,
    base_url: &str,
    session_id: &str,
    format: RunOutputFormat,
    show_thinking: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut current_event: Option<String> = None;
    let mut current_data: Vec<String> = Vec::new();

    loop {
        let Some(chunk) = StreamExt::next(&mut stream).await else {
            break;
        };
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buffer.find('\n') {
            let mut line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            if line.is_empty() {
                let data = current_data.join("\n");
                dispatch_remote_sse_event(
                    client,
                    base_url,
                    &show_thinking,
                    session_id,
                    &format,
                    current_event.take(),
                    data,
                )
                .await?;
                current_data.clear();
                continue;
            }
            if let Some(event) = line.strip_prefix("event:") {
                current_event = Some(event.trim().to_string());
            } else if let Some(data) = line.strip_prefix("data:") {
                current_data.push(data.trim_start().to_string());
            }
        }
    }

    if !current_data.is_empty() {
        dispatch_remote_sse_event(
            client,
            base_url,
            &show_thinking,
            session_id,
            &format,
            current_event.take(),
            current_data.join("\n"),
        )
        .await?;
    }

    Ok(())
}

async fn dispatch_remote_sse_event(
    client: &reqwest::Client,
    base_url: &str,
    show_thinking: &Arc<AtomicBool>,
    session_id: &str,
    format: &RunOutputFormat,
    event_name: Option<String>,
    data: String,
) -> anyhow::Result<()> {
    if data.trim().is_empty() {
        return Ok(());
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({ "raw": data }));
    let event_type = event_name
        .or_else(|| {
            parsed
                .get(wire_keys::TYPE)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| DEFAULT_SSE_EVENT_NAME.to_string());

    let event_kind = ServerEventType::parse(&event_type);

    if event_kind == Some(ServerEventType::ConfigUpdated) {
        if let Ok(config) = fetch_remote_config(client, base_url).await {
            if let Some(enabled) = remote_show_thinking_from_config(&config) {
                show_thinking.store(enabled, Ordering::SeqCst);
            }
        }
    }

    if matches!(format, &RunOutputFormat::Json) {
        let mut output = serde_json::Map::new();
        output.insert(
            wire_keys::TYPE.to_string(),
            serde_json::Value::String(event_type.clone()),
        );
        output.insert(
            "timestamp".to_string(),
            serde_json::json!(chrono::Utc::now().timestamp_millis()),
        );
        output.insert(
            wire_keys::SESSION_ID.to_string(),
            serde_json::Value::String(session_id.to_string()),
        );
        match parsed {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    output.insert(k, v);
                }
            }
            other => {
                output.insert("data".to_string(), other);
            }
        }
        println!("{}", serde_json::Value::Object(output));
        return Ok(());
    }

    if event_kind == Some(ServerEventType::OutputBlock) {
        let payload = parsed.get(wire_keys::BLOCK).unwrap_or(&parsed);
        if let Some(block) = parse_output_block(payload) {
            if matches!(block, OutputBlock::Reasoning(_)) && !show_thinking.load(Ordering::SeqCst) {
                return Ok(());
            }
            let style = CliStyle::detect();
            print!("{}", render_cli_block_rich(&block, &style));
            io::stdout().flush()?;
        }
        return Ok(());
    }

    if event_kind == Some(ServerEventType::Error) {
        let message = parsed
            .get(wire_keys::ERROR)
            .and_then(|v| v.as_str())
            .or_else(|| parsed.get(wire_keys::MESSAGE).and_then(|v| v.as_str()))
            .unwrap_or("unknown remote stream error");
        eprintln!("\nError: {}", message);
    }
    Ok(())
}

pub(crate) async fn run_non_interactive_attach(options: RemoteAttachOptions) -> anyhow::Result<()> {
    let RemoteAttachOptions {
        base_url,
        input,
        command,
        continue_last,
        session,
        fork,
        share,
        model,
        agent,
        scheduler_profile,
        variant,
        format,
        title,
        show_thinking,
    } = options;
    let client = reqwest::Client::new();
    let show_thinking = Arc::new(AtomicBool::new(show_thinking));
    let session_id =
        resolve_remote_session(&client, &base_url, continue_last, session, fork, title).await?;
    maybe_share_remote_session(&client, &base_url, &session_id, share).await?;

    let content = if let Some(command_name) = command {
        if input.trim().is_empty() {
            format!("/{}", command_name)
        } else {
            format!("/{} {}", command_name, input)
        }
    } else {
        input
    };

    let endpoint = server_url(&base_url, &format!("/session/{}/stream", session_id));
    let response = client
        .post(endpoint)
        .json(&serde_json::json!({
            "content": content,
            "model": model,
            "agent": agent,
            "scheduler_profile": scheduler_profile,
            session_keys::MODEL_VARIANT: variant,
            "stream": true
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Remote run failed ({}): {}", status, body);
    }

    consume_remote_sse(
        response,
        &client,
        &base_url,
        &session_id,
        format,
        show_thinking,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::parse_output_block;
    use rocode_command::governance_fixtures::canonical_scheduler_stage_fixture;
    use rocode_command::output_blocks::{MessagePhase, OutputBlock};
    use rocode_core::contracts::output_blocks::{MessagePhaseWire, OutputBlockKind};

    #[test]
    fn parses_canonical_scheduler_stage_payload() {
        let fixture = canonical_scheduler_stage_fixture();
        let block = parse_output_block(&fixture.payload).expect("scheduler stage block");
        assert_eq!(block, OutputBlock::SchedulerStage(Box::new(fixture.block)));
    }

    #[test]
    fn parses_reasoning_payload() {
        let payload = serde_json::json!({
            "kind": OutputBlockKind::Reasoning.as_str(),
            "phase": MessagePhaseWire::Delta.as_str(),
            "text": "thinking"
        });
        let block = parse_output_block(&payload).expect("reasoning block");
        assert!(matches!(
            block,
            OutputBlock::Reasoning(reasoning)
                if reasoning.phase == MessagePhase::Delta && reasoning.text == "thinking"
        ));
    }
}
