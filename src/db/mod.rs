use crate::types::TelegramId;
use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use tracing::trace;

pub mod schema;
use schema::*;

#[derive(Clone)]
pub struct Database {
    pub pool: Pool<Sqlite>,
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

impl Database {
    pub async fn new(db_filename: &str) -> Result<Self> {
        let db_url = format!("sqlite://{}", db_filename);

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
        let db = Self { pool };
        Ok(db)
    }

    pub async fn is_telegram_registered(&self, tg_id: TelegramId) -> Result<bool> {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) FROM telegram_registrations WHERE telegram_id = ?",
            tg_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

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

    pub async fn delete_registration(&self, tg_id: TelegramId) -> Result<bool> {
        let res = sqlx::query!(
            "DELETE FROM telegram_registrations WHERE telegram_id = ?",
            tg_id
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_all_registrations(&self) -> Result<Vec<TelegramRegistration>> {
        let users = sqlx::query_as!(
            TelegramRegistration,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username!: String\" FROM telegram_registrations"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

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

    pub async fn delete_pending_registration(&self, key: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM pending_telegram_registrations WHERE request_key = ?",
            key
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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

    pub async fn get_all_banned_users(&self) -> Result<Vec<BannedUser>> {
        let users = sqlx::query_as!(
            BannedUser,
            "SELECT telegram_id as \"telegram_id!: TelegramId\", teamtalk_username as \"teamtalk_username?: String\", banned_at as \"banned_at!: chrono::NaiveDateTime\", banned_by_admin_id as \"banned_by_admin_id?: TelegramId\", reason as \"reason?: String\" FROM banned_users ORDER BY banned_at DESC"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    pub async fn ban_user(
        &self,
        tg_id: TelegramId,
        tt_username: Option<&str>,
        admin_id: Option<TelegramId>,
        reason: Option<&str>,
    ) -> Result<()> {
        trace!(
            tg_id = %tg_id,
            admin_id = ?admin_id.map(|id| id.as_i64()),
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

    pub async fn unban_user(&self, tg_id: TelegramId) -> Result<bool> {
        let res = sqlx::query!("DELETE FROM banned_users WHERE telegram_id = ?", tg_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn is_ip_registered(&self, ip: &str) -> Result<bool> {
        let count: i64 = sqlx::query_scalar!(
            "SELECT count(*) FROM fastapi_registered_ips WHERE ip_address = ?",
            ip
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

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

    pub async fn mark_token_used(&self, token: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE fastapi_download_tokens SET is_used = 1 WHERE token = ?",
            token
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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

    pub async fn mark_deeplink_used(&self, token: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE deeplink_tokens SET is_used = 1 WHERE token = ?",
            token
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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
        let pending_ttl = format!("-{} seconds", pending_reg_ttl_seconds);
        let ip_ttl = format!("-{} seconds", registered_ip_ttl_seconds);
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

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
