use sea_orm_migration::prelude::*;

use crate::idents::Permissions;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000005_create_permissions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Permissions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Permissions::ProjectId)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Permissions::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Permissions::UpdatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Permissions::Data).string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Permissions::Table).to_owned())
            .await
    }
}

