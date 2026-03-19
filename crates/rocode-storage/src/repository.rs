use chrono::{DateTime, Utc};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use rocode_session::{
    MessagePart, MessageUsage, PartType, Role, Session, SessionMessage, SessionSummary,
    SessionTime, SessionUsage, ToolCallStatus,
};

use crate::database::DatabaseError;
use crate::entities::{messages, parts, session_shares, sessions, todos};
use crate::StorageConnection;

fn map_query_err(err: sea_orm::DbErr) -> DatabaseError {
    DatabaseError::QueryError(err.to_string())
}

fn map_tx_err(err: sea_orm::DbErr) -> DatabaseError {
    DatabaseError::TransactionError(err.to_string())
}

fn normalize_limit_offset(limit: i64, offset: i64) -> Result<(u64, u64), DatabaseError> {
    if limit < 0 {
        return Err(DatabaseError::QueryError(format!(
            "limit must be >= 0, got {}",
            limit
        )));
    }
    if offset < 0 {
        return Err(DatabaseError::QueryError(format!(
            "offset must be >= 0, got {}",
            offset
        )));
    }
    Ok((limit as u64, offset as u64))
}

fn parse_int_id(value: &str, _field: &str) -> Result<i64, DatabaseError> {
    if let Ok(id) = value.parse::<i64>() {
        return Ok(id);
    }

    let digits: String = value.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if !digits.is_empty() {
        if let Ok(id) = digits.parse::<i64>() {
            return Ok(id);
        }
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    Ok((hasher.finish() & i64::MAX as u64) as i64)
}

fn parse_opt_int_id(value: &Option<String>, field: &str) -> Result<Option<i64>, DatabaseError> {
    match value {
        Some(v) => Ok(Some(parse_int_id(v, field)?)),
        None => Ok(None),
    }
}

fn role_to_model(role: Role) -> messages::MessageRoleModel {
    match role {
        Role::User => messages::MessageRoleModel::User,
        Role::Assistant => messages::MessageRoleModel::Assistant,
        Role::System => messages::MessageRoleModel::System,
        Role::Tool => messages::MessageRoleModel::Tool,
    }
}

fn role_from_model(role: messages::MessageRoleModel) -> Role {
    match role {
        messages::MessageRoleModel::User => Role::User,
        messages::MessageRoleModel::Assistant => Role::Assistant,
        messages::MessageRoleModel::System => Role::System,
        messages::MessageRoleModel::Tool => Role::Tool,
    }
}

fn session_insert_model(session: &Session) -> Result<sessions::ActiveModel, DatabaseError> {
    let summary_diffs = session
        .summary
        .as_ref()
        .and_then(|s| serde_json::to_string(&s.diffs).ok());
    let revert_json = session
        .revert
        .as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let permission_json = session
        .permission
        .as_ref()
        .and_then(|p| serde_json::to_string(p).ok());
    let metadata_json = serde_json::to_string(&session.metadata).ok();

    let usage = session.usage.as_ref();

    Ok(sessions::ActiveModel {
        id: Set(parse_int_id(&session.id, "session.id")?),
        parent_id: Set(parse_opt_int_id(&session.parent_id, "session.parent_id")?),
        directory: Set(session.directory.clone()),
        title: Set(session.title.clone()),
        version: Set(session.version.clone()),
        share_url: Set(session.share.clone()),
        summary_additions: Set(session
            .summary
            .as_ref()
            .map(|s| s.additions as i64)
            .unwrap_or(0)),
        summary_deletions: Set(session
            .summary
            .as_ref()
            .map(|s| s.deletions as i64)
            .unwrap_or(0)),
        summary_files: Set(session
            .summary
            .as_ref()
            .map(|s| s.files as i64)
            .unwrap_or(0)),
        summary_diffs: Set(summary_diffs),
        revert: Set(revert_json),
        permission: Set(permission_json),
        metadata: Set(metadata_json),
        usage_input_tokens: Set(usage.map(|u| u.input_tokens as i64).unwrap_or(0)),
        usage_output_tokens: Set(usage.map(|u| u.output_tokens as i64).unwrap_or(0)),
        usage_reasoning_tokens: Set(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0)),
        usage_cache_write_tokens: Set(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0)),
        usage_cache_read_tokens: Set(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0)),
        usage_total_cost: Set(usage.map(|u| u.total_cost).unwrap_or(0.0)),
        status: Set(session.active),
        created_at: Set(session.time.created),
        updated_at: Set(session.time.updated),
    })
}

fn session_update_model(session: &Session) -> Result<sessions::ActiveModel, DatabaseError> {
    let summary_diffs = session
        .summary
        .as_ref()
        .and_then(|s| serde_json::to_string(&s.diffs).ok());
    let revert_json = session
        .revert
        .as_ref()
        .and_then(|r| serde_json::to_string(r).ok());
    let permission_json = session
        .permission
        .as_ref()
        .and_then(|p| serde_json::to_string(p).ok());
    let metadata_json = serde_json::to_string(&session.metadata).ok();

    let usage = session.usage.as_ref();

    Ok(sessions::ActiveModel {
        id: Set(parse_int_id(&session.id, "session.id")?),
        title: Set(session.title.clone()),
        version: Set(session.version.clone()),
        share_url: Set(session.share.clone()),
        summary_additions: Set(session
            .summary
            .as_ref()
            .map(|s| s.additions as i64)
            .unwrap_or(0)),
        summary_deletions: Set(session
            .summary
            .as_ref()
            .map(|s| s.deletions as i64)
            .unwrap_or(0)),
        summary_files: Set(session
            .summary
            .as_ref()
            .map(|s| s.files as i64)
            .unwrap_or(0)),
        summary_diffs: Set(summary_diffs),
        revert: Set(revert_json),
        permission: Set(permission_json),
        metadata: Set(metadata_json),
        usage_input_tokens: Set(usage.map(|u| u.input_tokens as i64).unwrap_or(0)),
        usage_output_tokens: Set(usage.map(|u| u.output_tokens as i64).unwrap_or(0)),
        usage_reasoning_tokens: Set(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0)),
        usage_cache_write_tokens: Set(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0)),
        usage_cache_read_tokens: Set(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0)),
        usage_total_cost: Set(usage.map(|u| u.total_cost).unwrap_or(0.0)),
        status: Set(session.active),
        updated_at: Set(session.time.updated),
        ..Default::default()
    })
}

fn session_from_model(model: sessions::Model) -> Session {
    let summary_present = model.summary_additions != 0
        || model.summary_deletions != 0
        || model.summary_files != 0
        || model.summary_diffs.is_some();
    let summary = summary_present.then(|| SessionSummary {
        additions: model.summary_additions as u64,
        deletions: model.summary_deletions as u64,
        files: model.summary_files as u64,
        diffs: model
            .summary_diffs
            .as_deref()
            .and_then(|d| serde_json::from_str(d).ok()),
    });

    let usage_present = model.usage_input_tokens != 0
        || model.usage_output_tokens != 0
        || model.usage_reasoning_tokens != 0
        || model.usage_cache_write_tokens != 0
        || model.usage_cache_read_tokens != 0
        || model.usage_total_cost != 0.0;
    let usage = usage_present.then(|| SessionUsage {
        input_tokens: model.usage_input_tokens as u64,
        output_tokens: model.usage_output_tokens as u64,
        reasoning_tokens: model.usage_reasoning_tokens as u64,
        cache_write_tokens: model.usage_cache_write_tokens as u64,
        cache_read_tokens: model.usage_cache_read_tokens as u64,
        total_cost: model.usage_total_cost,
    });

    Session {
        id: model.id.to_string(),
        directory: model.directory,
        parent_id: model.parent_id.map(|id| id.to_string()),
        title: model.title,
        version: model.version,
        time: SessionTime {
            created: model.created_at,
            updated: model.updated_at,
        },
        messages: vec![],
        summary,
        share: model.share_url,
        revert: model.revert.and_then(|r| serde_json::from_str(&r).ok()),
        permission: model.permission.and_then(|p| serde_json::from_str(&p).ok()),
        metadata: model
            .metadata
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or_default(),
        usage,
        active: model.status,
        cached_at: Utc::now(),
    }
}

fn message_insert_model(message: &SessionMessage) -> Result<messages::ActiveModel, DatabaseError> {
    let data_json = serde_json::to_string(&message.parts)
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
    let metadata_json = serde_json::to_string(&message.metadata)
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

    let usage = message.usage.as_ref();

    Ok(messages::ActiveModel {
        id: Set(parse_int_id(&message.id, "message.id")?),
        session_id: Set(parse_int_id(&message.session_id, "message.session_id")?),
        role: Set(role_to_model(message.role)),
        created_at: Set(message.created_at.timestamp_millis()),
        tokens_input: Set(usage.map(|u| u.input_tokens as i64).unwrap_or(0)),
        tokens_output: Set(usage.map(|u| u.output_tokens as i64).unwrap_or(0)),
        tokens_reasoning: Set(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0)),
        tokens_cache_read: Set(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0)),
        tokens_cache_write: Set(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0)),
        cost: Set(usage.map(|u| u.total_cost).unwrap_or(0.0)),
        finish: Set(message.finish.clone()),
        metadata: Set(Some(metadata_json)),
        data: Set(Some(data_json)),
        ..Default::default()
    })
}

fn message_from_model(model: messages::Model) -> Option<SessionMessage> {
    let msg_role = role_from_model(model.role);

    let parts: Vec<MessagePart> = model
        .data
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    let created = DateTime::from_timestamp_millis(model.created_at).unwrap_or_else(Utc::now);

    let usage_present = model.tokens_input != 0
        || model.tokens_output != 0
        || model.tokens_reasoning != 0
        || model.tokens_cache_read != 0
        || model.tokens_cache_write != 0
        || model.cost != 0.0;
    let usage = usage_present.then(|| MessageUsage {
        input_tokens: model.tokens_input as u64,
        output_tokens: model.tokens_output as u64,
        reasoning_tokens: model.tokens_reasoning as u64,
        cache_write_tokens: model.tokens_cache_write as u64,
        cache_read_tokens: model.tokens_cache_read as u64,
        total_cost: model.cost,
    });

    Some(SessionMessage {
        id: model.id.to_string(),
        session_id: model.session_id.to_string(),
        role: msg_role,
        parts,
        created_at: created,
        metadata: model
            .metadata
            .and_then(|m| serde_json::from_str(&m).ok())
            .unwrap_or_default(),
        usage,
        finish: model.finish,
    })
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

fn part_insert_model(
    session_id: &str,
    message_id: &str,
    sort_order: i64,
    part: &MessagePart,
) -> Result<parts::ActiveModel, DatabaseError> {
    let created_at = part.created_at.timestamp_millis();
    let data_json = serde_json::to_string(part)
        .map(Some)
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

    let mut active = parts::ActiveModel {
        id: Set(parse_int_id(&part.id, "part.id")?),
        message_id: Set(parse_int_id(message_id, "part.message_id")?),
        session_id: Set(parse_int_id(session_id, "part.session_id")?),
        created_at: Set(created_at),
        part_type: Set(part_type_to_str(&part.part_type).to_string()),
        sort_order: Set(sort_order),
        data: Set(data_json),
        ..Default::default()
    };

    match &part.part_type {
        PartType::Text { text, .. } => {
            active.text = Set(Some(text.clone()));
        }
        PartType::ToolCall {
            id,
            name,
            input,
            status,
            ..
        } => {
            active.tool_name = Set(Some(name.clone()));
            active.tool_call_id = Set(Some(id.clone()));
            active.tool_arguments = Set(serde_json::to_string(input).ok());
            active.tool_status = Set(Some(tool_status_to_str(status).to_string()));
        }
        PartType::ToolResult {
            tool_call_id,
            content,
            is_error,
            ..
        } => {
            active.tool_call_id = Set(Some(tool_call_id.clone()));
            active.tool_result = Set(Some(content.clone()));
            active.tool_error = Set(is_error.then(|| content.clone()));
            active.tool_status = Set(Some(if *is_error { "error" } else { "completed" }.into()));
        }
        PartType::Reasoning { text } => {
            active.reasoning = Set(Some(text.clone()));
        }
        PartType::File {
            url,
            filename,
            mime,
        } => {
            active.file_url = Set(Some(url.clone()));
            active.file_filename = Set(Some(filename.clone()));
            active.file_mime = Set(Some(mime.clone()));
        }
        _ => {}
    }

    Ok(active)
}

#[derive(Clone)]
pub struct SessionRepository {
    conn: StorageConnection,
}

impl SessionRepository {
    pub fn new(conn: StorageConnection) -> Self {
        Self { conn }
    }

    pub async fn create(&self, session: &Session) -> Result<(), DatabaseError> {
        sessions::Entity::insert(session_insert_model(session)?)
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<Session>, DatabaseError> {
        let row = sessions::Entity::find()
            .filter(sessions::Column::Id.eq(parse_int_id(id, "session.id")?))
            .one(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(row.map(session_from_model))
    }

    pub async fn list(
        &self,
        directory: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Session>, DatabaseError> {
        let (limit, _offset) = normalize_limit_offset(limit, 0)?;
        let mut query = sessions::Entity::find();
        if let Some(dir) = directory {
            query = query.filter(sessions::Column::Directory.eq(dir));
        }
        let rows = query
            .order_by_desc(sessions::Column::UpdatedAt)
            .limit(limit)
            .offset(0)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(rows.into_iter().map(session_from_model).collect())
    }

    pub async fn count(&self, directory: Option<&str>) -> Result<u64, DatabaseError> {
        let mut query = sessions::Entity::find();
        if let Some(dir) = directory {
            query = query.filter(sessions::Column::Directory.eq(dir));
        }
        query.count(&self.conn).await.map_err(map_query_err)
    }

    pub async fn list_page(
        &self,
        directory: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Session>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let mut query = sessions::Entity::find();
        if let Some(dir) = directory {
            query = query.filter(sessions::Column::Directory.eq(dir));
        }
        let rows = query
            .order_by_desc(sessions::Column::UpdatedAt)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(rows.into_iter().map(session_from_model).collect())
    }

    pub async fn count_for_directory(&self, directory: &str) -> Result<u64, DatabaseError> {
        sessions::Entity::find()
            .filter(sessions::Column::Directory.eq(directory))
            .count(&self.conn)
            .await
            .map_err(map_query_err)
    }

    pub async fn list_for_directory_page(
        &self,
        directory: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Session>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows = sessions::Entity::find()
            .filter(sessions::Column::Directory.eq(directory))
            .order_by_desc(sessions::Column::UpdatedAt)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(rows.into_iter().map(session_from_model).collect())
    }

    pub async fn update(&self, session: &Session) -> Result<(), DatabaseError> {
        session_update_model(session)?
            .update(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn upsert(&self, session: &Session) -> Result<(), DatabaseError> {
        sessions::Entity::insert(session_insert_model(session)?)
            .on_conflict(
                OnConflict::column(sessions::Column::Id)
                    .update_columns([
                        sessions::Column::Title,
                        sessions::Column::Version,
                        sessions::Column::ShareUrl,
                        sessions::Column::SummaryAdditions,
                        sessions::Column::SummaryDeletions,
                        sessions::Column::SummaryFiles,
                        sessions::Column::SummaryDiffs,
                        sessions::Column::Revert,
                        sessions::Column::Permission,
                        sessions::Column::Metadata,
                        sessions::Column::UsageInputTokens,
                        sessions::Column::UsageOutputTokens,
                        sessions::Column::UsageReasoningTokens,
                        sessions::Column::UsageCacheWriteTokens,
                        sessions::Column::UsageCacheReadTokens,
                        sessions::Column::UsageTotalCost,
                        sessions::Column::Status,
                        sessions::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        sessions::Entity::delete_many()
            .filter(sessions::Column::Id.eq(parse_int_id(id, "session.id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn list_children(&self, parent_id: &str) -> Result<Vec<Session>, DatabaseError> {
        let rows = sessions::Entity::find()
            .filter(sessions::Column::ParentId.eq(parse_int_id(parent_id, "session.parent_id")?))
            .order_by_desc(sessions::Column::CreatedAt)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(rows.into_iter().map(session_from_model).collect())
    }

    async fn upsert_session_in_tx(
        &self,
        tx: &DatabaseTransaction,
        session: &Session,
    ) -> Result<(), DatabaseError> {
        sessions::Entity::insert(session_insert_model(session)?)
            .on_conflict(
                OnConflict::column(sessions::Column::Id)
                    .update_columns([
                        sessions::Column::Title,
                        sessions::Column::Version,
                        sessions::Column::ShareUrl,
                        sessions::Column::SummaryAdditions,
                        sessions::Column::SummaryDeletions,
                        sessions::Column::SummaryFiles,
                        sessions::Column::SummaryDiffs,
                        sessions::Column::Revert,
                        sessions::Column::Permission,
                        sessions::Column::Metadata,
                        sessions::Column::UsageInputTokens,
                        sessions::Column::UsageOutputTokens,
                        sessions::Column::UsageReasoningTokens,
                        sessions::Column::UsageCacheWriteTokens,
                        sessions::Column::UsageCacheReadTokens,
                        sessions::Column::UsageTotalCost,
                        sessions::Column::Status,
                        sessions::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(tx)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    async fn upsert_messages_in_tx(
        &self,
        tx: &DatabaseTransaction,
        messages_to_upsert: &[SessionMessage],
    ) -> Result<(), DatabaseError> {
        for msg in messages_to_upsert {
            messages::Entity::insert(message_insert_model(msg)?)
                .on_conflict(
                    OnConflict::column(messages::Column::Id)
                        .update_columns([
                            messages::Column::SessionId,
                            messages::Column::Role,
                            messages::Column::CreatedAt,
                            messages::Column::TokensInput,
                            messages::Column::TokensOutput,
                            messages::Column::TokensReasoning,
                            messages::Column::TokensCacheRead,
                            messages::Column::TokensCacheWrite,
                            messages::Column::Cost,
                            messages::Column::Finish,
                            messages::Column::Metadata,
                            messages::Column::Data,
                        ])
                        .to_owned(),
                )
                .exec(tx)
                .await
                .map_err(map_query_err)?;
        }
        Ok(())
    }

    async fn upsert_parts_in_tx(
        &self,
        tx: &DatabaseTransaction,
        messages_to_upsert: &[SessionMessage],
    ) -> Result<(), DatabaseError> {
        for msg in messages_to_upsert {
            for (idx, part) in msg.parts.iter().enumerate() {
                parts::Entity::insert(part_insert_model(
                    msg.session_id.as_str(),
                    msg.id.as_str(),
                    idx as i64,
                    part,
                )?)
                .on_conflict(
                    OnConflict::column(parts::Column::Id)
                        .update_columns([
                            parts::Column::MessageId,
                            parts::Column::SessionId,
                            parts::Column::CreatedAt,
                            parts::Column::PartType,
                            parts::Column::Text,
                            parts::Column::ToolName,
                            parts::Column::ToolCallId,
                            parts::Column::ToolArguments,
                            parts::Column::ToolResult,
                            parts::Column::ToolError,
                            parts::Column::ToolStatus,
                            parts::Column::FileUrl,
                            parts::Column::FileFilename,
                            parts::Column::FileMime,
                            parts::Column::Reasoning,
                            parts::Column::SortOrder,
                            parts::Column::Data,
                        ])
                        .to_owned(),
                )
                .exec(tx)
                .await
                .map_err(map_query_err)?;
            }
        }
        Ok(())
    }

    async fn delete_stale_parts_for_message_in_tx(
        &self,
        tx: &DatabaseTransaction,
        message_id: &str,
        keep_ids: &HashSet<String>,
    ) -> Result<(), DatabaseError> {
        let message_id_i64 = parse_int_id(message_id, "message.id")?;
        let keep_ids_i64: HashSet<i64> = keep_ids
            .iter()
            .filter_map(|id| parse_int_id(id, "part.id").ok())
            .collect();
        if keep_ids.is_empty() {
            parts::Entity::delete_many()
                .filter(parts::Column::MessageId.eq(message_id_i64))
                .exec(tx)
                .await
                .map_err(map_query_err)?;
            return Ok(());
        }

        let existing_ids: Vec<i64> = parts::Entity::find()
            .filter(parts::Column::MessageId.eq(message_id_i64))
            .select_only()
            .column(parts::Column::Id)
            .into_tuple()
            .all(tx)
            .await
            .map_err(map_query_err)?;

        let stale: Vec<i64> = existing_ids
            .into_iter()
            .filter(|id| !keep_ids_i64.contains(id))
            .collect();

        for chunk in stale.chunks(500) {
            parts::Entity::delete_many()
                .filter(parts::Column::Id.is_in(chunk.to_vec()))
                .exec(tx)
                .await
                .map_err(map_query_err)?;
        }

        Ok(())
    }

    async fn delete_stale_messages_in_tx(
        &self,
        tx: &DatabaseTransaction,
        session_id: &str,
        keep_ids: &HashSet<String>,
    ) -> Result<(), DatabaseError> {
        let session_id_i64 = parse_int_id(session_id, "session.id")?;
        let keep_ids_i64: HashSet<i64> = keep_ids
            .iter()
            .filter_map(|id| parse_int_id(id, "message.id").ok())
            .collect();

        let existing_ids: Vec<i64> = messages::Entity::find()
            .filter(messages::Column::SessionId.eq(session_id_i64))
            .select_only()
            .column(messages::Column::Id)
            .into_tuple()
            .all(tx)
            .await
            .map_err(map_query_err)?;

        let stale: Vec<i64> = existing_ids
            .into_iter()
            .filter(|id| !keep_ids_i64.contains(id))
            .collect();

        for chunk in stale.chunks(500) {
            messages::Entity::delete_many()
                .filter(messages::Column::Id.is_in(chunk.to_vec()))
                .exec(tx)
                .await
                .map_err(map_query_err)?;
        }

        Ok(())
    }

    /// Atomically upsert a session, upsert its messages, and delete stale messages
    /// that no longer exist in the session layer (e.g. after revert/delete).
    pub async fn flush_with_messages(
        &self,
        session: &Session,
        messages_to_flush: &[SessionMessage],
    ) -> Result<(), DatabaseError> {
        let tx = self.conn.begin().await.map_err(map_tx_err)?;

        let keep_ids: HashSet<String> = messages_to_flush.iter().map(|m| m.id.clone()).collect();

        let result = async {
            self.upsert_session_in_tx(&tx, session).await?;
            self.upsert_messages_in_tx(&tx, messages_to_flush).await?;
            self.upsert_parts_in_tx(&tx, messages_to_flush).await?;
            self.delete_stale_messages_in_tx(&tx, &session.id, &keep_ids)
                .await?;
            for msg in messages_to_flush {
                let keep_parts: HashSet<String> = msg.parts.iter().map(|p| p.id.clone()).collect();
                self.delete_stale_parts_for_message_in_tx(&tx, &msg.id, &keep_parts)
                    .await?;
            }
            Ok::<(), DatabaseError>(())
        }
        .await;

        match result {
            Ok(()) => tx.commit().await.map_err(map_tx_err),
            Err(err) => {
                let _ = tx.rollback().await;
                Err(err)
            }
        }
    }
}

#[derive(Clone)]
pub struct MessageRepository {
    conn: StorageConnection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHeaderRow {
    pub id: String,
    pub session_id: String,
    pub role: Role,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
}

impl MessageRepository {
    pub fn new(conn: StorageConnection) -> Self {
        Self { conn }
    }

    pub async fn create(&self, message: &SessionMessage) -> Result<(), DatabaseError> {
        messages::Entity::insert(message_insert_model(message)?)
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn upsert(&self, message: &SessionMessage) -> Result<(), DatabaseError> {
        messages::Entity::insert(message_insert_model(message)?)
            .on_conflict(
                OnConflict::column(messages::Column::Id)
                    .update_columns([
                        messages::Column::SessionId,
                        messages::Column::Role,
                        messages::Column::CreatedAt,
                        messages::Column::TokensInput,
                        messages::Column::TokensOutput,
                        messages::Column::TokensReasoning,
                        messages::Column::TokensCacheRead,
                        messages::Column::TokensCacheWrite,
                        messages::Column::Cost,
                        messages::Column::Finish,
                        messages::Column::Metadata,
                        messages::Column::Data,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn list_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessage>, DatabaseError> {
        let rows = messages::Entity::find()
            .filter(messages::Column::SessionId.eq(parse_int_id(session_id, "message.session_id")?))
            .order_by_asc(messages::Column::CreatedAt)
            .order_by_asc(messages::Column::Id)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows.into_iter().filter_map(message_from_model).collect())
    }

    pub async fn count_for_session(&self, session_id: &str) -> Result<u64, DatabaseError> {
        messages::Entity::find()
            .filter(messages::Column::SessionId.eq(parse_int_id(session_id, "message.session_id")?))
            .count(&self.conn)
            .await
            .map_err(map_query_err)
    }

    pub async fn list_headers_for_session_page(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MessageHeaderRow>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows: Vec<(i64, i64, messages::MessageRoleModel, i64, Option<String>)> =
            messages::Entity::find()
                .filter(
                    messages::Column::SessionId.eq(parse_int_id(session_id, "message.session_id")?),
                )
                .select_only()
                .column(messages::Column::Id)
                .column(messages::Column::SessionId)
                .column(messages::Column::Role)
                .column(messages::Column::CreatedAt)
                .column(messages::Column::Finish)
                .order_by_asc(messages::Column::CreatedAt)
                .order_by_asc(messages::Column::Id)
                .limit(limit)
                .offset(offset)
                .into_tuple()
                .all(&self.conn)
                .await
                .map_err(map_query_err)?;

        rows.into_iter()
            .map(|(id, session_id, role, created_at, finish)| {
                Ok(MessageHeaderRow {
                    id: id.to_string(),
                    session_id: session_id.to_string(),
                    role: role_from_model(role),
                    created_at,
                    finish,
                })
            })
            .collect()
    }

    pub async fn list_for_session_page(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionMessage>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows = messages::Entity::find()
            .filter(messages::Column::SessionId.eq(parse_int_id(session_id, "message.session_id")?))
            .order_by_asc(messages::Column::CreatedAt)
            .order_by_asc(messages::Column::Id)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows.into_iter().filter_map(message_from_model).collect())
    }

    pub async fn get(&self, id: &str) -> Result<Option<SessionMessage>, DatabaseError> {
        let row = messages::Entity::find()
            .filter(messages::Column::Id.eq(parse_int_id(id, "message.id")?))
            .one(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(row.and_then(message_from_model))
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        messages::Entity::delete_many()
            .filter(messages::Column::Id.eq(parse_int_id(id, "message.id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        messages::Entity::delete_many()
            .filter(messages::Column::SessionId.eq(parse_int_id(session_id, "message.session_id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub position: i64,
}

#[derive(Clone)]
pub struct TodoRepository {
    conn: StorageConnection,
}

impl TodoRepository {
    pub fn new(conn: StorageConnection) -> Self {
        Self { conn }
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<TodoItem>, DatabaseError> {
        let rows = todos::Entity::find()
            .filter(todos::Column::SessionId.eq(parse_int_id(session_id, "todo.session_id")?))
            .order_by_asc(todos::Column::Position)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|row| TodoItem {
                id: row.id.to_string(),
                content: row.content,
                status: row.status,
                priority: row.priority,
                position: row.position,
            })
            .collect())
    }

    pub async fn count_for_session(&self, session_id: &str) -> Result<u64, DatabaseError> {
        todos::Entity::find()
            .filter(todos::Column::SessionId.eq(parse_int_id(session_id, "todo.session_id")?))
            .count(&self.conn)
            .await
            .map_err(map_query_err)
    }

    pub async fn list_for_session_page(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TodoItem>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows = todos::Entity::find()
            .filter(todos::Column::SessionId.eq(parse_int_id(session_id, "todo.session_id")?))
            .order_by_asc(todos::Column::Position)
            .order_by_asc(todos::Column::Id)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|row| TodoItem {
                id: row.id.to_string(),
                content: row.content,
                status: row.status,
                priority: row.priority,
                position: row.position,
            })
            .collect())
    }

    pub async fn upsert(&self, session_id: &str, todo: &TodoItem) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp_millis();
        let insert = todos::ActiveModel {
            id: Set(parse_int_id(&todo.id, "todo.id")?),
            session_id: Set(parse_int_id(session_id, "todo.session_id")?),
            content: Set(todo.content.clone()),
            status: Set(todo.status.clone()),
            priority: Set(todo.priority.clone()),
            position: Set(todo.position),
            created_at: Set(now),
            updated_at: Set(now),
        };

        todos::Entity::insert(insert)
            .on_conflict(
                OnConflict::column(todos::Column::Id)
                    .update_columns([
                        todos::Column::Content,
                        todos::Column::Status,
                        todos::Column::Priority,
                        todos::Column::Position,
                        todos::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete(&self, session_id: &str, todo_id: &str) -> Result<(), DatabaseError> {
        todos::Entity::delete_many()
            .filter(todos::Column::SessionId.eq(parse_int_id(session_id, "todo.session_id")?))
            .filter(todos::Column::Id.eq(parse_int_id(todo_id, "todo.id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        todos::Entity::delete_many()
            .filter(todos::Column::SessionId.eq(parse_int_id(session_id, "todo.session_id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionShareRow {
    pub session_id: String,
    pub id: String,
    pub secret: String,
    pub url: String,
}

#[derive(Clone)]
pub struct ShareRepository {
    conn: StorageConnection,
}

impl ShareRepository {
    pub fn new(conn: StorageConnection) -> Self {
        Self { conn }
    }

    pub async fn get(&self, session_id: &str) -> Result<Option<SessionShareRow>, DatabaseError> {
        let row = session_shares::Entity::find()
            .filter(
                session_shares::Column::SessionId.eq(parse_int_id(session_id, "share.session_id")?),
            )
            .one(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(row.map(|r| SessionShareRow {
            session_id: r.session_id.to_string(),
            id: r.share_id,
            secret: r.secret,
            url: r.url,
        }))
    }

    pub async fn upsert(&self, share: &SessionShareRow) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp_millis();
        let insert = session_shares::ActiveModel {
            id: Default::default(),
            session_id: Set(parse_int_id(&share.session_id, "share.session_id")?),
            share_id: Set(share.id.clone()),
            secret: Set(share.secret.clone()),
            url: Set(share.url.clone()),
            created_at: Set(now),
        };

        session_shares::Entity::insert(insert)
            .on_conflict(
                OnConflict::column(session_shares::Column::SessionId)
                    .update_columns([
                        session_shares::Column::ShareId,
                        session_shares::Column::Secret,
                        session_shares::Column::Url,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete(&self, session_id: &str) -> Result<(), DatabaseError> {
        session_shares::Entity::delete_many()
            .filter(
                session_shares::Column::SessionId.eq(parse_int_id(session_id, "share.session_id")?),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub created_at: i64,
    pub part_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_arguments: Option<String>,
    pub tool_result: Option<String>,
    pub tool_error: Option<String>,
    pub tool_status: Option<String>,
    pub file_url: Option<String>,
    pub file_filename: Option<String>,
    pub file_mime: Option<String>,
    pub reasoning: Option<String>,
    pub sort_order: i64,
    pub data: Option<String>,
}

#[derive(Clone)]
pub struct PartRepository {
    conn: StorageConnection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartSummaryRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub created_at: i64,
    pub part_type: String,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_status: Option<String>,
    pub sort_order: i64,
}

impl PartRepository {
    pub fn new(conn: StorageConnection) -> Self {
        Self { conn }
    }

    pub async fn get(&self, id: &str) -> Result<Option<PartRow>, DatabaseError> {
        let row = parts::Entity::find()
            .filter(parts::Column::Id.eq(parse_int_id(id, "part.id")?))
            .one(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(row.map(|r| PartRow {
            id: r.id.to_string(),
            message_id: r.message_id.to_string(),
            session_id: r.session_id.to_string(),
            created_at: r.created_at,
            part_type: r.part_type,
            text: r.text,
            tool_name: r.tool_name,
            tool_call_id: r.tool_call_id,
            tool_arguments: r.tool_arguments,
            tool_result: r.tool_result,
            tool_error: r.tool_error,
            tool_status: r.tool_status,
            file_url: r.file_url,
            file_filename: r.file_filename,
            file_mime: r.file_mime,
            reasoning: r.reasoning,
            sort_order: r.sort_order,
            data: r.data,
        }))
    }

    pub async fn list_for_message(&self, message_id: &str) -> Result<Vec<PartRow>, DatabaseError> {
        let rows = parts::Entity::find()
            .filter(parts::Column::MessageId.eq(parse_int_id(message_id, "part.message_id")?))
            .order_by_asc(parts::Column::SortOrder)
            .order_by_asc(parts::Column::CreatedAt)
            .order_by_asc(parts::Column::Id)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id.to_string(),
                message_id: r.message_id.to_string(),
                session_id: r.session_id.to_string(),
                created_at: r.created_at,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                file_url: r.file_url,
                file_filename: r.file_filename,
                file_mime: r.file_mime,
                reasoning: r.reasoning,
                sort_order: r.sort_order,
                data: r.data,
            })
            .collect())
    }

    pub async fn count_for_message(&self, message_id: &str) -> Result<u64, DatabaseError> {
        parts::Entity::find()
            .filter(parts::Column::MessageId.eq(parse_int_id(message_id, "part.message_id")?))
            .count(&self.conn)
            .await
            .map_err(map_query_err)
    }

    pub async fn list_summaries_for_message_page(
        &self,
        message_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PartSummaryRow>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows: Vec<(
            i64,
            i64,
            i64,
            i64,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
        )> = parts::Entity::find()
            .filter(parts::Column::MessageId.eq(parse_int_id(message_id, "part.message_id")?))
            .select_only()
            .column(parts::Column::Id)
            .column(parts::Column::MessageId)
            .column(parts::Column::SessionId)
            .column(parts::Column::CreatedAt)
            .column(parts::Column::PartType)
            .column(parts::Column::ToolName)
            .column(parts::Column::ToolCallId)
            .column(parts::Column::ToolStatus)
            .column(parts::Column::SortOrder)
            .order_by_asc(parts::Column::SortOrder)
            .order_by_asc(parts::Column::CreatedAt)
            .order_by_asc(parts::Column::Id)
            .limit(limit)
            .offset(offset)
            .into_tuple()
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    message_id,
                    session_id,
                    created_at,
                    part_type,
                    tool_name,
                    tool_call_id,
                    tool_status,
                    sort_order,
                )| PartSummaryRow {
                    id: id.to_string(),
                    message_id: message_id.to_string(),
                    session_id: session_id.to_string(),
                    created_at,
                    part_type,
                    tool_name,
                    tool_call_id,
                    tool_status,
                    sort_order,
                },
            )
            .collect())
    }

    pub async fn list_for_message_page(
        &self,
        message_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PartRow>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows = parts::Entity::find()
            .filter(parts::Column::MessageId.eq(parse_int_id(message_id, "part.message_id")?))
            .order_by_asc(parts::Column::SortOrder)
            .order_by_asc(parts::Column::CreatedAt)
            .order_by_asc(parts::Column::Id)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id.to_string(),
                message_id: r.message_id.to_string(),
                session_id: r.session_id.to_string(),
                created_at: r.created_at,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                file_url: r.file_url,
                file_filename: r.file_filename,
                file_mime: r.file_mime,
                reasoning: r.reasoning,
                sort_order: r.sort_order,
                data: r.data,
            })
            .collect())
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<PartRow>, DatabaseError> {
        let rows = parts::Entity::find()
            .filter(parts::Column::SessionId.eq(parse_int_id(session_id, "part.session_id")?))
            .order_by_asc(parts::Column::SortOrder)
            .order_by_asc(parts::Column::CreatedAt)
            .order_by_asc(parts::Column::Id)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id.to_string(),
                message_id: r.message_id.to_string(),
                session_id: r.session_id.to_string(),
                created_at: r.created_at,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                file_url: r.file_url,
                file_filename: r.file_filename,
                file_mime: r.file_mime,
                reasoning: r.reasoning,
                sort_order: r.sort_order,
                data: r.data,
            })
            .collect())
    }

    pub async fn count_for_session(&self, session_id: &str) -> Result<u64, DatabaseError> {
        parts::Entity::find()
            .filter(parts::Column::SessionId.eq(parse_int_id(session_id, "part.session_id")?))
            .count(&self.conn)
            .await
            .map_err(map_query_err)
    }

    pub async fn list_for_session_page(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PartRow>, DatabaseError> {
        let (limit, offset) = normalize_limit_offset(limit, offset)?;
        let rows = parts::Entity::find()
            .filter(parts::Column::SessionId.eq(parse_int_id(session_id, "part.session_id")?))
            .order_by_asc(parts::Column::SortOrder)
            .order_by_asc(parts::Column::CreatedAt)
            .order_by_asc(parts::Column::Id)
            .limit(limit)
            .offset(offset)
            .all(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id.to_string(),
                message_id: r.message_id.to_string(),
                session_id: r.session_id.to_string(),
                created_at: r.created_at,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                file_url: r.file_url,
                file_filename: r.file_filename,
                file_mime: r.file_mime,
                reasoning: r.reasoning,
                sort_order: r.sort_order,
                data: r.data,
            })
            .collect())
    }

    pub async fn upsert(&self, part: &PartRow) -> Result<(), DatabaseError> {
        let insert = parts::ActiveModel {
            id: Set(parse_int_id(&part.id, "part.id")?),
            message_id: Set(parse_int_id(&part.message_id, "part.message_id")?),
            session_id: Set(parse_int_id(&part.session_id, "part.session_id")?),
            created_at: Set(part.created_at),
            part_type: Set(part.part_type.clone()),
            text: Set(part.text.clone()),
            tool_name: Set(part.tool_name.clone()),
            tool_call_id: Set(part.tool_call_id.clone()),
            tool_arguments: Set(part.tool_arguments.clone()),
            tool_result: Set(part.tool_result.clone()),
            tool_error: Set(part.tool_error.clone()),
            tool_status: Set(part.tool_status.clone()),
            file_url: Set(part.file_url.clone()),
            file_filename: Set(part.file_filename.clone()),
            file_mime: Set(part.file_mime.clone()),
            reasoning: Set(part.reasoning.clone()),
            sort_order: Set(part.sort_order),
            data: Set(part.data.clone()),
            ..Default::default()
        };

        parts::Entity::insert(insert)
            .on_conflict(
                OnConflict::column(parts::Column::Id)
                    .update_columns([
                        parts::Column::MessageId,
                        parts::Column::SessionId,
                        parts::Column::CreatedAt,
                        parts::Column::PartType,
                        parts::Column::Text,
                        parts::Column::ToolName,
                        parts::Column::ToolCallId,
                        parts::Column::ToolArguments,
                        parts::Column::ToolResult,
                        parts::Column::ToolError,
                        parts::Column::ToolStatus,
                        parts::Column::FileUrl,
                        parts::Column::FileFilename,
                        parts::Column::FileMime,
                        parts::Column::Reasoning,
                        parts::Column::SortOrder,
                        parts::Column::Data,
                    ])
                    .to_owned(),
            )
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        parts::Entity::delete_many()
            .filter(parts::Column::Id.eq(parse_int_id(id, "part.id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete_for_message(&self, message_id: &str) -> Result<(), DatabaseError> {
        parts::Entity::delete_many()
            .filter(parts::Column::MessageId.eq(parse_int_id(message_id, "part.message_id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        parts::Entity::delete_many()
            .filter(parts::Column::SessionId.eq(parse_int_id(session_id, "part.session_id")?))
            .exec(&self.conn)
            .await
            .map_err(map_query_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use chrono::Utc;
    use rocode_core::contracts::scheduler::keys as scheduler_keys;
    use rocode_core::contracts::session::keys as session_keys;
    use rocode_session::{Role, Session, SessionMessage, SessionTime};
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use std::collections::HashMap;

    fn make_session(id: &str) -> Session {
        let id = parse_int_id(id, "test.session.id").unwrap().to_string();
        Session {
            id: id.clone(),
            directory: "/tmp/test".to_string(),
            parent_id: None,
            title: format!("Session {}", id),
            version: "1.0.0".to_string(),
            time: SessionTime::default(),
            messages: vec![],
            summary: None,
            share: None,
            revert: None,
            permission: None,
            usage: None,
            active: true,
            metadata: HashMap::new(),
            cached_at: Utc::now(),
        }
    }

    fn make_message(id: &str, session_id: &str, role: Role) -> SessionMessage {
        let id = parse_int_id(id, "test.message.id").unwrap().to_string();
        let session_id = parse_int_id(session_id, "test.message.session_id")
            .unwrap()
            .to_string();
        SessionMessage {
            id,
            session_id,
            role,
            parts: vec![],
            created_at: Utc::now(),
            metadata: HashMap::new(),
            usage: None,
            finish: None,
        }
    }

    #[tokio::test]
    async fn session_metadata_roundtrips() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());

        let mut session = make_session("s_meta");
        session.metadata.insert(
            scheduler_keys::PROFILE.to_string(),
            serde_json::json!("sisyphus"),
        );
        session.metadata.insert(
            session_keys::SCHEDULER_APPLIED.to_string(),
            serde_json::json!(true),
        );

        session_repo.upsert(&session).await.unwrap();

        let loaded = session_repo.get("s_meta").await.unwrap().unwrap();
        assert_eq!(
            loaded.metadata.get(scheduler_keys::PROFILE),
            Some(&serde_json::json!("sisyphus"))
        );
        assert_eq!(
            loaded.metadata.get(session_keys::SCHEDULER_APPLIED),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn message_metadata_roundtrips() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        session_repo.upsert(&make_session("s_meta")).await.unwrap();

        let mut message = make_message("m_meta", "s_meta", Role::User);
        message.metadata.insert(
            session_keys::RESOLVED_SYSTEM_PROMPT.to_string(),
            serde_json::json!("You are Sisyphus"),
        );
        message.metadata.insert(
            scheduler_keys::RESOLVED_PROFILE.to_string(),
            serde_json::json!("sisyphus"),
        );
        message
            .metadata
            .insert("mode".to_string(), serde_json::json!("sisyphus"));

        message_repo.create(&message).await.unwrap();

        let loaded = message_repo.get("m_meta").await.unwrap().unwrap();
        assert_eq!(
            loaded.metadata.get(session_keys::RESOLVED_SYSTEM_PROMPT),
            Some(&serde_json::json!("You are Sisyphus"))
        );
        assert_eq!(
            loaded.metadata.get(scheduler_keys::RESOLVED_PROFILE),
            Some(&serde_json::json!("sisyphus"))
        );
        assert_eq!(
            loaded.metadata.get("mode"),
            Some(&serde_json::json!("sisyphus"))
        );
    }

    #[tokio::test]
    async fn flush_with_messages_atomicity() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        let session = make_session("s1");
        let msgs = vec![
            make_message("m1", "s1", Role::User),
            make_message("m2", "s1", Role::Assistant),
        ];

        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();

        let loaded = session_repo.get("s1").await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().title, "Session 1");

        let loaded_msgs = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(loaded_msgs.len(), 2);
        assert_eq!(loaded_msgs[0].id, "1");
        assert_eq!(loaded_msgs[1].id, "2");
    }

    #[tokio::test]
    async fn flush_deletes_stale_messages() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        let session = make_session("s1");
        let msgs = vec![
            make_message("m1", "s1", Role::User),
            make_message("m2", "s1", Role::Assistant),
            make_message("m3", "s1", Role::User),
        ];

        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();
        assert_eq!(message_repo.list_for_session("s1").await.unwrap().len(), 3);

        // Simulate revert: flush with only m1
        let msgs_after_revert = vec![make_message("m1", "s1", Role::User)];
        session_repo
            .flush_with_messages(&session, &msgs_after_revert)
            .await
            .unwrap();

        let remaining = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "1");

        assert!(message_repo.get("2").await.unwrap().is_none());
        assert!(message_repo.get("3").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn flush_deletes_stale_messages_large_set() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        let session = make_session("s1");

        let mut msgs: Vec<SessionMessage> = (0..1100)
            .map(|i| make_message(&format!("m{}", i), "s1", Role::User))
            .collect();

        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();
        assert_eq!(
            message_repo.list_for_session("s1").await.unwrap().len(),
            1100
        );

        // Remove last 100
        msgs.truncate(1000);
        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();

        let remaining = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(remaining.len(), 1000);
        assert!(message_repo.get("1099").await.unwrap().is_none());
        assert!(message_repo.get("0").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn upsert_updates_existing_session() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());

        let mut session = make_session("s1");
        session_repo.upsert(&session).await.unwrap();

        session.title = "Updated Title".to_string();
        session_repo.upsert(&session).await.unwrap();

        let loaded = session_repo.get("s1").await.unwrap().unwrap();
        assert_eq!(loaded.title, "Updated Title");

        let all = session_repo.list(None, 100).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn session_count_and_pagination_work() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());

        let mut s1 = make_session("s1");
        s1.time.created = 10;
        s1.time.updated = 10;
        session_repo.upsert(&s1).await.unwrap();

        let mut s2 = make_session("s2");
        s2.time.created = 20;
        s2.time.updated = 20;
        session_repo.upsert(&s2).await.unwrap();

        let mut s3 = make_session("s3");
        s3.time.created = 30;
        s3.time.updated = 30;
        session_repo.upsert(&s3).await.unwrap();

        let mut s4 = make_session("s4");
        s4.time.created = 40;
        s4.time.updated = 40;
        session_repo.upsert(&s4).await.unwrap();

        let mut s5 = make_session("s5");
        s5.directory = "/tmp/other".to_string();
        s5.time.created = 50;
        s5.time.updated = 50;
        session_repo.upsert(&s5).await.unwrap();

        assert_eq!(session_repo.count(None).await.unwrap(), 5);
        assert_eq!(
            session_repo.count_for_directory("/tmp/test").await.unwrap(),
            4
        );
        assert_eq!(
            session_repo
                .count_for_directory("/tmp/other")
                .await
                .unwrap(),
            1
        );

        let first_two = session_repo.list_page(None, 2, 0).await.unwrap();
        assert_eq!(
            first_two.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["5", "4"]
        );

        let middle_two = session_repo.list_page(None, 2, 2).await.unwrap();
        assert_eq!(
            middle_two.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["3", "2"]
        );

        let dir_sessions = session_repo
            .list_for_directory_page("/tmp/test", 10, 0)
            .await
            .unwrap();
        assert_eq!(
            dir_sessions
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["4", "3", "2", "1"]
        );
    }

    #[tokio::test]
    async fn message_count_and_pagination_work() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        session_repo.upsert(&make_session("s1")).await.unwrap();

        for (idx, millis) in [10, 20, 30, 40, 50].iter().enumerate() {
            let id = format!("m{}", idx + 1);
            let mut msg = make_message(&id, "s1", Role::User);
            msg.created_at = DateTime::from_timestamp_millis(*millis).unwrap_or_else(Utc::now);
            message_repo.create(&msg).await.unwrap();
        }

        assert_eq!(message_repo.count_for_session("s1").await.unwrap(), 5);

        let page = message_repo
            .list_for_session_page("s1", 2, 2)
            .await
            .unwrap();
        assert_eq!(
            page.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["3", "4"]
        );
    }

    #[tokio::test]
    async fn flush_rolls_back_on_mid_transaction_failure() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.conn().clone());
        let message_repo = MessageRepository::new(db.conn().clone());

        // Establish baseline: session "v1" with messages m1, m2
        let mut session = make_session("s1");
        session.title = "v1".to_string();
        let msgs = vec![
            make_message("m1", "s1", Role::User),
            make_message("m2", "s1", Role::Assistant),
        ];
        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();

        // Sabotage: rename messages table so message upsert fails inside the tx
        db.conn()
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE messages RENAME TO messages_backup".to_string(),
            ))
            .await
            .unwrap();

        // Attempt flush with updated title — session upsert succeeds within tx,
        // but message upsert hits the missing table and the whole tx should roll back.
        session.title = "v2".to_string();
        let new_msgs = vec![make_message("m3", "s1", Role::User)];
        let result = session_repo.flush_with_messages(&session, &new_msgs).await;
        assert!(
            result.is_err(),
            "flush should fail when messages table is missing"
        );

        // Restore messages table
        db.conn()
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "ALTER TABLE messages_backup RENAME TO messages".to_string(),
            ))
            .await
            .unwrap();

        // Verify rollback: session title must still be "v1"
        let loaded = session_repo.get("s1").await.unwrap().unwrap();
        assert_eq!(
            loaded.title, "v1",
            "session upsert should have been rolled back"
        );

        // Verify original messages are intact
        let loaded_msgs = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(
            loaded_msgs.len(),
            2,
            "original messages should survive the failed tx"
        );
        assert_eq!(loaded_msgs[0].id, "1");
        assert_eq!(loaded_msgs[1].id, "2");
    }
}
