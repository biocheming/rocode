use futures::StreamExt;
use rocode_command::cli_style::CliStyle;
use rocode_command::output_blocks::{render_cli_block_rich, OutputBlock};
use rocode_config::schema::ShareMode;
use rocode_config::Config;
use serde::Deserialize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rocode_types::ServerEvent;

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
    serde_json::from_value(payload.clone()).ok()
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
    let server_event = {
        if let Ok(event) = serde_json::from_value::<ServerEvent>(parsed.clone()) {
            Some(event)
        } else if let (Some(event_name), Some(obj)) = (event_name.as_deref(), parsed.as_object()) {
            if obj.contains_key("type") {
                None
            } else {
                let mut patched = obj.clone();
                patched.insert(
                    "type".to_string(),
                    serde_json::Value::String(event_name.to_string()),
                );
                serde_json::from_value::<ServerEvent>(serde_json::Value::Object(patched)).ok()
            }
        } else {
            None
        }
    };

    #[derive(Debug, serde::Deserialize)]
    struct EventTypeOnly {
        #[serde(rename = "type")]
        event_type: Option<String>,
    }

    let event_type = if let Some(name) = event_name.as_deref().filter(|s| !s.is_empty()) {
        name.to_string()
    } else if let Some(ref event) = server_event {
        event.event_name().to_string()
    } else {
        serde_json::from_value::<EventTypeOnly>(parsed.clone())
            .ok()
            .and_then(|v| v.event_type)
            .unwrap_or_else(|| "message".to_string())
    };

    if matches!(server_event.as_ref(), Some(ServerEvent::ConfigUpdated))
        || event_type.as_str() == "config.updated"
    {
        if let Ok(config) = fetch_remote_config(client, base_url).await {
            if let Some(enabled) = remote_show_thinking_from_config(&config) {
                show_thinking.store(enabled, Ordering::SeqCst);
            }
        }
    }

    if matches!(format, &RunOutputFormat::Json) {
        let mut output = serde_json::Map::new();
        output.insert(
            "type".to_string(),
            serde_json::Value::String(event_type.clone()),
        );
        output.insert(
            "timestamp".to_string(),
            serde_json::json!(chrono::Utc::now().timestamp_millis()),
        );
        output.insert(
            "sessionID".to_string(),
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

    if event_type == "output_block" {
        let payload = server_event
            .as_ref()
            .and_then(|event| match event {
                ServerEvent::OutputBlock { block, .. } => Some(block.clone()),
                _ => None,
            })
            .unwrap_or_else(|| parsed.clone());
        if let Some(block) = parse_output_block(&payload) {
            if matches!(block, OutputBlock::Reasoning(_)) && !show_thinking.load(Ordering::SeqCst) {
                return Ok(());
            }
            let style = CliStyle::detect();
            print!("{}", render_cli_block_rich(&block, &style));
            io::stdout().flush()?;
        }
        return Ok(());
    }

    if event_type.as_str() == "error" {
        if let Some(ServerEvent::Error { error, .. }) = server_event.as_ref() {
            eprintln!("\nError: {}", error);
            return Ok(());
        }

        #[derive(Debug, serde::Deserialize)]
        struct ErrorLike {
            #[serde(default)]
            error: Option<String>,
            #[serde(default)]
            message: Option<String>,
        }

        let message = serde_json::from_value::<ErrorLike>(parsed.clone())
            .ok()
            .and_then(|err| err.error.or(err.message))
            .unwrap_or_else(|| "unknown remote stream error".to_string());
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
            "variant": variant,
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

    #[test]
    fn parses_canonical_scheduler_stage_payload() {
        let fixture = canonical_scheduler_stage_fixture();
        let block = parse_output_block(&fixture.payload).expect("scheduler stage block");
        assert_eq!(block, OutputBlock::SchedulerStage(Box::new(fixture.block)));
    }

    #[test]
    fn parses_reasoning_payload() {
        let payload = serde_json::json!({
            "kind": "reasoning",
            "phase": "delta",
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
