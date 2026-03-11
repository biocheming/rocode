use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use rocode_storage::{Database, MessageRepository, SessionRepository};
use rocode_types::{MessagePart, Session, SessionMessage};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SessionExportEntry {
    info: Session,
    messages: Vec<SessionMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionExportFile {
    version: String,
    exported_at: i64,
    sessions: Vec<SessionExportEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum SessionImportPayload {
    Wrapped(SessionExportFile),
    Single(SessionExportEntry),
    Legacy {
        info: Session,
        messages: Vec<LegacyMessageExport>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct LegacyMessageExport {
    info: SessionMessage,
    #[serde(default)]
    parts: Vec<MessagePart>,
}

pub(crate) async fn export_session_data(
    session_id: Option<String>,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let db = Database::new().await?;
    let session_repo = SessionRepository::new(db.pool().clone());
    let message_repo = MessageRepository::new(db.pool().clone());

    let session = if let Some(session_id) = session_id {
        session_repo
            .get(&session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?
    } else {
        session_repo
            .list(None, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No sessions found to export"))?
    };

    let messages = message_repo.list_for_session(&session.id).await?;
    let export = SessionExportFile {
        version: "rocode-rust/v1".to_string(),
        exported_at: chrono::Utc::now().timestamp_millis(),
        sessions: vec![SessionExportEntry {
            info: session,
            messages,
        }],
    };

    let json = serde_json::to_string_pretty(&export)?;
    match output {
        Some(path) => {
            fs::write(&path, json)?;
            println!("Exported session data to {}", path.display());
        }
        None => {
            println!("{}", json);
        }
    }

    Ok(())
}

fn normalize_import_payload(payload: SessionImportPayload) -> Vec<SessionExportEntry> {
    match payload {
        SessionImportPayload::Wrapped(file) => file.sessions,
        SessionImportPayload::Single(entry) => vec![entry],
        SessionImportPayload::Legacy { info, messages } => {
            let normalized_messages = messages
                .into_iter()
                .map(|legacy| {
                    let mut msg = legacy.info;
                    if msg.parts.is_empty() {
                        msg.parts = legacy.parts;
                    }
                    msg
                })
                .collect();
            vec![SessionExportEntry {
                info,
                messages: normalized_messages,
            }]
        }
    }
}

fn parse_share_slug(url: &str) -> Option<String> {
    let trimmed = url.trim_end_matches('/');
    if let Some(idx) = trimmed.rfind("/share/") {
        return Some(trimmed[idx + 7..].to_string());
    }
    if let Some(idx) = trimmed.rfind("/s/") {
        return Some(trimmed[idx + 3..].to_string());
    }
    None
}

pub(crate) async fn import_session_data(file_or_url: String) -> anyhow::Result<()> {
    let raw = if file_or_url.starts_with("http://") || file_or_url.starts_with("https://") {
        let client = reqwest::Client::new();
        let mut text = client.get(&file_or_url).send().await?.text().await?;

        if let Some(slug) = parse_share_slug(&file_or_url) {
            if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                let share_api = format!("https://opencode.ai/api/share/{}/data", slug);
                text = client.get(share_api).send().await?.text().await?;
            }
        }
        text
    } else {
        fs::read_to_string(&file_or_url)?
    };
    let payload: SessionImportPayload = serde_json::from_str(&raw)?;
    let entries = normalize_import_payload(payload);

    if entries.is_empty() {
        anyhow::bail!("No session entries found in {}", file_or_url);
    }

    let db = Database::new().await?;
    let session_repo = SessionRepository::new(db.pool().clone());
    let message_repo = MessageRepository::new(db.pool().clone());

    let mut imported = 0usize;
    for mut entry in entries {
        entry.info.messages.clear();

        if session_repo.get(&entry.info.id).await?.is_some() {
            session_repo.update(&entry.info).await?;
        } else {
            session_repo.create(&entry.info).await?;
        }

        for mut message in entry.messages {
            if message.session_id.is_empty() {
                message.session_id = entry.info.id.clone();
            }
            message_repo.upsert(&message).await?;
        }
        imported += 1;
    }

    println!("Imported {} session(s) from {}", imported, file_or_url);
    Ok(())
}
