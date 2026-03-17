use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

use rocode_types::{MessagePart, PartType};
use serde_json::Value;
use tracing::{info, warn};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000009_migrate_tool_call_input_data"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = DbBackend::Sqlite;

        // `messages.data` stores a JSON array of parts. Historical clients could store
        // unrecoverable tool args sentinels; this migration sanitizes them into a
        // stable payload so downstream code can rely on the shape.
        let select_stmt = Statement::from_sql_and_values(
            backend,
            "SELECT id, data FROM messages WHERE role = 'assistant' AND data IS NOT NULL"
                .to_string(),
            vec![],
        );
        let rows = conn.query_all(select_stmt).await?;

        let mut updated_rows = 0usize;
        let mut recovered_inputs = 0usize;
        let mut invalid_reroutes = 0usize;

        for row in rows {
            let id: String = row.try_get("", "id")?;
            let data: String = row.try_get("", "data")?;
            let mut parts: Vec<MessagePart> = match serde_json::from_str(&data) {
                Ok(parts) => parts,
                Err(error) => {
                    warn!(
                        message_id = %id,
                        %error,
                        "skipping tool-call input migration for message with invalid parts JSON"
                    );
                    continue;
                }
            };

            let mut changed = false;
            for part in &mut parts {
                if let PartType::ToolCall { name, input, .. } = &mut part.part_type {
                    let (sanitized, was_recovered, rerouted_invalid) =
                        sanitize_tool_call_input_for_storage(name, input);
                    if *input != sanitized {
                        *input = sanitized;
                        changed = true;
                    }
                    if was_recovered {
                        recovered_inputs += 1;
                    }
                    if rerouted_invalid {
                        invalid_reroutes += 1;
                    }
                }
            }

            if !changed {
                continue;
            }

            let next_data = serde_json::to_string(&parts)
                .map_err(|e| DbErr::Custom(e.to_string()))?;
            let update_stmt = Statement::from_sql_and_values(
                backend,
                "UPDATE messages SET data = ? WHERE id = ?".to_string(),
                vec![next_data.into(), id.clone().into()],
            );
            conn.execute(update_stmt).await?;
            updated_rows += 1;
        }

        if updated_rows > 0 || recovered_inputs > 0 || invalid_reroutes > 0 {
            info!(
                updated_rows,
                recovered_inputs,
                invalid_reroutes,
                "tool call input data migration complete"
            );
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

fn invalid_tool_payload_for_storage(tool_name: &str, error: &str, received_args: Value) -> Value {
    serde_json::json!({
        "tool": tool_name,
        "error": error,
        "receivedArgs": received_args,
        "source": "storage-migration",
    })
}

fn sanitize_tool_call_input_for_storage(tool_name: &str, input: &Value) -> (Value, bool, bool) {
    if let Some(obj) = input.as_object() {
        let is_legacy_unrecoverable = obj
            .get("_rocode_unrecoverable_tool_args")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_legacy_unrecoverable {
            return (input.clone(), false, false);
        }

        let received_args = serde_json::json!({
            "type": "object",
            "source": "legacy-unrecoverable-sentinel",
            "raw_len": obj.get("raw_len").and_then(Value::as_u64),
            "preview": obj.get("raw_preview").and_then(Value::as_str),
        });
        return (
            invalid_tool_payload_for_storage(
                tool_name,
                "Stored tool arguments were previously marked unrecoverable.",
                received_args,
            ),
            false,
            true,
        );
    }

    if let Some(text) = input.as_str() {
        // The legacy store could truncate tool arguments and keep a short preview.
        // Attempt recovery when the preview looks like a valid JSON object.
        if let Some(preview) = text.strip_prefix("{") {
            let candidate = format!("{{{}", preview);
            if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
                return (value, true, false);
            }
        }
    }

    (
        invalid_tool_payload_for_storage(
            tool_name,
            "Unsupported tool arguments shape; value could not be recovered.",
            input.clone(),
        ),
        false,
        true,
    )
}

