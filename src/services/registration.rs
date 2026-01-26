use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::files::{create_client_zip, generate_tt_file_content, generate_tt_link};
use crate::types::{RegistrationSource, TTAccountType, TTWorkerCommand, TelegramId};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use tracing::{error, instrument};

/// Assets generated for a registration (tt file, link, filename).
pub struct RegistrationAssets {
    pub content: String,
    pub link: String,
    pub filename: String,
}

/// Result of `TeamTalk` account creation flow.
pub struct RegistrationResult {
    pub created: bool,
    pub db_sync_error: Option<String>,
    pub assets: Option<RegistrationAssets>,
}

/// Build registration assets from config and account fields.
pub fn build_assets(
    config: &AppConfig,
    username: &str,
    password: &str,
    nickname: &str,
) -> RegistrationAssets {
    let tt_content = generate_tt_file_content(config, username, password, nickname);
    let tt_link = generate_tt_link(config, username, password, nickname);
    let tt_filename = format!("{}.tt", config.teamtalk.server_name);

    RegistrationAssets {
        content: tt_content,
        link: tt_link,
        filename: tt_filename,
    }
}

/// Parameters for `TeamTalk` account creation.
pub struct CreateAccountParams<'a> {
    pub username: &'a Username,
    pub password: &'a Password,
    pub nickname: &'a Nickname,
    pub account_type: TTAccountType,
    pub source: RegistrationSource,
    pub source_info: Option<String>,
    pub telegram_id: Option<TelegramId>,
    pub tx_tt: Sender<TTWorkerCommand>,
    pub db: &'a Database,
    pub config: &'a AppConfig,
}

#[instrument(
    skip(params),
    fields(username = %params.username.as_str(), account_type = ?params.account_type)
)]
/// Create a `TeamTalk` account and sync DB metadata.
pub async fn create_teamtalk_account(
    params: CreateAccountParams<'_>,
) -> Result<RegistrationResult, Box<dyn Error + Send + Sync>> {
    let CreateAccountParams {
        username,
        password,
        nickname,
        account_type,
        source,
        source_info,
        telegram_id,
        tx_tt,
        db,
        config,
    } = params;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = TTWorkerCommand::CreateAccount {
        username: username.clone(),
        password: password.clone(),
        nickname: nickname.clone(),
        account_type,
        source,
        source_info,
        resp: tx,
    };
    if let Err(e) = tx_tt.send(cmd) {
        error!(error = %e, "Failed to send TeamTalk create command");
        return Err(Box::new(e));
    }

    let result = rx.await;
    match result {
        Ok(Ok(true)) => {
            let db_sync_error = if let Some(tg_id) = telegram_id
                && let Err(e) = db.add_registration(tg_id, username.as_str()).await
            {
                Some(e.to_string())
            } else {
                None
            };

            let assets = build_assets(
                config,
                username.as_str(),
                password.as_str(),
                nickname.as_str(),
            );
            Ok(RegistrationResult {
                created: true,
                db_sync_error,
                assets: Some(assets),
            })
        }
        Ok(Ok(false)) => {
            error!("TeamTalk create account returned false");
            Ok(RegistrationResult {
                created: false,
                db_sync_error: None,
                assets: None,
            })
        }
        Ok(Err(e)) => {
            error!(error = %e, "TeamTalk create account failed");
            Ok(RegistrationResult {
                created: false,
                db_sync_error: None,
                assets: None,
            })
        }
        Err(e) => {
            error!(error = %e, "TeamTalk create account response channel failed");
            Ok(RegistrationResult {
                created: false,
                db_sync_error: None,
                assets: None,
            })
        }
    }
}

/// Resolve temp directory used for generated files.
pub fn temp_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("temp_files")
}

/// Try to create a client ZIP if template is present.
pub async fn try_create_client_zip_async(
    config: &AppConfig,
    output_path: &Path,
    assets: &RegistrationAssets,
) -> bool {
    let Some(tpl_dir) = &config.web.teamtalk_client_template_dir else {
        return false;
    };
    if !Path::new(tpl_dir).exists() {
        return false;
    }

    let tpl_dir = tpl_dir.clone();
    let output_path = output_path.to_path_buf();
    let tt_filename = assets.filename.clone();
    let tt_content = assets.content.clone();

    tokio::task::spawn_blocking(move || {
        create_client_zip(&tpl_dir, &output_path, &tt_filename, &tt_content).is_ok()
    })
    .await
    .unwrap_or(false)
}
