use sea_orm_migration::prelude::*;

use crate::idents::{Messages, Sessions};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000010_add_pagination_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Support common server queries:
        // - list sessions by directory ordered by updated_at
        // - list messages in a session ordered by created_at
        manager
            .create_index(
                Index::create()
                    .name("idx_sessions_directory_updated")
                    .table(Sessions::Table)
                    .col(Sessions::Directory)
                    .col(Sessions::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_messages_session_created")
                    .table(Messages::Table)
                    .col(Messages::SessionId)
                    .col(Messages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Best-effort cleanup: drop indexes.
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_sessions_directory_updated")
                    .table(Sessions::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_messages_session_created")
                    .table(Messages::Table)
                    .to_owned(),
            )
            .await;

        Ok(())
    }
}
