use sea_orm_migration::prelude::*;

use crate::idents::{SessionShares, Sessions};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000006_create_session_shares"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SessionShares::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SessionShares::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SessionShares::SessionId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(SessionShares::ShareId).string().not_null())
                    .col(ColumnDef::new(SessionShares::Secret).string().not_null())
                    .col(ColumnDef::new(SessionShares::Url).string().not_null())
                    .col(
                        ColumnDef::new(SessionShares::CreatedAt)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_session_shares_session")
                            .from(SessionShares::Table, SessionShares::SessionId)
                            .to(Sessions::Table, Sessions::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .index(
                        Index::create()
                            .name("ux_session_shares_session_id")
                            .col(SessionShares::SessionId)
                            .unique(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SessionShares::Table).to_owned())
            .await
    }
}
