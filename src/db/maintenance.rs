use super::Database;
use crate::security::crypto::EncryptionService;
use anyhow::Result;
use chrono::Utc;
use sqlx::{AssertSqlSafe, Pool, Row, Sqlite};
use std::collections::HashSet;
use tracing::{error, info, instrument, trace};

#[instrument(skip(db), err)]
pub(super) async fn cleanup(
    db: &Database,
    pending_reg_ttl_seconds: u64,
    registered_ip_ttl_seconds: u64,
) -> Result<()> {
    trace!(
        pending_reg_ttl_seconds,
        registered_ip_ttl_seconds, "Running db cleanup"
    );
    let now = Utc::now().naive_utc();
    sqlx::query!(
        "DELETE FROM fastapi_download_tokens WHERE expires_at < ? OR is_used = 1",
        now
    )
    .execute(&db.pool)
    .await?;
    sqlx::query!(
        "DELETE FROM deeplink_tokens WHERE expires_at < ? OR is_used = 1",
        now
    )
    .execute(&db.pool)
    .await?;
    let pending_ttl = format!("-{pending_reg_ttl_seconds} seconds");
    let ip_ttl = format!("-{registered_ip_ttl_seconds} seconds");
    sqlx::query!(
        "DELETE FROM pending_telegram_registrations WHERE created_at < datetime('now', ?)",
        pending_ttl
    )
    .execute(&db.pool)
    .await?;
    sqlx::query!(
        "DELETE FROM fastapi_registered_ips WHERE registration_timestamp < datetime('now', ?)",
        ip_ttl
    )
    .execute(&db.pool)
    .await?;

    sqlx::query("PRAGMA optimize;").execute(&db.pool).await?;

    Ok(())
}

pub(super) async fn migrate_pending_passwords_online(db: &Database) -> Result<()> {
    migrate_table_passwords(
        &db.pool,
        &db.encryption,
        "pending_telegram_registrations",
        "id",
    )
    .await?;
    migrate_table_passwords(&db.pool, &db.encryption, "pending_web_registrations", "id").await?;
    Ok(())
}

pub(super) async fn integrity_check(pool: &Pool<Sqlite>) -> Result<()> {
    let result: String = sqlx::query_scalar("PRAGMA integrity_check;")
        .fetch_one(pool)
        .await?;
    if result.trim() == "ok" {
        info!("Database integrity check: ok");
        Ok(())
    } else {
        error!(result = %result, "Database integrity check failed");
        anyhow::bail!("Database integrity check failed: {result}");
    }
}

pub(super) async fn validate_schema(pool: &Pool<Sqlite>) -> Result<()> {
    let tables: Vec<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map(|row: sqlx::sqlite::SqliteRow| row.get::<String, _>("name"))
        .fetch_all(pool)
        .await?;
    let present: HashSet<String> = tables.into_iter().collect();
    let required_tables = [
        "telegram_registrations",
        "pending_telegram_registrations",
        "pending_web_registrations",
        "banned_users",
        "fastapi_download_tokens",
        "fastapi_registered_ips",
        "deeplink_tokens",
        "_sqlx_migrations",
    ];
    for table in &required_tables {
        if !present.contains(*table) {
            anyhow::bail!("Database schema missing table: {table}");
        }
    }

    ensure_columns(
        pool,
        "pending_telegram_registrations",
        &[
            "id",
            "request_key",
            "registrant_telegram_id",
            "username",
            "password_encrypted",
            "nickname",
            "source_info",
            "created_at",
        ],
    )
    .await?;

    ensure_columns(
        pool,
        "pending_web_registrations",
        &[
            "id",
            "request_key",
            "username",
            "password_encrypted",
            "nickname",
            "ip_address",
            "user_agent",
            "source_info",
            "created_at",
        ],
    )
    .await?;

    Ok(())
}

async fn ensure_columns(pool: &Pool<Sqlite>, table: &str, expected: &[&str]) -> Result<()> {
    let rows = sqlx::query(AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(pool)
        .await?;
    let mut present = HashSet::new();
    for row in rows {
        let name: String = row.get("name");
        present.insert(name);
    }
    for col in expected {
        if !present.contains(*col) {
            anyhow::bail!("Table {table} missing column: {col}");
        }
    }
    Ok(())
}

async fn migrate_table_passwords(
    pool: &Pool<Sqlite>,
    encryption: &EncryptionService,
    table: &str,
    id_col: &str,
) -> Result<()> {
    let sql = format!("SELECT {id_col}, password_encrypted FROM {table}");
    let rows = sqlx::query(AssertSqlSafe(sql)).fetch_all(pool).await?;
    let mut migrated_count = 0_usize;

    for row in rows {
        let id: i64 = row.try_get(id_col)?;
        let value: String = row.try_get("password_encrypted")?;
        if EncryptionService::is_encrypted(&value) {
            continue;
        }
        let encrypted = encryption.encrypt(&value)?;
        let update_sql = format!("UPDATE {table} SET password_encrypted = ? WHERE {id_col} = ?");
        sqlx::query(AssertSqlSafe(update_sql))
            .bind(encrypted)
            .bind(id)
            .execute(pool)
            .await?;
        migrated_count += 1;
    }

    if migrated_count > 0 {
        info!(
            table,
            migrated_count, "Migrated legacy plaintext pending passwords"
        );
    }
    Ok(())
}
