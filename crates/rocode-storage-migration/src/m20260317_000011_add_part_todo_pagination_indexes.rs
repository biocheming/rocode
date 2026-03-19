use sea_orm_migration::prelude::*;

use crate::idents::{Parts, Todos};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000011_add_part_todo_pagination_indexes"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Support common paging queries:
        // - list parts by message ordered by sort_order/created_at/id
        // - list parts by session ordered by sort_order/created_at/id
        // - list todos by session ordered by position/id
        manager
            .create_index(
                Index::create()
                    .name("idx_parts_message_sort")
                    .table(Parts::Table)
                    .col(Parts::MessageId)
                    .col(Parts::SortOrder)
                    .col(Parts::CreatedAt)
                    .col(Parts::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_parts_session_sort")
                    .table(Parts::Table)
                    .col(Parts::SessionId)
                    .col(Parts::SortOrder)
                    .col(Parts::CreatedAt)
                    .col(Parts::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_todos_session_position")
                    .table(Todos::Table)
                    .col(Todos::SessionId)
                    .col(Todos::Position)
                    .col(Todos::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_parts_message_sort")
                    .table(Parts::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_parts_session_sort")
                    .table(Parts::Table)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .drop_index(
                Index::drop()
                    .name("idx_todos_session_position")
                    .table(Todos::Table)
                    .to_owned(),
            )
            .await;
        Ok(())
    }
}
