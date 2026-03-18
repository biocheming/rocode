use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json;
use sqlx::{FromRow, SqlitePool};

use rocode_types::{
    MessagePart, MessageRole, Session, SessionMessage, SessionShare, SessionStatus, SessionSummary,
    SessionTime, SessionUsage,
};

use crate::database::DatabaseError;

// ── Shared SQL constants (single source of truth for upsert schemas) ────────

const SESSION_UPSERT_SQL: &str = r#"
INSERT INTO sessions (
    id, project_id, parent_id, slug, directory, title, version, share_url,
    summary_additions, summary_deletions, summary_files, summary_diffs,
    revert, permission, metadata,
    usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
    usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
    status, created_at, updated_at, time_compacting, time_archived
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title, version = excluded.version, share_url = excluded.share_url,
    summary_additions = excluded.summary_additions, summary_deletions = excluded.summary_deletions,
    summary_files = excluded.summary_files, summary_diffs = excluded.summary_diffs,
    revert = excluded.revert, permission = excluded.permission, metadata = excluded.metadata,
    usage_input_tokens = excluded.usage_input_tokens, usage_output_tokens = excluded.usage_output_tokens,
    usage_reasoning_tokens = excluded.usage_reasoning_tokens,
    usage_cache_write_tokens = excluded.usage_cache_write_tokens,
    usage_cache_read_tokens = excluded.usage_cache_read_tokens,
    usage_total_cost = excluded.usage_total_cost,
    status = excluded.status, updated_at = excluded.updated_at,
    time_compacting = excluded.time_compacting, time_archived = excluded.time_archived
"#;

const MESSAGE_UPSERT_SQL: &str = r#"
INSERT INTO messages (id, session_id, role, created_at, finish, metadata, data)
VALUES (?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET
    session_id = excluded.session_id,
    role = excluded.role,
    created_at = excluded.created_at,
    finish = excluded.finish,
    metadata = excluded.metadata,
    data = excluded.data
"#;

fn bind_session_upsert<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    session: &'q Session,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    let usage = session.usage.as_ref();
    query
        .bind(&session.id)
        .bind(&session.project_id)
        .bind(&session.parent_id)
        .bind(&session.slug)
        .bind(&session.directory)
        .bind(&session.title)
        .bind(&session.version)
        .bind(session.share.as_ref().map(|s| s.url.as_str()))
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.additions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.deletions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.files as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .and_then(|s| serde_json::to_string(&s.diffs).ok()),
        )
        .bind(
            session
                .revert
                .as_ref()
                .and_then(|r| serde_json::to_string(r).ok()),
        )
        .bind(
            session
                .permission
                .as_ref()
                .and_then(|p| serde_json::to_string(p).ok()),
        )
        .bind(serde_json::to_string(&session.metadata).ok())
        .bind(usage.map(|u| u.input_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.output_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.total_cost).unwrap_or(0.0))
        .bind(status_to_string(&session.status))
        .bind(session.time.created)
        .bind(session.time.updated)
        .bind(session.time.compacting)
        .bind(session.time.archived)
}

fn role_to_str(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

#[derive(Debug, FromRow)]
struct SessionRow {
    id: String,
    project_id: String,
    parent_id: Option<String>,
    slug: String,
    directory: String,
    title: String,
    version: String,
    share_url: Option<String>,
    summary_additions: Option<i64>,
    summary_deletions: Option<i64>,
    summary_files: Option<i64>,
    summary_diffs: Option<String>,
    revert: Option<String>,
    permission: Option<String>,
    metadata: Option<String>,
    usage_input_tokens: Option<i64>,
    usage_output_tokens: Option<i64>,
    usage_reasoning_tokens: Option<i64>,
    usage_cache_write_tokens: Option<i64>,
    usage_cache_read_tokens: Option<i64>,
    usage_total_cost: Option<f64>,
    status: String,
    created_at: i64,
    updated_at: i64,
    time_compacting: Option<i64>,
    time_archived: Option<i64>,
}

impl SessionRow {
    fn into_session(self) -> Session {
        let summary = if self.summary_additions.is_some()
            || self.summary_deletions.is_some()
            || self.summary_files.is_some()
        {
            Some(SessionSummary {
                additions: self.summary_additions.unwrap_or(0) as u64,
                deletions: self.summary_deletions.unwrap_or(0) as u64,
                files: self.summary_files.unwrap_or(0) as u64,
                diffs: self
                    .summary_diffs
                    .and_then(|d| serde_json::from_str(&d).ok()),
            })
        } else {
            None
        };

        let created_dt = DateTime::from_timestamp_millis(self.created_at).unwrap_or_else(Utc::now);
        let updated_dt = DateTime::from_timestamp_millis(self.updated_at).unwrap_or_else(Utc::now);

        Session {
            id: self.id,
            slug: self.slug,
            project_id: self.project_id,
            directory: self.directory,
            parent_id: self.parent_id,
            title: self.title,
            version: self.version,
            time: SessionTime {
                created: self.created_at,
                updated: self.updated_at,
                compacting: self.time_compacting,
                archived: self.time_archived,
            },
            messages: vec![],
            summary,
            share: self.share_url.map(|url| SessionShare { url }),
            revert: self.revert.and_then(|r| serde_json::from_str(&r).ok()),
            permission: self.permission.and_then(|p| serde_json::from_str(&p).ok()),
            metadata: self
                .metadata
                .and_then(|m| serde_json::from_str(&m).ok())
                .unwrap_or_default(),
            usage: if self.usage_input_tokens.is_some() {
                Some(SessionUsage {
                    input_tokens: self.usage_input_tokens.unwrap_or(0) as u64,
                    output_tokens: self.usage_output_tokens.unwrap_or(0) as u64,
                    reasoning_tokens: self.usage_reasoning_tokens.unwrap_or(0) as u64,
                    cache_write_tokens: self.usage_cache_write_tokens.unwrap_or(0) as u64,
                    cache_read_tokens: self.usage_cache_read_tokens.unwrap_or(0) as u64,
                    total_cost: self.usage_total_cost.unwrap_or(0.0),
                })
            } else {
                None
            },
            status: string_to_status(&self.status),
            created_at: created_dt,
            updated_at: updated_dt,
        }
    }
}

#[derive(Clone)]
pub struct SessionRepository {
    pool: SqlitePool,
}

impl SessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, session: &Session) -> Result<(), DatabaseError> {
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

        let share_url = session.share.as_ref().map(|s| s.url.as_str());

        let usage = session.usage.as_ref();

        sqlx::query(
            r#"
            INSERT INTO sessions (
                id, project_id, parent_id, slug, directory, title, version, share_url,
                summary_additions, summary_deletions, summary_files, summary_diffs,
                revert, permission, metadata,
                usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
                usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
                status, created_at, updated_at, time_compacting, time_archived
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&session.id)
        .bind(&session.project_id)
        .bind(&session.parent_id)
        .bind(&session.slug)
        .bind(&session.directory)
        .bind(&session.title)
        .bind(&session.version)
        .bind(share_url)
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.additions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.deletions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.files as i64)
                .unwrap_or(0),
        )
        .bind(summary_diffs)
        .bind(revert_json)
        .bind(permission_json)
        .bind(metadata_json)
        .bind(usage.map(|u| u.input_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.output_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.total_cost).unwrap_or(0.0))
        .bind(status_to_string(&session.status))
        .bind(session.time.created)
        .bind(session.time.updated)
        .bind(session.time.compacting)
        .bind(session.time.archived)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<Session>, DatabaseError> {
        let row = sqlx::query_as::<_, SessionRow>(
            r#"SELECT 
                id, project_id, parent_id, slug, directory, title, version, share_url,
                summary_additions, summary_deletions, summary_files, summary_diffs,
                revert, permission, metadata,
                usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
                usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
                status, created_at, updated_at, time_compacting, time_archived
            FROM sessions WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(row.map(|r| r.into_session()))
    }

    pub async fn list(
        &self,
        project_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Session>, DatabaseError> {
        let rows = match project_id {
            Some(pid) => sqlx::query_as::<_, SessionRow>(
                r#"SELECT 
                        id, project_id, parent_id, slug, directory, title, version, share_url,
                        summary_additions, summary_deletions, summary_files, summary_diffs,
                        revert, permission, metadata,
                        usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
                        usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
                        status, created_at, updated_at, time_compacting, time_archived
                    FROM sessions WHERE project_id = ? 
                    ORDER BY updated_at DESC LIMIT ?"#,
            )
            .bind(pid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?,
            None => sqlx::query_as::<_, SessionRow>(
                r#"SELECT 
                        id, project_id, parent_id, slug, directory, title, version, share_url,
                        summary_additions, summary_deletions, summary_files, summary_diffs,
                        revert, permission, metadata,
                        usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
                        usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
                        status, created_at, updated_at, time_compacting, time_archived
                    FROM sessions 
                    ORDER BY updated_at DESC LIMIT ?"#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?,
        };

        Ok(rows.into_iter().map(|r| r.into_session()).collect())
    }

    pub async fn update(&self, session: &Session) -> Result<(), DatabaseError> {
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

        let share_url = session.share.as_ref().map(|s| s.url.as_str());
        let metadata_json = serde_json::to_string(&session.metadata).ok();

        let usage = session.usage.as_ref();

        sqlx::query(
            r#"
            UPDATE sessions SET
                title = ?, version = ?, share_url = ?,
                summary_additions = ?, summary_deletions = ?, summary_files = ?, summary_diffs = ?,
                revert = ?, permission = ?, metadata = ?,
                usage_input_tokens = ?, usage_output_tokens = ?, usage_reasoning_tokens = ?,
                usage_cache_write_tokens = ?, usage_cache_read_tokens = ?, usage_total_cost = ?,
                status = ?, updated_at = ?, time_compacting = ?, time_archived = ?
            WHERE id = ?
            "#,
        )
        .bind(&session.title)
        .bind(&session.version)
        .bind(share_url)
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.additions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.deletions as i64)
                .unwrap_or(0),
        )
        .bind(
            session
                .summary
                .as_ref()
                .map(|s| s.files as i64)
                .unwrap_or(0),
        )
        .bind(summary_diffs)
        .bind(revert_json)
        .bind(permission_json)
        .bind(metadata_json)
        .bind(usage.map(|u| u.input_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.output_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.reasoning_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_write_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.cache_read_tokens as i64).unwrap_or(0))
        .bind(usage.map(|u| u.total_cost).unwrap_or(0.0))
        .bind(status_to_string(&session.status))
        .bind(session.time.updated)
        .bind(session.time.compacting)
        .bind(session.time.archived)
        .bind(&session.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn upsert(&self, session: &Session) -> Result<(), DatabaseError> {
        bind_session_upsert(sqlx::query(SESSION_UPSERT_SQL), session)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn list_children(&self, parent_id: &str) -> Result<Vec<Session>, DatabaseError> {
        let rows = sqlx::query_as::<_, SessionRow>(
            r#"SELECT 
                id, project_id, parent_id, slug, directory, title, version, share_url,
                summary_additions, summary_deletions, summary_files, summary_diffs,
                revert, permission, metadata,
                usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
                usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
                status, created_at, updated_at, time_compacting, time_archived
            FROM sessions WHERE parent_id = ? 
            ORDER BY created_at DESC"#,
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into_session()).collect())
    }

    /// Atomically upsert a session, upsert its messages, and delete stale messages
    /// that no longer exist in the session layer (e.g. after revert/delete).
    pub async fn flush_with_messages(
        &self,
        session: &Session,
        messages: &[SessionMessage],
    ) -> Result<(), DatabaseError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DatabaseError::TransactionError(e.to_string()))?;

        // Upsert session
        bind_session_upsert(sqlx::query(SESSION_UPSERT_SQL), session)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        // Upsert messages
        for msg in messages {
            let data_json = serde_json::to_string(&msg.parts)
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            let metadata_json = serde_json::to_string(&msg.metadata)
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            sqlx::query(MESSAGE_UPSERT_SQL)
                .bind(&msg.id)
                .bind(&msg.session_id)
                .bind(role_to_str(&msg.role))
                .bind(msg.created_at.timestamp_millis())
                .bind(&msg.finish)
                .bind(&metadata_json)
                .bind(&data_json)
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        }

        // Delete stale messages
        let keep_ids: Vec<&str> = messages.iter().map(|m| m.id.as_str()).collect();
        if keep_ids.is_empty() {
            sqlx::query("DELETE FROM messages WHERE session_id = ?")
                .bind(&session.id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        } else if keep_ids.len() <= 998 {
            let placeholders: String = keep_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "DELETE FROM messages WHERE session_id = ? AND id NOT IN ({})",
                placeholders
            );
            let mut query = sqlx::query(&sql).bind(&session.id);
            for id in &keep_ids {
                query = query.bind(*id);
            }
            query
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        } else {
            sqlx::query("CREATE TEMP TABLE IF NOT EXISTS _keep_msg_ids (id TEXT PRIMARY KEY)")
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            sqlx::query("DELETE FROM _keep_msg_ids")
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            for chunk in keep_ids.chunks(500) {
                let placeholders: String =
                    chunk.iter().map(|_| "(?)").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "INSERT OR IGNORE INTO _keep_msg_ids (id) VALUES {}",
                    placeholders
                );
                let mut query = sqlx::query(&sql);
                for id in chunk {
                    query = query.bind(*id);
                }
                query
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            }
            sqlx::query(
                "DELETE FROM messages WHERE session_id = ? AND id NOT IN (SELECT id FROM _keep_msg_ids)",
            )
            .bind(&session.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            sqlx::query("DROP TABLE IF EXISTS _keep_msg_ids")
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| DatabaseError::TransactionError(e.to_string()))?;
        Ok(())
    }
}

fn status_to_string(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::Completed => "completed",
        SessionStatus::Archived => "archived",
        SessionStatus::Compacting => "compacting",
    }
}

fn string_to_status(s: &str) -> SessionStatus {
    match s {
        "completed" => SessionStatus::Completed,
        "archived" => SessionStatus::Archived,
        "compacting" => SessionStatus::Compacting,
        _ => SessionStatus::Active,
    }
}

#[derive(Clone)]
pub struct MessageRepository {
    pool: SqlitePool,
}

impl MessageRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, message: &SessionMessage) -> Result<(), DatabaseError> {
        let data_json = serde_json::to_string(&message.parts)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        let metadata_json = serde_json::to_string(&message.metadata)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let role_str = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        };

        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, role, created_at, finish, metadata, data)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(role_str)
        .bind(message.created_at.timestamp_millis())
        .bind(&message.finish)
        .bind(&metadata_json)
        .bind(&data_json)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn upsert(&self, message: &SessionMessage) -> Result<(), DatabaseError> {
        let data_json = serde_json::to_string(&message.parts)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
        let metadata_json = serde_json::to_string(&message.metadata)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        sqlx::query(MESSAGE_UPSERT_SQL)
            .bind(&message.id)
            .bind(&message.session_id)
            .bind(role_to_str(&message.role))
            .bind(message.created_at.timestamp_millis())
            .bind(&message.finish)
            .bind(&metadata_json)
            .bind(&data_json)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn list_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessage>, DatabaseError> {
        #[derive(FromRow)]
        struct MessageRow {
            id: String,
            session_id: String,
            role: String,
            created_at: i64,
            finish: Option<String>,
            metadata: Option<String>,
            data: Option<String>,
        }

        let rows = sqlx::query_as::<_, MessageRow>(
            r#"SELECT id, session_id, role, created_at, finish, metadata, data
               FROM messages WHERE session_id = ? ORDER BY created_at ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let messages: Vec<SessionMessage> = rows
            .into_iter()
            .filter_map(|row| {
                let msg_role = match row.role.as_str() {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "tool" => MessageRole::Tool,
                    _ => return None,
                };

                let parts: Vec<MessagePart> = row
                    .data
                    .and_then(|c| serde_json::from_str(&c).ok())
                    .unwrap_or_default();

                let created =
                    DateTime::from_timestamp_millis(row.created_at).unwrap_or_else(Utc::now);

                Some(SessionMessage {
                    id: row.id,
                    session_id: row.session_id,
                    role: msg_role,
                    parts,
                    created_at: created,
                    metadata: row
                        .metadata
                        .and_then(|m| serde_json::from_str(&m).ok())
                        .unwrap_or_default(),
                    finish: row.finish,
                })
            })
            .collect();

        Ok(messages)
    }

    pub async fn get(&self, id: &str) -> Result<Option<SessionMessage>, DatabaseError> {
        #[derive(FromRow)]
        struct MessageRow {
            id: String,
            session_id: String,
            role: String,
            created_at: i64,
            finish: Option<String>,
            metadata: Option<String>,
            data: Option<String>,
        }

        let row = sqlx::query_as::<_, MessageRow>(
            r#"SELECT id, session_id, role, created_at, finish, metadata, data
               FROM messages WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match row {
            Some(row) => {
                let msg_role = match row.role.as_str() {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "tool" => MessageRole::Tool,
                    _ => return Ok(None),
                };

                let parts: Vec<MessagePart> = row
                    .data
                    .and_then(|c| serde_json::from_str(&c).ok())
                    .unwrap_or_default();

                let created =
                    DateTime::from_timestamp_millis(row.created_at).unwrap_or_else(Utc::now);

                Ok(Some(SessionMessage {
                    id: row.id,
                    session_id: row.session_id,
                    role: msg_role,
                    parts,
                    created_at: created,
                    metadata: row
                        .metadata
                        .and_then(|m| serde_json::from_str(&m).ok())
                        .unwrap_or_default(),
                    finish: row.finish,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM messages WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

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

pub struct TodoRepository {
    pool: SqlitePool,
}

impl TodoRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<TodoItem>, DatabaseError> {
        #[derive(FromRow)]
        struct TodoRow {
            todo_id: String,
            content: String,
            status: String,
            priority: String,
            position: i64,
        }

        let rows = sqlx::query_as::<_, TodoRow>(
            r#"SELECT todo_id, content, status, priority, position 
               FROM todos WHERE session_id = ? ORDER BY position ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let todos: Vec<TodoItem> = rows
            .into_iter()
            .map(|row| TodoItem {
                id: row.todo_id,
                content: row.content,
                status: row.status,
                priority: row.priority,
                position: row.position,
            })
            .collect();

        Ok(todos)
    }

    pub async fn upsert(&self, session_id: &str, todo: &TodoItem) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO todos (session_id, todo_id, content, status, priority, position, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_id, todo_id) DO UPDATE SET
                content = excluded.content,
                status = excluded.status,
                priority = excluded.priority,
                position = excluded.position,
                updated_at = excluded.updated_at
            "#
        )
        .bind(session_id)
        .bind(&todo.id)
        .bind(&todo.content)
        .bind(&todo.status)
        .bind(&todo.priority)
        .bind(todo.position)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete(&self, session_id: &str, todo_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM todos WHERE session_id = ? AND todo_id = ?")
            .bind(session_id)
            .bind(todo_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM todos WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

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

pub struct ShareRepository {
    pool: SqlitePool,
}

impl ShareRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, session_id: &str) -> Result<Option<SessionShareRow>, DatabaseError> {
        #[derive(FromRow)]
        struct ShareRow {
            session_id: String,
            id: String,
            secret: String,
            url: String,
        }

        let row = sqlx::query_as::<_, ShareRow>(
            r#"SELECT session_id, id, secret, url FROM session_shares WHERE session_id = ?"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(row.map(|r| SessionShareRow {
            session_id: r.session_id,
            id: r.id,
            secret: r.secret,
            url: r.url,
        }))
    }

    pub async fn upsert(&self, share: &SessionShareRow) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO session_shares (session_id, id, secret, url, created_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(session_id) DO UPDATE SET
                id = excluded.id,
                secret = excluded.secret,
                url = excluded.url
            "#,
        )
        .bind(&share.session_id)
        .bind(&share.id)
        .bind(&share.secret)
        .bind(&share.url)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete(&self, session_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM session_shares WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartRow {
    pub id: String,
    pub message_id: String,
    pub session_id: String,
    pub part_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_arguments: Option<String>,
    pub tool_result: Option<String>,
    pub tool_error: Option<String>,
    pub tool_status: Option<String>,
    pub sort_order: i64,
}

pub struct PartRepository {
    pool: SqlitePool,
}

impl PartRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn list_for_message(&self, message_id: &str) -> Result<Vec<PartRow>, DatabaseError> {
        #[derive(FromRow)]
        struct Row {
            id: String,
            message_id: String,
            session_id: String,
            part_type: String,
            text: Option<String>,
            tool_name: Option<String>,
            tool_call_id: Option<String>,
            tool_arguments: Option<String>,
            tool_result: Option<String>,
            tool_error: Option<String>,
            tool_status: Option<String>,
            sort_order: i64,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"SELECT id, message_id, session_id, part_type, text, 
                      tool_name, tool_call_id, tool_arguments, tool_result, 
                      tool_error, tool_status, sort_order
               FROM parts WHERE message_id = ? ORDER BY sort_order ASC"#,
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id,
                message_id: r.message_id,
                session_id: r.session_id,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                sort_order: r.sort_order,
            })
            .collect())
    }

    pub async fn list_for_session(&self, session_id: &str) -> Result<Vec<PartRow>, DatabaseError> {
        #[derive(FromRow)]
        struct Row {
            id: String,
            message_id: String,
            session_id: String,
            part_type: String,
            text: Option<String>,
            tool_name: Option<String>,
            tool_call_id: Option<String>,
            tool_arguments: Option<String>,
            tool_result: Option<String>,
            tool_error: Option<String>,
            tool_status: Option<String>,
            sort_order: i64,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"SELECT id, message_id, session_id, part_type, text, 
                      tool_name, tool_call_id, tool_arguments, tool_result, 
                      tool_error, tool_status, sort_order
               FROM parts WHERE session_id = ? ORDER BY sort_order ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| PartRow {
                id: r.id,
                message_id: r.message_id,
                session_id: r.session_id,
                part_type: r.part_type,
                text: r.text,
                tool_name: r.tool_name,
                tool_call_id: r.tool_call_id,
                tool_arguments: r.tool_arguments,
                tool_result: r.tool_result,
                tool_error: r.tool_error,
                tool_status: r.tool_status,
                sort_order: r.sort_order,
            })
            .collect())
    }

    pub async fn upsert(&self, part: &PartRow) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO parts (id, message_id, session_id, part_type, text, 
                              tool_name, tool_call_id, tool_arguments, tool_result, 
                              tool_error, tool_status, sort_order, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                text = excluded.text,
                tool_name = excluded.tool_name,
                tool_call_id = excluded.tool_call_id,
                tool_arguments = excluded.tool_arguments,
                tool_result = excluded.tool_result,
                tool_error = excluded.tool_error,
                tool_status = excluded.tool_status,
                sort_order = excluded.sort_order
            "#,
        )
        .bind(&part.id)
        .bind(&part.message_id)
        .bind(&part.session_id)
        .bind(&part.part_type)
        .bind(&part.text)
        .bind(&part.tool_name)
        .bind(&part.tool_call_id)
        .bind(&part.tool_arguments)
        .bind(&part.tool_result)
        .bind(&part.tool_error)
        .bind(&part.tool_status)
        .bind(part.sort_order)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM parts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_for_message(&self, message_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM parts WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_for_session(&self, session_id: &str) -> Result<(), DatabaseError> {
        sqlx::query("DELETE FROM parts WHERE session_id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

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
    use rocode_types::{MessageRole, Session, SessionMessage, SessionStatus, SessionTime};
    use std::collections::HashMap;

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            slug: format!("slug-{}", id),
            project_id: "proj-1".to_string(),
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
            status: SessionStatus::Active,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_message(id: &str, session_id: &str, role: MessageRole) -> SessionMessage {
        SessionMessage {
            id: id.to_string(),
            session_id: session_id.to_string(),
            role,
            parts: vec![],
            created_at: Utc::now(),
            metadata: HashMap::new(),
            finish: None,
        }
    }

    #[tokio::test]
    async fn session_metadata_roundtrips() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.pool().clone());

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
        let session_repo = SessionRepository::new(db.pool().clone());
        let message_repo = MessageRepository::new(db.pool().clone());

        session_repo.upsert(&make_session("s_meta")).await.unwrap();

        let mut message = make_message("m_meta", "s_meta", MessageRole::User);
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
        let session_repo = SessionRepository::new(db.pool().clone());
        let message_repo = MessageRepository::new(db.pool().clone());

        let session = make_session("s1");
        let msgs = vec![
            make_message("m1", "s1", MessageRole::User),
            make_message("m2", "s1", MessageRole::Assistant),
        ];

        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();

        let loaded = session_repo.get("s1").await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().title, "Session s1");

        let loaded_msgs = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(loaded_msgs.len(), 2);
        assert_eq!(loaded_msgs[0].id, "m1");
        assert_eq!(loaded_msgs[1].id, "m2");
    }

    #[tokio::test]
    async fn flush_deletes_stale_messages() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.pool().clone());
        let message_repo = MessageRepository::new(db.pool().clone());

        let session = make_session("s1");
        let msgs = vec![
            make_message("m1", "s1", MessageRole::User),
            make_message("m2", "s1", MessageRole::Assistant),
            make_message("m3", "s1", MessageRole::User),
        ];

        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();
        assert_eq!(message_repo.list_for_session("s1").await.unwrap().len(), 3);

        // Simulate revert: flush with only m1
        let msgs_after_revert = vec![make_message("m1", "s1", MessageRole::User)];
        session_repo
            .flush_with_messages(&session, &msgs_after_revert)
            .await
            .unwrap();

        let remaining = message_repo.list_for_session("s1").await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "m1");

        assert!(message_repo.get("m2").await.unwrap().is_none());
        assert!(message_repo.get("m3").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_stale_large_set_uses_temp_table() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.pool().clone());
        let message_repo = MessageRepository::new(db.pool().clone());

        let session = make_session("s1");

        // 1100 messages exceeds the 998 inline limit → temp table path
        let mut msgs: Vec<SessionMessage> = (0..1100)
            .map(|i| make_message(&format!("m{}", i), "s1", MessageRole::User))
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
        assert!(message_repo.get("m1099").await.unwrap().is_none());
        assert!(message_repo.get("m0").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn upsert_updates_existing_session() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.pool().clone());

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
    async fn flush_rolls_back_on_mid_transaction_failure() {
        let db = Database::in_memory().await.unwrap();
        let session_repo = SessionRepository::new(db.pool().clone());
        let message_repo = MessageRepository::new(db.pool().clone());

        // Establish baseline: session "v1" with messages m1, m2
        let mut session = make_session("s1");
        session.title = "v1".to_string();
        let msgs = vec![
            make_message("m1", "s1", MessageRole::User),
            make_message("m2", "s1", MessageRole::Assistant),
        ];
        session_repo
            .flush_with_messages(&session, &msgs)
            .await
            .unwrap();

        // Sabotage: rename messages table so message upsert fails inside the tx
        sqlx::query("ALTER TABLE messages RENAME TO messages_backup")
            .execute(db.pool())
            .await
            .unwrap();

        // Attempt flush with updated title — session upsert succeeds within tx,
        // but message upsert hits the missing table and the whole tx should roll back.
        session.title = "v2".to_string();
        let new_msgs = vec![make_message("m3", "s1", MessageRole::User)];
        let result = session_repo.flush_with_messages(&session, &new_msgs).await;
        assert!(
            result.is_err(),
            "flush should fail when messages table is missing"
        );

        // Restore messages table
        sqlx::query("ALTER TABLE messages_backup RENAME TO messages")
            .execute(db.pool())
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
        assert_eq!(loaded_msgs[0].id, "m1");
        assert_eq!(loaded_msgs[1].id, "m2");
    }
}
