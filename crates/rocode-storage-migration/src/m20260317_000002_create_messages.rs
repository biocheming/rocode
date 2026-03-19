use sea_orm_migration::prelude::*;

use crate::idents::{Messages, Sessions};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000002_create_messages"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Messages::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Messages::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Messages::SessionId).big_integer().not_null())
                    .col(ColumnDef::new(Messages::Role).string().not_null())
                    .col(ColumnDef::new(Messages::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Messages::ProviderId).string())
                    .col(ColumnDef::new(Messages::ModelId).string())
                    .col(
                        ColumnDef::new(Messages::TokensInput)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Messages::TokensOutput)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Messages::TokensReasoning)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Messages::TokensCacheRead)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Messages::TokensCacheWrite)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Messages::Cost)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(ColumnDef::new(Messages::Finish).string())
                    .col(ColumnDef::new(Messages::Metadata).string())
                    .col(ColumnDef::new(Messages::Data).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_messages_session")
                            .from(Messages::Table, Messages::SessionId)
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
            .drop_table(Table::drop().table(Messages::Table).to_owned())
            .await
    }
}
