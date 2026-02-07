use crate::types::LanguageCode;
use crate::types::TelegramId;
use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Application configuration loaded from TOML.
#[derive(Clone, Deserialize, Debug)]
pub struct AppConfig {
    #[serde(flatten)]
    pub telegram: TelegramConfig,
    #[serde(flatten)]
    pub teamtalk: TeamTalkConfig,
    #[serde(flatten)]
    pub web: WebConfig,
    #[serde(flatten)]
    pub database: DatabaseConfig,
    #[serde(flatten)]
    pub logging: LoggingConfig,
}

/// Telegram and admin settings.
#[derive(Clone, Deserialize, Debug)]
pub struct TelegramConfig {
    pub tg_bot_token: String,
    #[serde(default)]
    pub admin_ids: Vec<TelegramId>,
    #[serde(default = "default_lang")]
    pub bot_admin_lang: LanguageCode,
    #[serde(default)]
    pub verify_registration: bool,
    #[serde(default)]
    pub telegram_deeplink_registration_enabled: bool,
    #[serde(default = "default_true")]
    pub telegram_public_registration_enabled: bool,
}

/// `TeamTalk` server settings.
#[derive(Clone, Deserialize, Debug)]
pub struct TeamTalkConfig {
    pub host_name: String,
    #[serde(rename = "port")]
    pub tcp_port: i32,
    pub udp_port: Option<i32>,
    pub user_name: String,
    pub password: String,
    #[serde(default = "default_nickname")]
    pub nick_name: String,
    #[serde(default = "default_client_name")]
    pub client_name: String,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(default = "default_server_name")]
    pub server_name: String,
    #[serde(default)]
    pub tt_public_hostname: Option<String>,
    #[serde(default)]
    pub tt_join_channel: Option<String>,
    #[serde(default)]
    pub tt_join_channel_password: Option<String>,
    #[serde(default = "default_status")]
    pub tt_status_text: String,
    #[serde(default = "default_gender")]
    pub tt_gender: String,
    #[serde(default)]
    pub teamtalk_default_user_rights: Vec<String>,
    #[serde(default = "default_true")]
    pub teamtalk_registration_broadcast_enabled: bool,
}

/// Web server settings.
#[derive(Clone, Deserialize, Debug)]
pub struct WebConfig {
    #[serde(default)]
    pub web_registration_enabled: bool,
    #[serde(default = "default_host")]
    pub web_app_host: String,
    #[serde(default = "default_port")]
    pub web_app_port: u16,
    #[serde(default)]
    pub web_app_ssl_enabled: bool,
    #[serde(default)]
    pub web_app_ssl_cert_path: Option<String>,
    #[serde(default)]
    pub web_app_ssl_key_path: Option<String>,
    #[serde(default)]
    pub root_path: String,
    #[serde(default)]
    pub web_app_proxy_headers: bool,
    #[serde(default = "default_forwarded_allow_ips")]
    pub web_app_forwarded_allow_ips: String,
    #[serde(default, deserialize_with = "deserialize_optional_lang")]
    pub force_user_lang: Option<LanguageCode>,
    pub teamtalk_client_template_dir: Option<String>,
}

/// Database and file generation settings.
#[derive(Clone, Deserialize, Debug)]
pub struct DatabaseConfig {
    #[serde(default = "default_ttl")]
    pub generated_file_ttl_seconds: u64,
    #[serde(default = "default_db_name")]
    pub db_name: String,
    #[serde(default = "default_cleanup")]
    pub db_cleanup_interval_seconds: u64,
    #[serde(default = "default_pending_ttl")]
    pub pending_reg_ttl_seconds: u64,
    #[serde(default = "default_registered_ip_ttl")]
    pub registered_ip_ttl_seconds: u64,
}

/// Logging settings.
#[derive(Clone, Deserialize, Debug)]
pub struct LoggingConfig {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub log_level: Option<String>,
}

fn deserialize_optional_lang<'de, D>(deserializer: D) -> Result<Option<LanguageCode>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.and_then(|s| LanguageCode::parse(&s)))
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }))
}

fn default_nickname() -> String {
    "RegisterBot".to_string()
}
fn default_client_name() -> String {
    "PyTalkRegisterBot".to_string()
}
fn default_server_name() -> String {
    "TeamTalk Server".to_string()
}
const fn default_status() -> String {
    String::new()
}
fn default_gender() -> String {
    "neutral".to_string()
}
fn default_lang() -> LanguageCode {
    LanguageCode::default()
}
fn default_host() -> String {
    "0.0.0.0".to_string()
}
const fn default_port() -> u16 {
    5000
}
fn default_forwarded_allow_ips() -> String {
    "*".to_string()
}
const fn default_ttl() -> u64 {
    600
}
fn default_db_name() -> String {
    "users.db".to_string()
}
const fn default_cleanup() -> u64 {
    3600
}
const fn default_true() -> bool {
    true
}
const fn default_pending_ttl() -> u64 {
    604_800
}
const fn default_registered_ip_ttl() -> u64 {
    2_592_000
}

impl AppConfig {
    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;

        if config.teamtalk.udp_port.is_none() {
            config.teamtalk.udp_port = Some(config.teamtalk.tcp_port);
        }
        Ok(config)
    }

    /// Resolve the database path relative to the config file.
    pub fn get_db_path(&self, config_path: &Path) -> PathBuf {
        let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
        parent.join(&self.database.db_name)
    }
}
