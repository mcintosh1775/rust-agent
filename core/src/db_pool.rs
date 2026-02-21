use crate::{detect_storage_backend, StorageBackend};
use anyhow::{Context, Result};
use sqlx::{
    postgres::PgPoolOptions,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    PgPool, SqlitePool,
};
use std::str::FromStr;

#[derive(Clone)]
pub enum DbPool {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

impl DbPool {
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self> {
        let backend = detect_storage_backend(database_url)
            .context("DATABASE_URL storage backend detection failed")?;
        match backend {
            StorageBackend::Postgres => {
                let pool = PgPoolOptions::new()
                    .max_connections(max_connections)
                    .connect(database_url)
                    .await
                    .context("failed to connect to Postgres")?;
                Ok(Self::Postgres(pool))
            }
            StorageBackend::Sqlite => {
                let options = SqliteConnectOptions::from_str(database_url)
                    .context("failed parsing sqlite DATABASE_URL")?
                    .create_if_missing(true)
                    .foreign_keys(true);
                let pool = SqlitePoolOptions::new()
                    .max_connections(max_connections.max(1))
                    .connect_with(options)
                    .await
                    .context("failed to connect to SQLite")?;
                Ok(Self::Sqlite(pool))
            }
        }
    }

    pub fn backend(&self) -> StorageBackend {
        match self {
            Self::Postgres(_) => StorageBackend::Postgres,
            Self::Sqlite(_) => StorageBackend::Sqlite,
        }
    }

    pub async fn migrate(&self) -> Result<()> {
        match self {
            Self::Postgres(pool) => {
                sqlx::migrate!("../migrations")
                    .run(pool)
                    .await
                    .context("failed to run Postgres migrations")?;
            }
            Self::Sqlite(pool) => {
                sqlx::migrate!("../migrations/sqlite")
                    .run(pool)
                    .await
                    .context("failed to run SQLite migrations")?;
            }
        }
        Ok(())
    }
}
