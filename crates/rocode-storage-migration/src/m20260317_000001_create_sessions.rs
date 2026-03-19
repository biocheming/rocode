use sea_orm_migration::prelude::*;

use crate::idents::Sessions;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000001_create_sessions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Sessions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Sessions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Sessions::ProjectId)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Sessions::ParentId).big_integer())
                    .col(
                        ColumnDef::new(Sessions::Slug)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Sessions::Directory).string().not_null())
                    .col(ColumnDef::new(Sessions::Title).string().not_null())
                    .col(
                        ColumnDef::new(Sessions::Version)
                            .string()
                            .not_null()
                            .default("1.0.0"),
                    )
                    .col(ColumnDef::new(Sessions::ShareUrl).string())
                    .col(
                        ColumnDef::new(Sessions::SummaryAdditions)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::SummaryDeletions)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::SummaryFiles)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Sessions::SummaryDiffs).string())
                    .col(ColumnDef::new(Sessions::Revert).string())
                    .col(ColumnDef::new(Sessions::Permission).string())
                    .col(ColumnDef::new(Sessions::Metadata).string())
                    .col(
                        ColumnDef::new(Sessions::UsageInputTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::UsageOutputTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::UsageReasoningTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::UsageCacheWriteTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::UsageCacheReadTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sessions::UsageTotalCost)
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(
                        ColumnDef::new(Sessions::Status)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Sessions::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Sessions::UpdatedAt).big_integer().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Sessions::Table).to_owned())
            .await
    }
}
