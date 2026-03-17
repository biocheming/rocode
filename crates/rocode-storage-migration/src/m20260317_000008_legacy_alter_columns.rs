use sea_orm_migration::prelude::*;

use crate::idents::{Messages, Sessions};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260317_000008_legacy_alter_columns"
    }
}

fn is_duplicate_column_err(err: &DbErr) -> bool {
    let msg = err.to_string();
    msg.contains("duplicate column") || msg.contains("already exists")
}

async fn alter_ignoring_duplicate(
    manager: &SchemaManager<'_>,
    stmt: TableAlterStatement,
) -> Result<(), DbErr> {
    match manager.alter_table(stmt).await {
        Ok(()) => Ok(()),
        Err(err) if is_duplicate_column_err(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // These columns may be missing in older local DBs; adding them is safe for new DBs
        // because we ignore "duplicate column" errors.
        alter_ignoring_duplicate(
            manager,
            Table::alter()
                .table(Messages::Table)
                .add_column(ColumnDef::new(Messages::Finish).string())
                .to_owned(),
        )
        .await?;

        alter_ignoring_duplicate(
            manager,
            Table::alter()
                .table(Sessions::Table)
                .add_column(ColumnDef::new(Sessions::Metadata).string())
                .to_owned(),
        )
        .await?;

        alter_ignoring_duplicate(
            manager,
            Table::alter()
                .table(Messages::Table)
                .add_column(ColumnDef::new(Messages::Metadata).string())
                .to_owned(),
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot drop columns in-place; keep as no-op.
        Ok(())
    }
}
