use crate::security::crypto::EncryptionService;
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use tracing::instrument;

mod maintenance;
mod registration_records;
/// Database schema row types.
pub mod schema;
mod tokens_pending;

/// Database access layer.
#[derive(Clone)]
pub struct Database {
    pub pool: Pool<Sqlite>,
    encryption: EncryptionService,
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

impl Database {
    /// `new` database operation.
    pub async fn new(db_filename: &str, encryption: EncryptionService) -> Result<Self> {
        let db_url = format!("sqlite://{db_filename}");

        if !Path::new(db_filename).exists() {
            if let Some(parent) = Path::new(db_filename).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(db_filename)?;
        }

        let connect_options = SqliteConnectOptions::from_str(&db_url)?
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    sqlx::query("PRAGMA journal_mode = WAL;")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA synchronous = NORMAL;")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA foreign_keys = ON;")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA temp_store = MEMORY;")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(connect_options)
            .await?;

        MIGRATOR.run(&pool).await?;
        maintenance::integrity_check(&pool).await?;
        maintenance::validate_schema(&pool).await?;
        let db = Self { pool, encryption };
        maintenance::migrate_pending_passwords_online(&db).await?;
        Ok(db)
    }

    /// `cleanup` database operation.
    #[instrument(skip(self), err)]
    pub async fn cleanup(
        &self,
        pending_reg_ttl_seconds: u64,
        registered_ip_ttl_seconds: u64,
    ) -> Result<()> {
        maintenance::cleanup(self, pending_reg_ttl_seconds, registered_ip_ttl_seconds).await
    }

    /// `close` database operation.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
