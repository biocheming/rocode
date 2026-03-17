use sea_orm_migration::prelude::*;

use crate::idents::{Messages, Parts, Sessions, Todos};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000007_create_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Session indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_sessions_project")
                    .table(Sessions::Table)
                    .col(Sessions::ProjectId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sessions_parent")
                    .table(Sessions::Table)
                    .col(Sessions::ParentId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sessions_updated")
                    .table(Sessions::Table)
                    .col(Sessions::UpdatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_sessions_status")
                    .table(Sessions::Table)
                    .col(Sessions::Status)
                    .to_owned(),
            )
            .await?;

        // Message indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_session")
                    .table(Messages::Table)
                    .col(Messages::SessionId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_messages_created")
                    .table(Messages::Table)
                    .col(Messages::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // Part indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_parts_message")
                    .table(Parts::Table)
                    .col(Parts::MessageId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_parts_session")
                    .table(Parts::Table)
                    .col(Parts::SessionId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_parts_order")
                    .table(Parts::Table)
                    .col(Parts::SortOrder)
                    .to_owned(),
            )
            .await?;

        // Todo indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_todos_session")
                    .table(Todos::Table)
                    .col(Todos::SessionId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_todos_status")
                    .table(Todos::Table)
                    .col(Todos::Status)
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
                    .name("idx_sessions_project")
                    .table(Sessions::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_sessions_parent")
                    .table(Sessions::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_sessions_updated")
                    .table(Sessions::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_sessions_status")
                    .table(Sessions::Table)
                    .to_owned(),
            )
            .await;

        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_messages_session")
                    .table(Messages::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_messages_created")
                    .table(Messages::Table)
                    .to_owned(),
            )
            .await;

        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_parts_message")
                    .table(Parts::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_parts_session")
                    .table(Parts::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_parts_order")
                    .table(Parts::Table)
                    .to_owned(),
            )
            .await;

        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_todos_session")
                    .table(Todos::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_todos_status")
                    .table(Todos::Table)
                    .to_owned(),
            )
            .await;
        Ok(())
    }
}
