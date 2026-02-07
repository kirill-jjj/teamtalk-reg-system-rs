use super::Database;
use super::schema::{BannedUser, TelegramRegistration};
use crate::types::TelegramId;
use anyhow::Result;
use chrono::Utc;
use tracing::{instrument, trace};

impl Database {
    /// `is_telegram_registered` database operation.
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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

    /// `get_banned_user` database operation.
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
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
    #[instrument(skip(self), err)]
    pub async fn unban_user(&self, tg_id: TelegramId) -> Result<bool> {
        let res = sqlx::query!("DELETE FROM banned_users WHERE telegram_id = ?", tg_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}
