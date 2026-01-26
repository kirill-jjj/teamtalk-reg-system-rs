use crate::types::TelegramId;
use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info, trace};

/// Database schema row types.
pub mod schema;
use schema::{
    BannedUser, DeeplinkToken, FastapiDownloadToken, PendingTelegramRegistration,
    TelegramRegistration,
};

/// Database access layer.
#[derive(Clone)]
pub struct Database {
    pub pool: Pool<Sqlite>,
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

impl Database {
    /// `new` database operation.
    pub async fn new(db_filename: &str) -> Result<Self> {
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
        integrity_check(&pool).await?;
        validate_schema(&pool).await?;
        let db = Self { pool };
        Ok(db)
    }

    /// `is_telegram_registered` database operation.
    pub async fn is_telegram_registered(&self, tg_id: TelegramId) -> Result<bool> {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) FROM telegram_registrations WHERE telegram_id = ?",
            tg_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    /// `add_registration` database operation.
    pub async fn add_registration(&self, tg_id: TelegramId, tt_username: &str) -> Result<()> {
        trace!(tg_id = %tg_id, tt_username, "Adding registration");
        sqlx::query!(
            "INSERT OR REPLACE INTO telegram_registrations (telegram_id, teamtalk_username) VALUES (?, ?)",
            tg_id,
            tt_username
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `delete_registration` database operation.
    pub async fn delete_registration(&self, tg_id: TelegramId) -> Result<bool> {
        let res = sqlx::query!(
            "DELETE FROM telegram_registrations WHERE telegram_id = ?",
            tg_id
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// `get_all_registrations` database operation.
    pub async fn get_all_registrations(&self) -> Result<Vec<TelegramRegistration>> {
        let users = sqlx::query_as!(
            TelegramRegistration,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username!: String\" FROM telegram_registrations"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    /// `get_registration_by_id` database operation.
    pub async fn get_registration_by_id(
        &self,
        tg_id: TelegramId,
    ) -> Result<Option<TelegramRegistration>> {
        let user = sqlx::query_as!(
            TelegramRegistration,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username!: String\" FROM telegram_registrations WHERE telegram_id = ?",
            tg_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// `get_registration_by_tt_username` database operation.
    pub async fn get_registration_by_tt_username(
        &self,
        tt_username: &str,
    ) -> Result<Option<TelegramRegistration>> {
        let user = sqlx::query_as!(
            TelegramRegistration,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username!: String\" FROM telegram_registrations WHERE teamtalk_username = ?",
            tt_username
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// `add_pending_registration` database operation.
    pub async fn add_pending_registration(
        &self,
        key: &str,
        tg_id: TelegramId,
        username: &str,
        password: &str,
        nickname: &str,
        source_info: &str,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO pending_telegram_registrations (request_key, registrant_telegram_id, username, password_cleartext, nickname, source_info) VALUES (?, ?, ?, ?, ?, ?)",
            key,
            tg_id,
            username,
            password,
            nickname,
            source_info
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `get_pending_registration` database operation.
    pub async fn get_pending_registration(
        &self,
        key: &str,
    ) -> Result<Option<PendingTelegramRegistration>> {
        let reg = sqlx::query_as!(
            PendingTelegramRegistration,
            "SELECT id as \"id?: i64\", request_key as \"request_key!: String\", registrant_telegram_id as \"registrant_telegram_id!: TelegramId\", username as \"username!: String\", password_cleartext as \"password_cleartext!: String\", nickname as \"nickname!: String\", source_info as \"source_info!: String\", created_at as \"created_at!: chrono::NaiveDateTime\" FROM pending_telegram_registrations WHERE request_key = ?",
            key
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(reg)
    }

    /// `delete_pending_registration` database operation.
    pub async fn delete_pending_registration(&self, key: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM pending_telegram_registrations WHERE request_key = ?",
            key
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `get_banned_user` database operation.
    pub async fn get_banned_user(&self, tg_id: TelegramId) -> Result<Option<BannedUser>> {
        let user = sqlx::query_as!(
            BannedUser,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username?: String\", banned_at as \"banned_at!: chrono::NaiveDateTime\", banned_by_admin_id as \"banned_by_admin_id?: TelegramId\", reason as \"reason?: String\" FROM banned_users WHERE telegram_id = ?",
            tg_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// `get_all_banned_users` database operation.
    pub async fn get_all_banned_users(&self) -> Result<Vec<BannedUser>> {
        let users = sqlx::query_as!(
            BannedUser,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username?: String\", banned_at as \"banned_at!: chrono::NaiveDateTime\", banned_by_admin_id as \"banned_by_admin_id?: TelegramId\", reason as \"reason?: String\" FROM banned_users ORDER BY banned_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    /// `ban_user` database operation.
    pub async fn ban_user(
        &self,
        tg_id: TelegramId,
        tt_username: Option<&str>,
        admin_id: Option<TelegramId>,
        reason: Option<&str>,
    ) -> Result<()> {
        trace!(
            tg_id = %tg_id,
            admin_id = ?admin_id.map(TelegramId::as_i64),
            "Banning user"
        );
        let now = Utc::now().naive_utc();
        sqlx::query!(
            "INSERT OR REPLACE INTO banned_users (telegram_id, teamtalk_username, banned_at, banned_by_admin_id, reason) VALUES (?, ?, ?, ?, ?)",
            tg_id,
            tt_username,
            now,
            admin_id,
            reason
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `unban_user` database operation.
    pub async fn unban_user(&self, tg_id: TelegramId) -> Result<bool> {
        let res = sqlx::query!("DELETE FROM banned_users WHERE telegram_id = ?", tg_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// `is_ip_registered` database operation.
    pub async fn is_ip_registered(&self, ip: &str) -> Result<bool> {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) FROM fastapi_registered_ips WHERE ip_address = ?",
            ip
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    /// `add_registered_ip` database operation.
    pub async fn add_registered_ip(&self, ip: &str, username: Option<&str>) -> Result<()> {
        let now = Utc::now().naive_utc();
        sqlx::query!(
            "INSERT INTO fastapi_registered_ips (ip_address, username, registration_timestamp) VALUES (?, ?, ?)",
            ip,
            username,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `add_download_token` database operation.
    pub async fn add_download_token(
        &self,
        token: &str,
        filepath: &str,
        original_name: &str,
        token_type: crate::types::DownloadTokenType,
        expires_at: chrono::NaiveDateTime,
    ) -> Result<()> {
        let now = Utc::now().naive_utc();
        let token_type_str = token_type.as_str();
        sqlx::query!(
            "INSERT INTO fastapi_download_tokens (token, filepath_on_server, original_filename, token_type, created_at, expires_at, is_used) VALUES (?, ?, ?, ?, ?, ?, 0)",
            token,
            filepath,
            original_name,
            token_type_str,
            now,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `get_download_token` database operation.
    pub async fn get_download_token(&self, token: &str) -> Result<Option<FastapiDownloadToken>> {
        let now = Utc::now().naive_utc();
        let tok = sqlx::query_as!(
            FastapiDownloadToken,
            "SELECT token as \"token!: String\", filepath_on_server as \"filepath_on_server!: String\", original_filename as \"original_filename!: String\", token_type as \"token_type!: String\", created_at as \"created_at!: chrono::NaiveDateTime\", expires_at as \"expires_at!: chrono::NaiveDateTime\", is_used as \"is_used!: bool\" FROM fastapi_download_tokens WHERE token = ? AND is_used = 0 AND expires_at > ?",
            token,
            now
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(tok)
    }

    /// `mark_token_used` database operation.
    pub async fn mark_token_used(&self, token: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE fastapi_download_tokens SET is_used = 1 WHERE token = ?",
            token
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `create_deeplink` database operation.
    pub async fn create_deeplink(
        &self,
        token: &str,
        expires_at: chrono::NaiveDateTime,
        admin_id: TelegramId,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO deeplink_tokens (token, expires_at, generated_by_admin_id, created_at) VALUES (?, ?, ?, datetime('now'))",
            token,
            expires_at,
            admin_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `get_valid_deeplink` database operation.
    pub async fn get_valid_deeplink(&self, token: &str) -> Result<Option<DeeplinkToken>> {
        let now = Utc::now().naive_utc();
        let token_obj = sqlx::query_as!(
            DeeplinkToken,
            "SELECT id as \"id?: i64\", token as \"token!: String\", created_at as \"created_at!: chrono::NaiveDateTime\", expires_at as \"expires_at!: chrono::NaiveDateTime\", is_used as \"is_used!: bool\", generated_by_admin_id as \"generated_by_admin_id?: i64\" FROM deeplink_tokens WHERE token = ? AND is_used = 0 AND expires_at > ?",
            token,
            now
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(token_obj)
    }

    /// `mark_deeplink_used` database operation.
    pub async fn mark_deeplink_used(&self, token: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE deeplink_tokens SET is_used = 1 WHERE token = ?",
            token
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `cleanup` database operation.
    pub async fn cleanup(
        &self,
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
        .execute(&self.pool)
        .await?;
        sqlx::query!(
            "DELETE FROM deeplink_tokens WHERE expires_at < ? OR is_used = 1",
            now
        )
        .execute(&self.pool)
        .await?;
        let pending_ttl = format!("-{pending_reg_ttl_seconds} seconds");
        let ip_ttl = format!("-{registered_ip_ttl_seconds} seconds");
        sqlx::query!(
            "DELETE FROM pending_telegram_registrations WHERE created_at < datetime('now', ?)",
            pending_ttl
        )
        .execute(&self.pool)
        .await?;
        sqlx::query!(
            "DELETE FROM fastapi_registered_ips WHERE registration_timestamp < datetime('now', ?)",
            ip_ttl
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("PRAGMA optimize;").execute(&self.pool).await?;

        Ok(())
    }

    /// `close` database operation.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

async fn integrity_check(pool: &Pool<Sqlite>) -> Result<()> {
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

async fn validate_schema(pool: &Pool<Sqlite>) -> Result<()> {
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
            "password_cleartext",
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
            "password_cleartext",
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
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
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
