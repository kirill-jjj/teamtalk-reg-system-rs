use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use unic_langid::LanguageIdentifier;

/// Telegram user info stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UserInfo {
    pub username: String,
    pub nickname: String,
    pub telegram_id: Option<TelegramId>,
}

/// Online `TeamTalk` user info.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OnlineUser {
    pub id: i32,
    pub nickname: String,
    pub username: String,
    pub channel_id: i32,
    pub user_type: u8,
}

/// Telegram user identifier wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct TelegramId(i64);

impl TelegramId {
    /// Create a new `TelegramId`.
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// Return the raw identifier value.
    pub const fn as_i64(self) -> i64 {
        self.0
    }
}

impl fmt::Display for TelegramId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for TelegramId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<TelegramId> for i64 {
    fn from(value: TelegramId) -> Self {
        value.0
    }
}

/// Language code wrapper (BCP-47/Fluent).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageCode(String);

impl LanguageCode {
    /// Parse a language code, returning `None` if empty or invalid.
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            let langid = LanguageIdentifier::from_str(trimmed).ok()?;
            Some(Self(langid.to_string()))
        }
    }

    /// Parse a language code or fall back to default.
    pub fn parse_or_default(input: &str) -> Self {
        Self::parse(input).unwrap_or_default()
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LanguageCode {
    fn default() -> Self {
        Self("en".to_string())
    }
}

impl fmt::Display for LanguageCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for LanguageCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanguageCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).ok_or_else(|| serde::de::Error::custom("invalid language code"))
    }
}

/// Origin of a registration request.
#[derive(Debug, Clone)]
pub enum RegistrationSource {
    Telegram(TelegramId),
    Web(IpAddr),
}

/// `TeamTalk` account type.
#[derive(Debug, Clone, Copy)]
pub enum TTAccountType {
    Default,
    Admin,
}

/// Type of downloadable asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadTokenType {
    TtConfig,
    ClientZip,
}

impl DownloadTokenType {
    /// Convert token type to its storage string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TtConfig => "tt_config",
            Self::ClientZip => "client_zip",
        }
    }
}

impl TryFrom<&str> for DownloadTokenType {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "tt_config" => Ok(Self::TtConfig),
            "client_zip" => Ok(Self::ClientZip),
            _ => Err(()),
        }
    }
}

/// Commands for the `TeamTalk` worker thread.
#[derive(Debug)]
pub enum TTWorkerCommand {
    CreateAccount {
        username: crate::domain::Username,
        password: crate::domain::Password,
        nickname: crate::domain::Nickname,
        account_type: TTAccountType,
        source: RegistrationSource,
        source_info: Option<String>,
        resp: tokio::sync::oneshot::Sender<Result<bool, String>>,
    },
    CheckUserExists {
        username: crate::domain::Username,
        resp: tokio::sync::oneshot::Sender<bool>,
    },
    #[allow(dead_code)]
    GetOnlineUsers {
        resp: tokio::sync::oneshot::Sender<Vec<OnlineUser>>,
    },
    GetAllUsers {
        resp: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    DeleteUser {
        username: crate::domain::Username,
        resp: tokio::sync::oneshot::Sender<Result<bool, String>>,
    },
}
