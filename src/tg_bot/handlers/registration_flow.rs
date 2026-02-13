use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Password, Username};
use crate::i18n::{t, t_args};
use crate::types::{LanguageCode, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide_ng::prelude::*;
use tracing::{debug, error, warn};

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

    let is_admin = config.telegram.admin_ids.contains(&chat_id);
    let initial_lang = msg
        .from
        .as_ref()
        .and_then(|u| u.language_code.as_deref())
        .map_or_else(
            || config.telegram.bot_admin_lang.clone(),
            LanguageCode::parse_or_default,
        );
    let text = msg.text().unwrap_or("");
    let args: Vec<&str> = text.split_whitespace().collect();

    let mut is_deeplink = false;
    if args.len() > 1 {
        let token = args[1];
        if !config.telegram.telegram_deeplink_registration_enabled {
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
    } else if !config.telegram.telegram_public_registration_enabled && !is_admin {
        return Ok(());
    }

    if !is_admin && db.is_telegram_registered(chat_id).await.unwrap_or(false) {
        bot.send_message(msg.chat.id, t(initial_lang.as_str(), "already-registered"))
            .await?;
        return Ok(());
    }

    if let Some(lang) = &config.web.force_user_lang {
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
        } else {
            warn!("Language callback missing message");
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
