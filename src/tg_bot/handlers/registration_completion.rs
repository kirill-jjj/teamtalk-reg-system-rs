use super::HandlerResult;
use super::registration::{notify_db_sync_error, send_registration_assets};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::registration;
use crate::types::{LanguageCode, RegistrationSource, TTAccountType, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide_ng::prelude::*;
use teloxide_ng::types::ChatId;
use tracing::warn;
use uuid::Uuid;

pub(super) struct RegistrationEndInput {
    pub(super) bot: Bot,
    pub(super) chat_id: ChatId,
    pub(super) lang: LanguageCode,
    pub(super) username: Username,
    pub(super) password: Password,
    pub(super) nickname: Nickname,
    pub(super) account_type: TTAccountType,
    pub(super) tx_tt: Sender<TTWorkerCommand>,
    pub(super) db: Database,
    pub(super) config: Arc<AppConfig>,
}

pub(super) async fn handle_registration_end_with_type(
    input: RegistrationEndInput,
) -> HandlerResult {
    let RegistrationEndInput {
        bot,
        chat_id,
        lang,
        username,
        password,
        nickname,
        account_type,
        tx_tt,
        db,
        config,
    } = input;
    let is_admin = config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(chat_id.0));

    if config.telegram.verify_registration && !is_admin {
        handle_admin_verification(AdminVerificationInput {
            bot: &bot,
            chat_id,
            lang,
            username: &username,
            password: &password,
            nickname: &nickname,
            db: &db,
            config: &config,
        })
        .await?;
        return Ok(());
    }

    handle_direct_registration(DirectRegistrationInput {
        bot: &bot,
        chat_id,
        lang,
        username: &username,
        password: &password,
        nickname: &nickname,
        account_type,
        tx_tt,
        db: &db,
        config: &config,
    })
    .await?;
    Ok(())
}

struct AdminVerificationInput<'a> {
    bot: &'a Bot,
    chat_id: ChatId,
    lang: LanguageCode,
    username: &'a Username,
    password: &'a Password,
    nickname: &'a Nickname,
    db: &'a Database,
    config: &'a AppConfig,
}

struct DirectRegistrationInput<'a> {
    bot: &'a Bot,
    chat_id: ChatId,
    lang: LanguageCode,
    username: &'a Username,
    password: &'a Password,
    nickname: &'a Nickname,
    account_type: TTAccountType,
    tx_tt: Sender<TTWorkerCommand>,
    db: &'a Database,
    config: &'a AppConfig,
}

async fn handle_admin_verification(input: AdminVerificationInput<'_>) -> HandlerResult {
    let AdminVerificationInput {
        bot,
        chat_id,
        lang,
        username,
        password,
        nickname,
        db,
        config,
    } = input;
    let request_id = Uuid::new_v4().to_string();
    let (fullname, tg_username) = fetch_user_info(bot, chat_id).await;
    let source_info = format!(
        "lang={};tg_username={};fullname={}",
        lang.as_str(),
        tg_username,
        fullname
    );
    if db
        .add_pending_registration(
            &request_id,
            TelegramId::new(chat_id.0),
            username.as_str(),
            password.as_str(),
            nickname.as_str(),
            &source_info,
        )
        .await
        .is_err()
    {
        bot.send_message(chat_id, t(lang.as_str(), "admin-submit-error"))
            .await?;
        return Ok(());
    }

    bot.send_message(chat_id, t(lang.as_str(), "admin-approval-sent"))
        .await?;

    let admin_lang = config.telegram.bot_admin_lang.clone();
    let text = build_admin_request_text(
        admin_lang.as_str(),
        chat_id,
        username,
        nickname,
        &fullname,
        &tg_username,
    );

    let keyboard = crate::tg_bot::keyboards::admin_approval_keyboard(
        &t(admin_lang.as_str(), "btn-admin-verify"),
        &t(admin_lang.as_str(), "btn-admin-reject"),
        &request_id,
    );

    for &admin_id in &config.telegram.admin_ids {
        if let Err(e) = bot
            .send_message(ChatId(admin_id.as_i64()), &text)
            .reply_markup(keyboard.clone())
            .await
        {
            warn!(error = %e, admin_id = %admin_id, "Failed to send admin approval message");
        }
    }

    Ok(())
}

async fn handle_direct_registration(input: DirectRegistrationInput<'_>) -> HandlerResult {
    let DirectRegistrationInput {
        bot,
        chat_id,
        lang,
        username,
        password,
        nickname,
        account_type,
        tx_tt,
        db,
        config,
    } = input;
    let (tg_fullname, tg_username) = fetch_user_info(bot, chat_id).await;
    let mut source_info = format!("Telegram ID: {}", chat_id.0);
    if !tg_username.is_empty() {
        let _ = write!(&mut source_info, ", username: @{tg_username}");
    }
    if !tg_fullname.is_empty() {
        let _ = write!(&mut source_info, ", name: {tg_fullname}");
    }

    let result = registration::create_teamtalk_account(registration::CreateAccountParams {
        username,
        password,
        nickname,
        account_type,
        source: RegistrationSource::Telegram(TelegramId::new(chat_id.0)),
        source_info: Some(source_info),
        telegram_id: Some(TelegramId::new(chat_id.0)),
        tx_tt,
        db,
        config,
    })
    .await?;

    if !result.created {
        bot.send_message(chat_id, t(lang.as_str(), "register-error"))
            .await?;
        return Ok(());
    }

    if let Some(err) = result.db_sync_error {
        notify_db_sync_error(bot, config, chat_id, username.as_str(), &err).await;
        if let Err(e) = bot
            .send_message(chat_id, t(lang.as_str(), "register-success-db-sync-issue"))
            .await
        {
            warn!(error = %e, "Failed to notify user about db sync issue");
        }
    }

    let args = HashMap::from([("username".to_string(), username.as_str().to_string())]);
    bot.send_message(chat_id, t_args(lang.as_str(), "register-success", &args))
        .await?;

    if let Some(assets) = result.assets {
        send_registration_assets(
            bot,
            chat_id,
            lang.as_str(),
            config,
            username.as_str(),
            password.as_str(),
            &assets,
        )
        .await?;
    }

    Ok(())
}

async fn fetch_user_info(bot: &Bot, chat_id: ChatId) -> (String, String) {
    match bot.get_chat(chat_id).await {
        Ok(u) => {
            let first = u.first_name().unwrap_or("Unknown");
            let last = u.last_name().unwrap_or("");
            let fullname = if last.is_empty() {
                first.to_string()
            } else {
                format!("{first} {last}")
            };
            let username = u.username().map(ToString::to_string).unwrap_or_default();
            (fullname, username)
        }
        Err(e) => {
            warn!(error = %e, "Failed to fetch Telegram user info");
            ("Unknown".to_string(), String::new())
        }
    }
}

fn build_admin_request_text(
    lang: &str,
    chat_id: ChatId,
    username: &Username,
    nickname: &Nickname,
    fullname: &str,
    tg_username: &str,
) -> String {
    let mut text = String::new();
    text.push_str(&t(lang, "admin-request-title"));
    text.push('\n');
    text.push_str(&t(lang, "admin-request-username"));
    text.push(' ');
    text.push_str(username.as_str());
    text.push('\n');
    if nickname.as_str() != username.as_str() {
        text.push_str(&t(lang, "admin-request-nickname"));
        text.push(' ');
        text.push_str(nickname.as_str());
        text.push('\n');
    }

    let mut tg_line = String::new();
    tg_line.push_str(fullname);
    if !tg_username.is_empty() {
        tg_line.push_str(" (@");
        tg_line.push_str(tg_username);
        tg_line.push(')');
    }
    tg_line.push_str(" (ID: ");
    tg_line.push_str(&chat_id.0.to_string());
    tg_line.push(')');
    text.push_str(&t(lang, "admin-request-telegram-user"));
    text.push(' ');
    text.push_str(&tg_line);
    text.push('\n');
    text.push_str(&t(lang, "admin-request-approve"));
    text
}
