use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, TransactionTrait};
use sea_orm_migration::MigratorTrait;
use std::future::Future;
use std::path::PathBuf;
use thiserror::Error;
use tracing::{info, warn};

use crate::StorageConnection;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Database connection error: {0}")]
    ConnectionError(String),

    #[error("Migration error: {0}")]
    MigrationError(String),

    #[error("Query error: {0}")]
    QueryError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),
}

pub struct Database {
    conn: StorageConnection,
}

impl Database {
    pub async fn new() -> Result<Self, DatabaseError> {
        let db_path = Self::get_database_path()?;

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DatabaseError::ConnectionError(e.to_string()))?;
        }

        info!("Connecting to database at {}", db_path.display());
        let db_url = Self::sqlite_url_for_path(&db_path);

        let conn = sea_orm::Database::connect(db_url)
            .await
            .map_err(|e| DatabaseError::ConnectionError(e.to_string()))?;

        // WAL mode allows concurrent reads during writes; NORMAL sync reduces fsync overhead.
        if let Err(e) = conn
            .execute(sea_orm::Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA journal_mode=WAL".to_string(),
            ))
            .await
        {
            warn!("failed to set journal_mode=WAL: {}", e);
        }
        if let Err(e) = conn
            .execute(sea_orm::Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA synchronous=NORMAL".to_string(),
            ))
            .await
        {
            warn!("failed to set synchronous=NORMAL: {}", e);
        }

        rocode_storage_migration::Migrator::up(&conn, None)
            .await
            .map_err(|e| DatabaseError::MigrationError(e.to_string()))?;

        Ok(Self { conn })
    }

    pub async fn in_memory() -> Result<Self, DatabaseError> {
        let conn = sea_orm::Database::connect("sqlite::memory:")
            .await
            .map_err(|e| DatabaseError::ConnectionError(e.to_string()))?;

        rocode_storage_migration::Migrator::up(&conn, None)
            .await
            .map_err(|e| DatabaseError::MigrationError(e.to_string()))?;

        Ok(Self { conn })
    }

    pub fn conn(&self) -> &StorageConnection {
        &self.conn
    }

    pub async fn begin(&self) -> Result<DatabaseTransaction, DatabaseError> {
        self.conn
            .begin()
            .await
            .map_err(|e| DatabaseError::TransactionError(e.to_string()))
    }

    pub async fn transaction<F, T, Fut>(&self, f: F) -> Result<T, DatabaseError>
    where
        F: FnOnce(&DatabaseTransaction) -> Fut,
        Fut: Future<Output = Result<T, DatabaseError>>,
    {
        let tx = self.begin().await?;
        let result = f(&tx).await?;
        tx.commit()
            .await
            .map_err(|e| DatabaseError::TransactionError(e.to_string()))?;
        Ok(result)
    }

    fn sqlite_url_for_path(db_path: &PathBuf) -> String {
        // SeaORM uses SQLx under the hood, but expects the URL form.
        // `sqlite:///abs/path.db?mode=rwc` for absolute paths.
        if db_path.is_absolute() {
            format!("sqlite://{}?mode=rwc", db_path.display())
        } else {
            format!("sqlite:{}?mode=rwc", db_path.display())
        }
    }

    fn get_database_path() -> Result<PathBuf, DatabaseError> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rocode");

        Ok(data_dir.join("rocode.db"))
    }
}
