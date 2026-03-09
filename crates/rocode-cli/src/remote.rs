use futures::StreamExt;
use rocode_command::cli_style::CliStyle;
use rocode_command::output_blocks::{
    render_cli_block_rich, BlockTone, MessageBlock, MessagePhase, MessageRole, OutputBlock,
    StatusBlock, ToolBlock, ToolPhase,
};
use serde::Deserialize;
use std::io::{self, Write};

use crate::cli::RunOutputFormat;
use crate::util::{parse_bool_env, parse_http_json, server_url};

#[derive(Debug, Deserialize)]
struct RemoteSessionInfo {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteConfigInfo {
    share: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteShareInfo {
    url: String,
}

fn parse_output_block(payload: &serde_json::Value) -> Option<OutputBlock> {
    let kind = payload.get("kind")?.as_str()?;
    match kind {
        "status" => {
            let tone = match payload
                .get("tone")
                .and_then(|v| v.as_str())
                .unwrap_or("normal")
            {
                "title" => BlockTone::Title,
                "muted" => BlockTone::Muted,
                "success" => BlockTone::Success,
                "warning" => BlockTone::Warning,
                "error" => BlockTone::Error,
                _ => BlockTone::Normal,
            };
            let text = payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(OutputBlock::Status(StatusBlock { tone, text }))
        }
        "message" => {
            let role = match payload
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("assistant")
            {
                "user" => MessageRole::User,
                "system" => MessageRole::System,
                _ => MessageRole::Assistant,
            };
            let phase = match payload
                .get("phase")
                .and_then(|v| v.as_str())
                .unwrap_or("delta")
            {
                "start" => MessagePhase::Start,
                "end" => MessagePhase::End,
                "full" => MessagePhase::Full,
                _ => MessagePhase::Delta,
            };
            let text = payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Some(OutputBlock::Message(MessageBlock { role, phase, text }))
        }
        "tool" => {
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let phase = match payload
                .get("phase")
                .and_then(|v| v.as_str())
                .unwrap_or("running")
            {
                "start" => ToolPhase::Start,
                "done" | "result" => ToolPhase::Done,
                "error" => ToolPhase::Error,
                _ => ToolPhase::Running,
            };
            let detail = payload
                .get("detail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(OutputBlock::Tool(ToolBlock {
                name,
                phase,
                detail,
                structured: None,
            }))
        }
        _ => None,
    }
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
    let config_endpoint = server_url(base_url, "/config");
    let config: RemoteConfigInfo =
        parse_http_json(client.get(config_endpoint).send().await?).await?;
    let config_auto = config.share.as_deref() == Some("auto");

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
    session_id: &str,
    format: RunOutputFormat,
) -> anyhow::Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut current_event: Option<String> = None;
    let mut current_data: Vec<String> = Vec::new();

    let dispatch_event = |event_name: Option<String>, data: String| -> anyhow::Result<()> {
        if data.trim().is_empty() {
            return Ok(());
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({ "raw": data }));
        let event_type = event_name
            .or_else(|| {
                parsed
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "message".to_string());

        if matches!(format, RunOutputFormat::Json) {
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
            if let Some(block) = parse_output_block(&parsed) {
                let style = CliStyle::detect();
                print!("{}", render_cli_block_rich(&block, &style));
                io::stdout().flush()?;
            }
            return Ok(());
        }

        match event_type.as_str() {
            "error" => {
                let message = parsed
                    .get("error")
                    .and_then(|v| v.as_str())
                    .or_else(|| parsed.get("message").and_then(|v| v.as_str()))
                    .unwrap_or("unknown remote stream error");
                eprintln!("\nError: {}", message);
            }
            _ => {}
        }
        Ok(())
    };

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
                dispatch_event(current_event.take(), data)?;
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
        dispatch_event(current_event.take(), current_data.join("\n"))?;
    }

    Ok(())
}

pub(crate) async fn run_non_interactive_attach(
    base_url: String,
    input: String,
    command: Option<String>,
    continue_last: bool,
    session: Option<String>,
    fork: bool,
    share: bool,
    model: Option<String>,
    scheduler_profile: Option<String>,
    variant: Option<String>,
    format: RunOutputFormat,
    title: Option<String>,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
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

    consume_remote_sse(response, &session_id, format).await
}
