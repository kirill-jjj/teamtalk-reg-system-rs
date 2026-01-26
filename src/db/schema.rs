use crate::types::TelegramId;
use chrono::NaiveDateTime;
use sqlx::FromRow;

/// Row for Telegram registrations table.
#[derive(Debug, FromRow)]
pub struct TelegramRegistration {
    pub telegram_id: TelegramId,
    pub teamtalk_username: String,
}

/// Row for pending registration table.
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct PendingTelegramRegistration {
    pub id: Option<i64>,
    pub request_key: String,
    pub registrant_telegram_id: TelegramId,
    pub username: String,
    pub password_cleartext: String,
    pub nickname: String,
    pub source_info: String,
    pub created_at: NaiveDateTime,
}

/// Row for banned users table.
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct BannedUser {
    pub telegram_id: TelegramId,
    pub teamtalk_username: Option<String>,
    pub banned_at: NaiveDateTime,
    pub banned_by_admin_id: Option<TelegramId>,
    pub reason: Option<String>,
}

/// Row for download tokens table.
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct FastapiDownloadToken {
    pub token: String,
    pub filepath_on_server: String,
    pub original_filename: String,
    pub token_type: String,
    pub created_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub is_used: bool,
}

/// Row for registered IP table.
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct FastapiRegisteredIp {
    pub ip_address: String,
    pub username: Option<String>,
    pub registration_timestamp: NaiveDateTime,
}

/// Row for deeplink tokens table.
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct DeeplinkToken {
    pub id: Option<i64>,
    pub token: String,
    pub created_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub is_used: bool,
    pub generated_by_admin_id: Option<i64>,
}
