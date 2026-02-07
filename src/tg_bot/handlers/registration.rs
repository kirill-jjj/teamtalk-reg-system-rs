use super::registration_completion::{RegistrationEndInput, handle_registration_end_with_type};
pub use super::registration_flow::{receive_language, receive_password, receive_username, start};
use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::registration;
use crate::types::{LanguageCode, TTAccountType, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide::prelude::*;
use teloxide::types::{ChatId, InputFile};
use tracing::{error, trace, warn};

/// Handle nickname choice callback.
pub async fn receive_nickname_choice(
    bot: Bot,
    q: CallbackQuery,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    let Some(State::AwaitingNicknameChoice {
        lang,
        username,
        password,
    }) = (match dialogue.get().await {
        Ok(state) => state,
        Err(e) => {
            warn!(error = %e, "Failed to read dialogue state (AwaitingNicknameChoice)");
            return Ok(());
        }
    })
    else {
        return Ok(());
    };

    let data = q.data.clone().unwrap_or_default();
    if data.is_empty() {
        warn!("Registration callback query missing data");
        return Ok(());
    }
    bot.answer_callback_query(q.id).await?;

    if data == "nick_custom" {
        if let Some(msg) = q.message {
            bot.send_message(msg.chat().id, t(lang.as_str(), "nickname-prompt-enter"))
                .await?;
        }
        dialogue
            .update(State::AwaitingNickname {
                lang,
                username,
                password,
            })
            .await?;
    } else if data == "nick_default" {
        if let Some(msg) = q.message {
            let Some(nick) = Nickname::parse(username.as_str()) else {
                bot.send_message(msg.chat().id, t(lang.as_str(), "username-not-found"))
                    .await?;
                dialogue.exit().await?;
                return Ok(());
            };
            if config
                .telegram
                .admin_ids
                .contains(&TelegramId::new(msg.chat().id.0))
            {
                ask_account_type(bot, msg.chat().id, lang, username, password, nick, dialogue)
                    .await?;
                return Ok(());
            }
            handle_registration_end_with_type(RegistrationEndInput {
                bot,
                chat_id: msg.chat().id,
                lang,
                username,
                password,
                nickname: nick,
                account_type: TTAccountType::Default,
                tx_tt,
                db,
                config,
            })
            .await?;
        } else {
            warn!("Nickname choice callback missing message");
        }
        dialogue.exit().await?;
    } else {
        if let Some(msg) = q.message {
            bot.send_message(msg.chat().id, t(lang.as_str(), "invalid-choice"))
                .await?;
        }
        dialogue.exit().await?;
    }
    Ok(())
}

/// Handle custom nickname input.
pub async fn receive_nickname(
    bot: Bot,
    msg: Message,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    let Some(State::AwaitingNickname {
        lang,
        username,
        password,
    }) = (match dialogue.get().await {
        Ok(state) => state,
        Err(e) => {
            warn!(error = %e, "Failed to read dialogue state (AwaitingNickname)");
            return Ok(());
        }
    })
    else {
        return Ok(());
    };

    let Some(nickname) = Nickname::parse(msg.text().unwrap_or("")) else {
        bot.send_message(msg.chat.id, t(lang.as_str(), "nickname-empty-error"))
            .await?;
        return Ok(());
    };
    if config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(msg.chat.id.0))
    {
        ask_account_type(
            bot,
            msg.chat.id,
            lang,
            username,
            password,
            nickname,
            dialogue,
        )
        .await?;
        return Ok(());
    }
    handle_registration_end_with_type(RegistrationEndInput {
        bot,
        chat_id: msg.chat.id,
        lang,
        username,
        password,
        nickname,
        account_type: TTAccountType::Default,
        tx_tt,
        db,
        config,
    })
    .await?;
    dialogue.exit().await?;
    Ok(())
}

/// Handle account type selection callback for admin registrations.
pub async fn receive_account_type(
    bot: Bot,
    q: CallbackQuery,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    let Some(State::AwaitingAccountType {
        lang,
        username,
        password,
        nickname,
    }) = (match dialogue.get().await {
        Ok(state) => state,
        Err(e) => {
            warn!(error = %e, "Failed to read dialogue state (AwaitingAccountType)");
            return Ok(());
        }
    })
    else {
        return Ok(());
    };

    let data = q.data.clone().unwrap_or_default();
    if data.is_empty() {
        warn!("Registration callback query missing data");
        return Ok(());
    }
    bot.answer_callback_query(q.id).await?;

    let account_type = if data == "acct_admin" {
        TTAccountType::Admin
    } else {
        TTAccountType::Default
    };

    if let Some(msg) = q.message {
        handle_registration_end_with_type(RegistrationEndInput {
            bot,
            chat_id: msg.chat().id,
            lang,
            username,
            password,
            nickname,
            account_type,
            tx_tt,
            db,
            config,
        })
        .await?;
    } else {
        warn!("Account type callback missing message");
    }
    dialogue.exit().await?;
    Ok(())
}

pub(super) async fn send_registration_assets(
    bot: &Bot,
    chat_id: ChatId,
    lang: &str,
    config: &AppConfig,
    username: &str,
    _password: &str,
    assets: &registration::RegistrationAssets,
) -> HandlerResult {
    trace!(chat_id = chat_id.0, username, "Sending registration assets");
    let file_tt =
        InputFile::memory(assets.content.clone().into_bytes()).file_name(assets.filename.clone());
    if let Err(e) = bot
        .send_document(chat_id, file_tt)
        .caption(t(lang, "file-caption"))
        .await
    {
        warn!(error = %e, "Failed to send TT config file");
        bot.send_message(chat_id, t(lang, "file-send-error"))
            .await?;
        return Ok(());
    }

    let link_text = t(lang, "link-text");
    if let Err(e) = bot
        .send_message(chat_id, format!("{}\n{}", link_text, assets.link))
        .await
    {
        warn!(error = %e, "Failed to send TT link");
        bot.send_message(chat_id, t(lang, "file-send-error"))
            .await?;
    }

    let public_host = config
        .teamtalk
        .tt_public_hostname
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&config.teamtalk.host_name);
    let host_msg = t_args(
        lang,
        "msg-host",
        &HashMap::from([("host".to_string(), public_host.to_string())]),
    );
    let port_msg = t_args(
        lang,
        "msg-port",
        &HashMap::from([("port".to_string(), config.teamtalk.tcp_port.to_string())]),
    );

    bot.send_message(chat_id, host_msg).await?;
    bot.send_message(chat_id, port_msg).await?;

    let zip_filename = format!("{username}_TeamTalk.zip");
    let zip_path = registration::temp_dir().join(&zip_filename);
    if registration::try_create_client_zip_async(config, &zip_path, assets).await
        && let Ok(metadata) = tokio::fs::metadata(&zip_path).await
    {
        let size_mb = metadata.len() / 1_048_576;
        if size_mb < 49 {
            let file_zip = InputFile::file(zip_path.clone()).file_name(zip_filename);
            if let Err(e) = bot.send_document(chat_id, file_zip).await {
                error!(error = %e, "Failed to send ZIP");
            }
        } else {
            warn!(size_mb, "ZIP too big, skipping upload");
        }
    }

    Ok(())
}

pub(super) async fn notify_db_sync_error(
    bot: &Bot,
    config: &AppConfig,
    chat_id: ChatId,
    username: &str,
    err: &str,
) {
    for &admin_id in &config.telegram.admin_ids {
        if admin_id.as_i64() != chat_id.0
            && let Err(e) = bot
                .send_message(
                    ChatId(admin_id.as_i64()),
                    format!(
                        "DB SYNC ERROR (Exception): User {username} (TG ID: {}) created in TeamTalk but FAILED local DB save. Exception: {err}",
                        chat_id.0
                    ),
                )
                .await
        {
            warn!(error = %e, admin_id = %admin_id, "Failed to send DB sync error to admin");
        }
    }
}
async fn ask_account_type(
    bot: Bot,
    chat_id: ChatId,
    lang: LanguageCode,
    username: Username,
    password: Password,
    nickname: Nickname,
    dialogue: MyDialogue,
) -> HandlerResult {
    let args = HashMap::from([("username".to_string(), username.as_str().to_string())]);
    bot.send_message(
        chat_id,
        t_args(lang.as_str(), "tt-account-type-prompt", &args),
    )
    .reply_markup(crate::tg_bot::keyboards::admin_account_type_keyboard(
        &t(lang.as_str(), "tt-account-admin"),
        &t(lang.as_str(), "tt-account-user"),
    ))
    .await?;
    dialogue
        .update(State::AwaitingAccountType {
            lang,
            username,
            password,
            nickname,
        })
        .await?;
    Ok(())
}
