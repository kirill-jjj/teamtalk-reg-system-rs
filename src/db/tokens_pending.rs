use super::Database;
use super::schema::{DeeplinkToken, FastapiDownloadToken, PendingTelegramRegistration};
use crate::types::TelegramId;
use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use tracing::instrument;

impl Database {
    /// `add_pending_registration` database operation.
    #[instrument(skip(self), err)]
    pub async fn add_pending_registration(
        &self,
        key: &str,
        tg_id: TelegramId,
        username: &str,
        password: &str,
        nickname: &str,
        source_info: &str,
    ) -> Result<()> {
        let encrypted_password = self.encryption.encrypt(password)?;
        sqlx::query(
            "INSERT INTO pending_telegram_registrations (request_key, registrant_telegram_id, username, password_encrypted, nickname, source_info) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(key)
        .bind(tg_id)
        .bind(username)
        .bind(encrypted_password)
        .bind(nickname)
        .bind(source_info)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `get_pending_registration` database operation.
    #[instrument(skip(self), err)]
    pub async fn get_pending_registration(
        &self,
        key: &str,
    ) -> Result<Option<PendingTelegramRegistration>> {
        let row = sqlx::query(
            "SELECT id, request_key, registrant_telegram_id, username, password_encrypted, nickname, source_info, created_at FROM pending_telegram_registrations WHERE request_key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let password_encrypted: String = row.try_get("password_encrypted")?;
        let password = self.encryption.decrypt(&password_encrypted)?;

        Ok(Some(PendingTelegramRegistration {
            id: row.try_get("id")?,
            request_key: row.try_get("request_key")?,
            registrant_telegram_id: row.try_get("registrant_telegram_id")?,
            username: row.try_get("username")?,
            password,
            nickname: row.try_get("nickname")?,
            source_info: row.try_get("source_info")?,
            created_at: row.try_get("created_at")?,
        }))
    }

    /// `delete_pending_registration` database operation.
    #[instrument(skip(self), err)]
    pub async fn delete_pending_registration(&self, key: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM pending_telegram_registrations WHERE request_key = ?",
            key
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `is_ip_registered` database operation.
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
    pub async fn mark_deeplink_used(&self, token: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE deeplink_tokens SET is_used = 1 WHERE token = ?",
            token
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
