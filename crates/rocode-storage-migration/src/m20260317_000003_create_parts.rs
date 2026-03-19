use sea_orm_migration::prelude::*;

use crate::idents::{Messages, Parts, Sessions};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000003_create_parts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Parts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Parts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Parts::MessageId).big_integer().not_null())
                    .col(ColumnDef::new(Parts::SessionId).big_integer().not_null())
                    .col(ColumnDef::new(Parts::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Parts::PartType).string().not_null())
                    .col(ColumnDef::new(Parts::Text).string())
                    .col(ColumnDef::new(Parts::ToolName).string())
                    .col(ColumnDef::new(Parts::ToolCallId).string())
                    .col(ColumnDef::new(Parts::ToolArguments).string())
                    .col(ColumnDef::new(Parts::ToolResult).string())
                    .col(ColumnDef::new(Parts::ToolError).string())
                    .col(ColumnDef::new(Parts::ToolStatus).string())
                    .col(ColumnDef::new(Parts::FileUrl).string())
                    .col(ColumnDef::new(Parts::FileFilename).string())
                    .col(ColumnDef::new(Parts::FileMime).string())
                    .col(ColumnDef::new(Parts::Reasoning).string())
                    .col(
                        ColumnDef::new(Parts::SortOrder)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Parts::Data).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_parts_message")
                            .from(Parts::Table, Parts::MessageId)
                            .to(Messages::Table, Messages::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_parts_session")
                            .from(Parts::Table, Parts::SessionId)
                            .to(Sessions::Table, Sessions::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Parts::Table).to_owned())
            .await
    }
}
