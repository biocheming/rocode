use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260319_000013_int_primary_keys"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        let has_pk = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'pk' LIMIT 1".to_string(),
            ))
            .await?
            .is_some();

        if !has_pk {
            // Fresh schema already uses integer `id` primary keys.
            return Ok(());
        }

        manager
            .get_connection()
            .execute_unprepared(
                r#"
PRAGMA foreign_keys=OFF;

CREATE TABLE IF NOT EXISTS sessions_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL DEFAULT '',
    parent_id INTEGER,
    slug TEXT NOT NULL DEFAULT '',
    directory TEXT NOT NULL,
    title TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '1.0.0',
    share_url TEXT,
    summary_additions INTEGER NOT NULL DEFAULT 0,
    summary_deletions INTEGER NOT NULL DEFAULT 0,
    summary_files INTEGER NOT NULL DEFAULT 0,
    summary_diffs TEXT,
    revert TEXT,
    permission TEXT,
    metadata TEXT,
    usage_input_tokens INTEGER NOT NULL DEFAULT 0,
    usage_output_tokens INTEGER NOT NULL DEFAULT 0,
    usage_reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    usage_cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    usage_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    usage_total_cost REAL NOT NULL DEFAULT 0.0,
    status BOOLEAN NOT NULL DEFAULT FALSE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO sessions_new (
    id, project_id, parent_id, slug, directory, title, version, share_url,
    summary_additions, summary_deletions, summary_files, summary_diffs,
    revert, permission, metadata,
    usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
    usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
    status, created_at, updated_at
)
SELECT
    pk, project_id,
    (SELECT parent.pk FROM sessions AS parent WHERE parent.id = sessions.parent_id),
    slug, directory, title, version, share_url,
    summary_additions, summary_deletions, summary_files, summary_diffs,
    revert, permission, metadata,
    usage_input_tokens, usage_output_tokens, usage_reasoning_tokens,
    usage_cache_write_tokens, usage_cache_read_tokens, usage_total_cost,
    status, created_at, updated_at
FROM sessions;

DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;

CREATE TABLE IF NOT EXISTS messages_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    role TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    provider_id TEXT,
    model_id TEXT,
    tokens_input INTEGER NOT NULL DEFAULT 0,
    tokens_output INTEGER NOT NULL DEFAULT 0,
    tokens_reasoning INTEGER NOT NULL DEFAULT 0,
    tokens_cache_read INTEGER NOT NULL DEFAULT 0,
    tokens_cache_write INTEGER NOT NULL DEFAULT 0,
    cost REAL NOT NULL DEFAULT 0.0,
    finish TEXT,
    metadata TEXT,
    data TEXT,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE ON UPDATE CASCADE
);

INSERT INTO messages_new (
    id, session_id, role, created_at, provider_id, model_id,
    tokens_input, tokens_output, tokens_reasoning,
    tokens_cache_read, tokens_cache_write, cost,
    finish, metadata, data
)
SELECT
    messages.pk,
    sessions.pk,
    messages.role,
    messages.created_at,
    messages.provider_id,
    messages.model_id,
    tokens_input, tokens_output, tokens_reasoning,
    tokens_cache_read, tokens_cache_write, cost,
    finish, metadata, data
FROM messages
JOIN sessions ON sessions.id = messages.session_id;

DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;

CREATE TABLE IF NOT EXISTS parts_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    part_type TEXT NOT NULL,
    text TEXT,
    tool_name TEXT,
    tool_call_id TEXT,
    tool_arguments TEXT,
    tool_result TEXT,
    tool_error TEXT,
    tool_status TEXT,
    file_url TEXT,
    file_filename TEXT,
    file_mime TEXT,
    reasoning TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    data TEXT,
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE ON UPDATE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE ON UPDATE CASCADE
);

INSERT INTO parts_new (
    id, message_id, session_id, created_at, part_type,
    text, tool_name, tool_call_id, tool_arguments,
    tool_result, tool_error, tool_status,
    file_url, file_filename, file_mime,
    reasoning, sort_order, data
)
SELECT
    parts.pk,
    messages.pk,
    sessions.pk,
    parts.created_at,
    parts.part_type,
    text, tool_name, tool_call_id, tool_arguments,
    tool_result, tool_error, tool_status,
    file_url, file_filename, file_mime,
    reasoning, sort_order, data
FROM parts
JOIN messages ON messages.id = parts.message_id
JOIN sessions ON sessions.id = parts.session_id;

DROP TABLE parts;
ALTER TABLE parts_new RENAME TO parts;

CREATE TABLE IF NOT EXISTS todos_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    priority TEXT NOT NULL DEFAULT 'medium',
    position INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE ON UPDATE CASCADE
);

INSERT INTO todos_new (
    id, session_id, content, status, priority, position, created_at, updated_at
)
SELECT
    todos.pk,
    sessions.pk,
    todos.content,
    todos.status,
    todos.priority,
    todos.position,
    todos.created_at,
    todos.updated_at
FROM todos
JOIN sessions ON sessions.id = todos.session_id;

DROP TABLE todos;
ALTER TABLE todos_new RENAME TO todos;

CREATE TABLE IF NOT EXISTS session_shares_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL UNIQUE,
    share_id TEXT NOT NULL,
    secret TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE ON UPDATE CASCADE
);

INSERT INTO session_shares_new (id, session_id, share_id, secret, url, created_at)
SELECT
    session_shares.pk,
    sessions.pk,
    session_shares.id,
    session_shares.secret,
    session_shares.url,
    session_shares.created_at
FROM session_shares
JOIN sessions ON sessions.id = session_shares.session_id;

DROP TABLE session_shares;
ALTER TABLE session_shares_new RENAME TO session_shares;

DROP INDEX IF EXISTS idx_sessions_project;
DROP INDEX IF EXISTS idx_sessions_parent;
DROP INDEX IF EXISTS idx_sessions_updated;
DROP INDEX IF EXISTS idx_sessions_status;
DROP INDEX IF EXISTS idx_sessions_directory_updated;
DROP INDEX IF EXISTS idx_messages_session;
DROP INDEX IF EXISTS idx_messages_created;
DROP INDEX IF EXISTS idx_messages_session_created;
DROP INDEX IF EXISTS idx_parts_message;
DROP INDEX IF EXISTS idx_parts_session;
DROP INDEX IF EXISTS idx_parts_order;
DROP INDEX IF EXISTS idx_parts_message_sort;
DROP INDEX IF EXISTS idx_parts_session_sort;
DROP INDEX IF EXISTS idx_todos_session;
DROP INDEX IF EXISTS idx_todos_status;
DROP INDEX IF EXISTS idx_todos_session_position;

CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_id);
CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at);
CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_directory_updated ON sessions(directory, updated_at);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
CREATE INDEX IF NOT EXISTS idx_messages_session_created ON messages(session_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_parts_message ON parts(message_id);
CREATE INDEX IF NOT EXISTS idx_parts_session ON parts(session_id);
CREATE INDEX IF NOT EXISTS idx_parts_order ON parts(sort_order);
CREATE INDEX IF NOT EXISTS idx_parts_message_sort ON parts(message_id, sort_order, created_at, id);
CREATE INDEX IF NOT EXISTS idx_parts_session_sort ON parts(session_id, sort_order, created_at, id);

CREATE INDEX IF NOT EXISTS idx_todos_session ON todos(session_id);
CREATE INDEX IF NOT EXISTS idx_todos_status ON todos(status);
CREATE INDEX IF NOT EXISTS idx_todos_session_position ON todos(session_id, position, id);

PRAGMA foreign_keys=ON;
"#,
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible data-shape migration in SQLite.
        Ok(())
    }
}
