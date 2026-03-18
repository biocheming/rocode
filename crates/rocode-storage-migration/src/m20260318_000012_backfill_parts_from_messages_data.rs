use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

use rocode_types::{MessagePart, PartType, ToolCallStatus};
use tracing::{info, warn};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260318_000012_backfill_parts_from_messages_data"
    }
}

fn part_type_to_str(part_type: &PartType) -> &'static str {
    match part_type {
        PartType::Text { .. } => "text",
        PartType::ToolCall { .. } => "tool_call",
        PartType::ToolResult { .. } => "tool_result",
        PartType::Reasoning { .. } => "reasoning",
        PartType::File { .. } => "file",
        PartType::StepStart { .. } => "step_start",
        PartType::StepFinish { .. } => "step_finish",
        PartType::Snapshot { .. } => "snapshot",
        PartType::Patch { .. } => "patch",
        PartType::Agent { .. } => "agent",
        PartType::Subtask { .. } => "subtask",
        PartType::Retry { .. } => "retry",
        PartType::Compaction { .. } => "compaction",
    }
}

fn tool_status_to_str(status: &ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::Running => "running",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Error => "error",
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = DbBackend::Sqlite;

        // Backfill the normalized `parts` table from historical `messages.data` JSON blobs.
        // This enables lazy-loading / querying message parts without fetching the full JSON
        // payload for every message.
        let select_stmt = Statement::from_sql_and_values(
            backend,
            "SELECT id, session_id, data FROM messages WHERE data IS NOT NULL".to_string(),
            vec![],
        );
        let rows = conn.query_all(select_stmt).await?;

        let mut message_rows = 0usize;
        let mut inserted = 0usize;
        let mut skipped_invalid = 0usize;

        for row in rows {
            message_rows += 1;
            let message_id: String = row.try_get("", "id")?;
            let session_id: String = row.try_get("", "session_id")?;
            let data: String = row.try_get("", "data")?;

            let parts: Vec<MessagePart> = match serde_json::from_str(&data) {
                Ok(parts) => parts,
                Err(error) => {
                    skipped_invalid += 1;
                    warn!(
                        message_id = %message_id,
                        %error,
                        "skipping parts backfill for message with invalid parts JSON"
                    );
                    continue;
                }
            };

            for (idx, part) in parts.iter().enumerate() {
                let created_at = part.created_at.timestamp_millis();
                let part_type = part_type_to_str(&part.part_type).to_string();
                let sort_order = idx as i64;
                let data_json =
                    serde_json::to_string(part).map_err(|e| DbErr::Custom(e.to_string()))?;

                let mut text: Option<String> = None;
                let mut tool_name: Option<String> = None;
                let mut tool_call_id: Option<String> = None;
                let mut tool_arguments: Option<String> = None;
                let mut tool_result: Option<String> = None;
                let mut tool_error: Option<String> = None;
                let mut tool_status: Option<String> = None;
                let mut file_url: Option<String> = None;
                let mut file_filename: Option<String> = None;
                let mut file_mime: Option<String> = None;
                let mut reasoning: Option<String> = None;

                match &part.part_type {
                    PartType::Text { text: value, .. } => text = Some(value.clone()),
                    PartType::ToolCall {
                        id,
                        name,
                        input,
                        status,
                        ..
                    } => {
                        tool_call_id = Some(id.clone());
                        tool_name = Some(name.clone());
                        tool_arguments = serde_json::to_string(input).ok();
                        tool_status = Some(tool_status_to_str(status).to_string());
                    }
                    PartType::ToolResult {
                        tool_call_id: call_id,
                        content,
                        is_error,
                        ..
                    } => {
                        tool_call_id = Some(call_id.clone());
                        tool_result = Some(content.clone());
                        if *is_error {
                            tool_error = Some(content.clone());
                            tool_status = Some("error".to_string());
                        } else {
                            tool_status = Some("completed".to_string());
                        }
                    }
                    PartType::Reasoning { text: value } => reasoning = Some(value.clone()),
                    PartType::File {
                        url,
                        filename,
                        mime,
                    } => {
                        file_url = Some(url.clone());
                        file_filename = Some(filename.clone());
                        file_mime = Some(mime.clone());
                    }
                    _ => {}
                }

                let insert_stmt = Statement::from_sql_and_values(
                    backend,
                    "INSERT OR IGNORE INTO parts (id, message_id, session_id, created_at, part_type, text, tool_name, tool_call_id, tool_arguments, tool_result, tool_error, tool_status, file_url, file_filename, file_mime, reasoning, sort_order, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                        .to_string(),
                    vec![
                        part.id.clone().into(),
                        message_id.clone().into(),
                        session_id.clone().into(),
                        created_at.into(),
                        part_type.into(),
                        text.into(),
                        tool_name.into(),
                        tool_call_id.into(),
                        tool_arguments.into(),
                        tool_result.into(),
                        tool_error.into(),
                        tool_status.into(),
                        file_url.into(),
                        file_filename.into(),
                        file_mime.into(),
                        reasoning.into(),
                        sort_order.into(),
                        data_json.into(),
                    ],
                );

                conn.execute(insert_stmt).await?;
                inserted += 1;
            }
        }

        info!(
            message_rows,
            inserted_parts_attempted = inserted,
            skipped_invalid,
            "parts backfill migration complete"
        );

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
