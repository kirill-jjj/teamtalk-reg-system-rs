use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::registration;
use crate::types::{LanguageCode, RegistrationSource, TTAccountType, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide::prelude::*;
use teloxide::types::{ChatId, InputFile};
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

async fn is_banned(db: &Database, chat_id: TelegramId) -> bool {
    db.get_banned_user(chat_id).await.unwrap_or(None).is_some()
}

pub async fn start(
    bot: Bot,
    msg: Message,
    dialogue: MyDialogue,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    let chat_id = TelegramId::new(msg.chat.id.0);

    if is_banned(&db, chat_id).await {
        return Ok(());
    }

    let is_admin = config.admin_ids.contains(&chat_id);
    let initial_lang = msg
        .from
        .as_ref()
        .and_then(|u| u.language_code.as_deref())
        .map(LanguageCode::parse_or_default)
        .unwrap_or_else(|| config.bot_admin_lang.clone());
    let text = msg.text().unwrap_or("");
    let args: Vec<&str> = text.split_whitespace().collect();

    let mut is_deeplink = false;
    if args.len() > 1 {
        let token = args[1];
        if !config.telegram_deeplink_registration_enabled {
            bot.send_message(msg.chat.id, t(initial_lang.as_str(), "deeplink-disabled"))
                .await?;
            return Ok(());
        }

        if let Ok(Some(_token_obj)) = db.get_valid_deeplink(token).await {
            if db.is_telegram_registered(chat_id).await.unwrap_or(false) && !is_admin {
                bot.send_message(
                    msg.chat.id,
                    t(initial_lang.as_str(), "deeplink-used-already"),
                )
                .await?;
                return Ok(());
            }
            db.mark_deeplink_used(token).await?;
            debug!(chat_id = %chat_id, "Deeplink used by user");
            is_deeplink = true;
        } else {
            bot.send_message(msg.chat.id, t(initial_lang.as_str(), "deeplink-invalid"))
                .await?;
            return Ok(());
        }
    } else if !config.telegram_public_registration_enabled && !is_admin {
        return Ok(());
    }

    if !is_admin && db.is_telegram_registered(chat_id).await.unwrap_or(false) {
        bot.send_message(msg.chat.id, t(initial_lang.as_str(), "already-registered"))
            .await?;
        return Ok(());
    }

    if let Some(lang) = &config.force_user_lang {
        bot.send_message(msg.chat.id, t(lang.as_str(), "username-prompt"))
            .await?;
        dialogue
            .update(State::AwaitingUsername { lang: lang.clone() })
            .await?;
        return Ok(());
    }

    let start_key = if is_deeplink {
        "deeplink-welcome"
    } else {
        "start-message"
    };
    bot.send_message(msg.chat.id, t(initial_lang.as_str(), start_key))
        .reply_markup(crate::tg_bot::keyboards::language_keyboard())
        .await?;

    dialogue.update(State::ChoosingLanguage).await?;
    Ok(())
}

pub async fn receive_language(bot: Bot, q: CallbackQuery, dialogue: MyDialogue) -> HandlerResult {
    if let Some(data) = q.data {
        let lang = LanguageCode::parse_or_default(&data.replace("lang_", ""));
        bot.answer_callback_query(q.id)
            .text(t(lang.as_str(), "language-set"))
            .await?;

        if let Some(msg) = q.message {
            bot.send_message(msg.chat().id, t(lang.as_str(), "username-prompt"))
                .await?;
        }

        dialogue.update(State::AwaitingUsername { lang }).await?;
    }
    Ok(())
}

pub async fn receive_username(
    bot: Bot,
    msg: Message,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
) -> HandlerResult {
    let lang = match dialogue.get().await {
        Ok(Some(State::AwaitingUsername { lang })) => lang,
        Ok(_) => LanguageCode::default(),
        Err(e) => {
            warn!(error = %e, "Failed to read dialogue state (AwaitingUsername)");
            LanguageCode::default()
        }
    };

    let Some(username) = Username::parse(msg.text().unwrap_or("")) else {
        bot.send_message(msg.chat.id, t(lang.as_str(), "username-empty-error"))
            .await?;
        return Ok(());
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Err(e) = tx_tt.send(TTWorkerCommand::CheckUserExists {
        username: username.clone(),
        resp: tx,
    }) {
        error!(error = %e, "Failed to enqueue username check");
        bot.send_message(msg.chat.id, t(lang.as_str(), "username-check-error"))
            .await?;
        return Ok(());
    }

    match rx.await {
        Ok(true) => {
            bot.send_message(msg.chat.id, t(lang.as_str(), "username-taken"))
                .await?;
            return Ok(());
        }
        Ok(false) => {}
        Err(e) => {
            warn!(error = %e, "Failed to receive username check response");
            bot.send_message(msg.chat.id, t(lang.as_str(), "username-check-error"))
                .await?;
            return Ok(());
        }
    }

    bot.send_message(msg.chat.id, t(lang.as_str(), "password-prompt"))
        .await?;
    dialogue
        .update(State::AwaitingPassword { lang, username })
        .await?;
    Ok(())
}

pub async fn receive_password(bot: Bot, msg: Message, dialogue: MyDialogue) -> HandlerResult {
    let Some(State::AwaitingPassword { lang, username }) = (match dialogue.get().await {
        Ok(state) => state,
        Err(e) => {
            warn!(error = %e, "Failed to read dialogue state (AwaitingPassword)");
            return Ok(());
        }
    }) else {
        return Ok(());
    };

    let Some(password) = Password::parse(msg.text().unwrap_or("")) else {
        bot.send_message(msg.chat.id, t(lang.as_str(), "password-empty-error"))
            .await?;
        return Ok(());
    };
    let args = HashMap::from([("username".to_string(), username.as_str().to_string())]);

    bot.send_message(
        msg.chat.id,
        t_args(lang.as_str(), "nickname-prompt-choice", &args),
    )
    .reply_markup(crate::tg_bot::keyboards::nickname_choice_keyboard(
        &t(lang.as_str(), "btn-yes"),
        &t(lang.as_str(), "btn-no"),
    ))
    .await?;

    dialogue
        .update(State::AwaitingNicknameChoice {
            lang,
            username,
            password,
        })
        .await?;
    Ok(())
}

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
            if config.admin_ids.contains(&TelegramId::new(msg.chat().id.0)) {
                ask_account_type(bot, msg.chat().id, lang, username, password, nick, dialogue)
                    .await?;
                return Ok(());
            }
            handle_registration_end(
                bot,
                msg.chat().id,
                lang,
                username,
                password,
                nick,
                tx_tt,
                db,
                config,
            )
            .await?;
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
    if config.admin_ids.contains(&TelegramId::new(msg.chat.id.0)) {
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
    handle_registration_end(
        bot,
        msg.chat.id,
        lang,
        username,
        password,
        nickname,
        tx_tt,
        db,
        config,
    )
    .await?;
    dialogue.exit().await?;
    Ok(())
}

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
    bot.answer_callback_query(q.id).await?;

    let account_type = if data == "acct_admin" {
        TTAccountType::Admin
    } else {
        TTAccountType::Default
    };

    if let Some(msg) = q.message {
        handle_registration_end_with_type(
            bot,
            msg.chat().id,
            lang,
            username,
            password,
            nickname,
            account_type,
            tx_tt,
            db,
            config,
        )
        .await?;
    }
    dialogue.exit().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_registration_end(
    bot: Bot,
    chat_id: ChatId,
    lang: LanguageCode,
    username: Username,
    password: Password,
    nickname: Nickname,
    tx_tt: Sender<TTWorkerCommand>,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    handle_registration_end_with_type(
        bot,
        chat_id,
        lang,
        username,
        password,
        nickname,
        TTAccountType::Default,
        tx_tt,
        db,
        config,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_registration_end_with_type(
    bot: Bot,
    chat_id: ChatId,
    lang: LanguageCode,
    username: Username,
    password: Password,
    nickname: Nickname,
    account_type: TTAccountType,
    tx_tt: Sender<TTWorkerCommand>,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    let is_admin = config.admin_ids.contains(&TelegramId::new(chat_id.0));

    if config.verify_registration && !is_admin {
        let request_id = Uuid::new_v4().to_string();
        let user_info = bot.get_chat(chat_id).await;
        let (fullname, tg_username) = match user_info {
            Ok(u) => (
                u.first_name().unwrap_or("Unknown").to_string(),
                u.username().unwrap_or("").to_string(),
            ),
            Err(e) => {
                warn!(error = %e, "Failed to fetch Telegram user info");
                ("Unknown".to_string(), "".to_string())
            }
        };
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

        let admin_lang = config.bot_admin_lang.clone();

        let mut text = String::new();
        text.push_str(&t(admin_lang.as_str(), "admin-request-title"));
        text.push('\n');
        text.push_str(&t(admin_lang.as_str(), "admin-request-username"));
        text.push(' ');
        text.push_str(username.as_str());
        text.push('\n');
        if nickname.as_str() != username.as_str() {
            text.push_str(&t(admin_lang.as_str(), "admin-request-nickname"));
            text.push(' ');
            text.push_str(nickname.as_str());
            text.push('\n');
        }
        let mut tg_line = String::new();
        tg_line.push_str(&fullname);
        if !tg_username.is_empty() {
            tg_line.push_str(" (@");
            tg_line.push_str(&tg_username);
            tg_line.push(')');
        }
        tg_line.push_str(" (ID: ");
        tg_line.push_str(&chat_id.0.to_string());
        tg_line.push(')');
        text.push_str(&t(admin_lang.as_str(), "admin-request-telegram-user"));
        text.push(' ');
        text.push_str(&tg_line);
        text.push('\n');
        text.push_str(&t(admin_lang.as_str(), "admin-request-approve"));

        let keyboard = crate::tg_bot::keyboards::admin_approval_keyboard(
            &t(admin_lang.as_str(), "btn-admin-verify"),
            &t(admin_lang.as_str(), "btn-admin-reject"),
            &request_id,
        );

        for &admin_id in &config.admin_ids {
            if let Err(e) = bot
                .send_message(ChatId(admin_id.as_i64()), &text)
                .reply_markup(keyboard.clone())
                .await
            {
                warn!(error = %e, admin_id = %admin_id, "Failed to send admin approval message");
            }
        }
    } else {
        let (tg_fullname, tg_username) = match bot.get_chat(chat_id).await {
            Ok(u) => {
                let first = u.first_name().unwrap_or("Unknown");
                let last = u.last_name().unwrap_or("");
                let fullname = if last.is_empty() {
                    first.to_string()
                } else {
                    format!("{} {}", first, last)
                };
                let username = u.username().map(|u| u.to_string()).unwrap_or_default();
                (fullname, username)
            }
            Err(e) => {
                warn!(error = %e, "Failed to fetch Telegram user info");
                ("Unknown".to_string(), String::new())
            }
        };
        let mut source_info = format!("Telegram ID: {}", chat_id.0);
        if !tg_username.is_empty() {
            source_info.push_str(&format!(", username: @{}", tg_username));
        }
        if !tg_fullname.is_empty() {
            source_info.push_str(&format!(", name: {}", tg_fullname));
        }

        let result = registration::create_teamtalk_account(registration::CreateAccountParams {
            username: &username,
            password: &password,
            nickname: &nickname,
            account_type,
            source: RegistrationSource::Telegram(TelegramId::new(chat_id.0)),
            source_info: Some(source_info),
            telegram_id: Some(TelegramId::new(chat_id.0)),
            tx_tt,
            db: &db,
            config: &config,
        })
        .await?;

        if !result.created {
            bot.send_message(chat_id, t(lang.as_str(), "register-error"))
                .await?;
            return Ok(());
        }

        if let Some(err) = result.db_sync_error {
            notify_db_sync_error(&bot, &config, chat_id, username.as_str(), &err).await;
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
                &bot,
                chat_id,
                lang.as_str(),
                &config,
                username.as_str(),
                password.as_str(),
                &assets,
            )
            .await?;
        }
    }
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
    let file_tt = InputFile::memory(assets.tt_content.clone().into_bytes())
        .file_name(assets.tt_filename.clone());
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
        .send_message(chat_id, format!("{}\n{}", link_text, assets.tt_link))
        .await
    {
        warn!(error = %e, "Failed to send TT link");
        bot.send_message(chat_id, t(lang, "file-send-error"))
            .await?;
    }

    let public_host = config
        .tt_public_hostname
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&config.host_name);
    let host_msg = t_args(
        lang,
        "msg-host",
        &HashMap::from([("host".to_string(), public_host.to_string())]),
    );
    let port_msg = t_args(
        lang,
        "msg-port",
        &HashMap::from([("port".to_string(), config.tcp_port.to_string())]),
    );

    bot.send_message(chat_id, host_msg).await?;
    bot.send_message(chat_id, port_msg).await?;

    let zip_filename = format!("{}_TeamTalk.zip", username);
    let zip_path = registration::temp_dir().join(&zip_filename);
    if registration::try_create_client_zip_async(config, &zip_path, assets).await
        && let Ok(metadata) = tokio::fs::metadata(&zip_path).await
    {
        let size_mb = metadata.len() as f64 / 1024.0 / 1024.0;
        if size_mb < 49.0 {
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
    for &admin_id in &config.admin_ids {
        if admin_id.as_i64() != chat_id.0
            && let Err(e) = bot
                .send_message(
                    ChatId(admin_id.as_i64()),
                    format!(
                        "DB SYNC ERROR (Exception): User {} (TG ID: {}) created in TeamTalk but FAILED local DB save. Exception: {}",
                        username, chat_id.0, err
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
