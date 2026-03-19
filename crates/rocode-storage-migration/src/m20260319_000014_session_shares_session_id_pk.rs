use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260319_000014_session_shares_session_id_pk"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        let has_id = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT 1 FROM pragma_table_info('session_shares') WHERE name = 'id' LIMIT 1"
                    .to_string(),
            ))
            .await?
            .is_some();

        if !has_id {
            // Already uses session_id as the primary key.
            return Ok(());
        }

        conn.execute_unprepared(
            r#"
PRAGMA foreign_keys=OFF;

CREATE TABLE IF NOT EXISTS session_shares_new (
    session_id INTEGER PRIMARY KEY,
    share_id TEXT NOT NULL,
    secret TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE ON UPDATE CASCADE
);

INSERT INTO session_shares_new (session_id, share_id, secret, url, created_at)
SELECT session_id, share_id, secret, url, created_at
FROM session_shares;

DROP TABLE session_shares;
ALTER TABLE session_shares_new RENAME TO session_shares;

PRAGMA foreign_keys=ON;
"#,
        )
        .await
        .map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible in SQLite without data-shape downgrade.
        Ok(())
    }
}
