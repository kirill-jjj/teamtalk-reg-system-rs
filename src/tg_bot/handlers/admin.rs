use super::registration::{notify_db_sync_error, send_registration_assets};
use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::admin::parse_source_info;
use crate::services::registration;
use crate::types::{RegistrationSource, TTAccountType, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tracing::warn;
use uuid::Uuid;

pub async fn admin_panel(
    bot: Bot,
    msg: Message,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
) -> HandlerResult {
    if !config.admin_ids.contains(&TelegramId::new(msg.chat.id.0)) {
        return Ok(());
    }
    let lang = config.bot_admin_lang.clone();
    bot.send_message(msg.chat.id, t(lang.as_str(), "admin-panel-title"))
        .reply_markup(crate::tg_bot::keyboards::admin_panel_keyboard(
            &t(lang.as_str(), "btn-delete-user"),
            &t(lang.as_str(), "btn-manage-banlist"),
            &t(lang.as_str(), "btn-list-tt-accounts"),
        ))
        .await?;
    dialogue.update(State::AdminPanel).await?;
    Ok(())
}

pub async fn admin_callback(
    bot: Bot,
    q: CallbackQuery,
    db: Database,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
) -> HandlerResult {
    let data = q.data.clone().unwrap_or_default();
    let chat_id = q.from.id.0 as i64;
    let lang = config.bot_admin_lang.clone();

    if config.admin_ids.contains(&TelegramId::new(chat_id)) {
        if data.starts_with("approve_") {
            let req_id = data.replace("approve_", "");
            if let Ok(Some(req)) = db.get_pending_registration(&req_id).await {
                let Some(username) = Username::parse(&req.username) else {
                    bot.answer_callback_query(q.id.clone())
                        .text(t(lang.as_str(), "admin-req-not-found"))
                        .await?;
                    return Ok(());
                };
                let Some(password) = Password::parse(&req.password_cleartext) else {
                    bot.answer_callback_query(q.id.clone())
                        .text(t(lang.as_str(), "admin-req-not-found"))
                        .await?;
                    return Ok(());
                };
                let Some(nickname) = Nickname::parse(&req.nickname) else {
                    bot.answer_callback_query(q.id.clone())
                        .text(t(lang.as_str(), "admin-req-not-found"))
                        .await?;
                    return Ok(());
                };
                let req_lang = parse_source_info(&req.source_info).lang;
                let result =
                    registration::create_teamtalk_account(registration::CreateAccountParams {
                        username: &username,
                        password: &password,
                        nickname: &nickname,
                        account_type: TTAccountType::Default,
                        source: RegistrationSource::Telegram(req.registrant_telegram_id),
                        source_info: Some(req.source_info.clone()),
                        telegram_id: Some(req.registrant_telegram_id),
                        tx_tt: tx_tt.clone(),
                        db: &db,
                        config: &config,
                    })
                    .await?;

                if let Err(e) = bot
                    .send_message(
                        ChatId(req.registrant_telegram_id.as_i64()),
                        t(req_lang.as_str(), "admin-approved"),
                    )
                    .await
                {
                    warn!(error = %e, "Failed to notify user about approval");
                }

                let alert_args =
                    HashMap::from([("username".to_string(), username.as_str().to_string())]);
                bot.answer_callback_query(q.id.clone())
                    .text(t_args(
                        lang.as_str(),
                        "admin-req-approved-alert",
                        &alert_args,
                    ))
                    .await?;
                if let Some(m) = &q.message
                    && let Err(e) = bot.delete_message(m.chat().id, m.id()).await
                {
                    warn!(error = %e, "Failed to delete admin request message");
                }
                if !result.created {
                    if let Err(e) = bot
                        .send_message(
                            ChatId(q.from.id.0 as i64),
                            t_args(
                                lang.as_str(),
                                "admin-approve-failed-critical",
                                &HashMap::from([(
                                    "username".to_string(),
                                    username.as_str().to_string(),
                                )]),
                            ),
                        )
                        .await
                    {
                        warn!(error = %e, "Failed to notify admin about approval failure");
                    }
                    notify_admin_decision(
                        &bot,
                        &config,
                        &q,
                        "approved",
                        username.as_str(),
                        req.registrant_telegram_id,
                        &req.source_info,
                    )
                    .await;
                    db.delete_pending_registration(&req_id).await?;
                    return Ok(());
                }

                if let Some(err) = result.db_sync_error {
                    notify_db_sync_error(
                        &bot,
                        &config,
                        ChatId(req.registrant_telegram_id.as_i64()),
                        username.as_str(),
                        &err,
                    )
                    .await;
                    if let Err(e) = bot
                        .send_message(
                            ChatId(req.registrant_telegram_id.as_i64()),
                            t(req_lang.as_str(), "register-success-db-sync-issue"),
                        )
                        .await
                    {
                        warn!(error = %e, "Failed to notify user about db sync issue");
                    }
                }
                if let Some(assets) = result.assets
                    && let Err(e) = send_registration_assets(
                        &bot,
                        ChatId(req.registrant_telegram_id.as_i64()),
                        req_lang.as_str(),
                        &config,
                        username.as_str(),
                        password.as_str(),
                        &assets,
                    )
                    .await
                {
                    warn!(error = %e, "Failed to send registration assets to user");
                }
                notify_admin_decision(
                    &bot,
                    &config,
                    &q,
                    "approved",
                    username.as_str(),
                    req.registrant_telegram_id,
                    &req.source_info,
                )
                .await;
                db.delete_pending_registration(&req_id).await?;
            } else {
                bot.answer_callback_query(q.id.clone())
                    .text(t(lang.as_str(), "admin-req-not-found"))
                    .await?;
                if let Some(m) = &q.message {
                    bot.edit_message_text(
                        m.chat().id,
                        m.id(),
                        t(lang.as_str(), "admin-req-handled"),
                    )
                    .await?;
                }
            }
            return Ok(());
        } else if data.starts_with("reject_") {
            let req_id = data.replace("reject_", "");
            if let Ok(Some(req)) = db.get_pending_registration(&req_id).await {
                let username = req.username.clone();
                let req_lang = parse_source_info(&req.source_info).lang;
                bot.send_message(
                    ChatId(req.registrant_telegram_id.as_i64()),
                    t(req_lang.as_str(), "admin-rejected"),
                )
                .await?;
                let alert_args = HashMap::from([("username".to_string(), username.clone())]);
                bot.answer_callback_query(q.id.clone())
                    .text(t_args(
                        lang.as_str(),
                        "admin-req-rejected-alert",
                        &alert_args,
                    ))
                    .await?;
                if let Some(m) = &q.message
                    && let Err(e) = bot.delete_message(m.chat().id, m.id()).await
                {
                    warn!(error = %e, "Failed to delete admin request message");
                }
                notify_admin_decision(
                    &bot,
                    &config,
                    &q,
                    "rejected",
                    &username,
                    req.registrant_telegram_id,
                    &req.source_info,
                )
                .await;
                db.delete_pending_registration(&req_id).await?;
            } else {
                bot.answer_callback_query(q.id.clone())
                    .text(t(lang.as_str(), "admin-req-not-found"))
                    .await?;
                if let Some(m) = &q.message {
                    bot.edit_message_text(
                        m.chat().id,
                        m.id(),
                        t(lang.as_str(), "admin-req-handled"),
                    )
                    .await?;
                }
            }
            return Ok(());
        }
    }

    if !config.admin_ids.contains(&TelegramId::new(chat_id)) {
        return Ok(());
    }
    bot.answer_callback_query(q.id).await?;

    let msg = match q.message {
        Some(m) => m,
        None => return Ok(()),
    };

    if data == "admin_del" {
        let users = db.get_all_registrations().await?;
        if users.is_empty() {
            bot.edit_message_text(msg.chat().id, msg.id(), t(lang.as_str(), "admin-no-users"))
                .await?;
        } else {
            let user_list: Vec<(TelegramId, String)> = users
                .into_iter()
                .map(|u| (u.telegram_id, u.teamtalk_username))
                .collect();
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-select-delete"),
            )
            .reply_markup(crate::tg_bot::keyboards::admin_user_list_keyboard(
                user_list,
            ))
            .await?;
        }
    } else if data.starts_with("admin_del_confirm_") {
        let target_id_str = data.replace("admin_del_confirm_", "");
        let Ok(target_id) = target_id_str.parse::<i64>() else {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-action-refresh-fail"),
            )
            .await?;
            return Ok(());
        };
        let tg_id = TelegramId::new(target_id);
        let reg = db.get_registration_by_id(tg_id).await?;
        if db.delete_registration(tg_id).await? {
            let tt_user = reg
                .map(|r| r.teamtalk_username)
                .unwrap_or_else(|| "Unknown".to_string());
            db.ban_user(
                tg_id,
                Some(&tt_user),
                Some(TelegramId::new(chat_id)),
                Some("Deleted via admin panel"),
            )
            .await?;
            let args = HashMap::from([("tg_id".to_string(), target_id.to_string())]);
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t_args(lang.as_str(), "admin-user-deleted", &args),
            )
            .await?;
        }
    } else if data == "admin_banlist_view" {
        let banned = db.get_all_banned_users().await?;
        if banned.is_empty() {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-banlist-empty"),
            )
            .await?;
        } else {
            let mut lines = Vec::new();
            lines.push(t(lang.as_str(), "admin-banlist-title"));
            let list: Vec<(TelegramId, String)> = banned
                .into_iter()
                .map(|b| {
                    let tt_user = b.teamtalk_username.unwrap_or_else(|| "N/A".to_string());
                    let reason = b.reason.unwrap_or_else(|| "N/A".to_string());
                    lines.push(format!(
                        "TG ID: {} - TT User: {} (Reason: {})",
                        b.telegram_id, tt_user, reason
                    ));
                    (b.telegram_id, reason)
                })
                .collect();
            let text = lines.join("\n");
            if bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .reply_markup(crate::tg_bot::keyboards::admin_banlist_keyboard(
                    list,
                    &t(lang.as_str(), "btn-unban"),
                    &t(lang.as_str(), "btn-add-ban-manual"),
                ))
                .await
                .is_err()
            {
                bot.send_message(msg.chat().id, t(lang.as_str(), "admin-action-refresh-fail"))
                    .await?;
            }
        }
    } else if data.starts_with("admin_unban_") {
        let target_id: i64 = data.replace("admin_unban_", "").parse().unwrap_or(0);
        if target_id == 0 {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-unban-no-target"),
            )
            .await?;
            return Ok(());
        }
        let args = HashMap::from([("tg_id".to_string(), target_id.to_string())]);
        let edit_result = if db.unban_user(TelegramId::new(target_id)).await? {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t_args(lang.as_str(), "admin-unbanned", &args),
            )
            .await
        } else {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t_args(lang.as_str(), "admin-unban-fail", &args),
            )
            .await
        };
        if edit_result.is_err() {
            bot.send_message(msg.chat().id, t(lang.as_str(), "admin-action-refresh-fail"))
                .await?;
        }
    } else if data == "admin_ban_manual" {
        bot.send_message(msg.chat().id, t(lang.as_str(), "admin-ban-prompt"))
            .await?;
        dialogue.update(State::AwaitingManualBanInput).await?;
    } else if data == "admin_tt_list" {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = tx_tt.send(TTWorkerCommand::GetAllUsers { resp: tx }) {
            warn!(error = %e, "Failed to enqueue TeamTalk users list request");
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-tt-list-error"),
            )
            .await?;
            return Ok(());
        }
        if let Ok(users) = rx.await {
            if users.is_empty() {
                bot.edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    t(lang.as_str(), "admin-tt-no-accounts"),
                )
                .await?;
            } else {
                let mut lines = Vec::new();
                lines.push(t(lang.as_str(), "admin-tt-list-title"));
                for u in &users {
                    lines.push(format!("- {}", u));
                }
                let text = lines.join("\n");
                bot.edit_message_text(msg.chat().id, msg.id(), text)
                    .reply_markup(crate::tg_bot::keyboards::admin_tt_accounts_keyboard(
                        users,
                        &t(lang.as_str(), "btn-delete-from-tt"),
                    ))
                    .await?;
            }
        } else {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-tt-list-error"),
            )
            .await?;
        }
    } else if data.starts_with("admin_tt_del_prompt_") {
        let username = data.replace("admin_tt_del_prompt_", "");
        let args = HashMap::from([("tt_username".to_string(), username.clone())]);
        bot.edit_message_text(
            msg.chat().id,
            msg.id(),
            t_args(lang.as_str(), "admin-tt-delete-prompt", &args),
        )
        .reply_markup(crate::tg_bot::keyboards::confirm_keyboard(
            &t(lang.as_str(), "btn-confirm-delete"),
            &t(lang.as_str(), "btn-cancel"),
            &format!("tt_del_{}", username),
        ))
        .await?;
    } else if data.starts_with("confirm_tt_del_") {
        let username = data.replace("confirm_tt_del_", "");
        let Some(tt_username) = Username::parse(&username) else {
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t(lang.as_str(), "admin-tt-list-error"),
            )
            .await?;
            return Ok(());
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = tx_tt.send(TTWorkerCommand::DeleteUser {
            username: tt_username,
            resp: tx,
        }) {
            warn!(error = %e, "Failed to enqueue TeamTalk delete user command");
            let mut args = HashMap::from([("tt_username".to_string(), username.clone())]);
            args.insert("error".to_string(), "Dispatcher error".to_string());
            bot.edit_message_text(
                msg.chat().id,
                msg.id(),
                t_args(lang.as_str(), "admin-tt-delete-fail", &args),
            )
            .await?;
            return Ok(());
        }
        let args = HashMap::from([("tt_username".to_string(), username.clone())]);
        match rx.await {
            Ok(Ok(true)) => {
                bot.edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    t_args(lang.as_str(), "admin-tt-deleted", &args),
                )
                .await?;
            }
            Ok(Ok(false)) => {
                let mut args = args.clone();
                args.insert(
                    "error".to_string(),
                    "Command indicated failure without a specific error.".to_string(),
                );
                bot.edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    t_args(lang.as_str(), "admin-tt-delete-fail", &args),
                )
                .await?;
            }
            Ok(Err(err)) => {
                let mut args = args.clone();
                args.insert("error".to_string(), err);
                bot.edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    t_args(lang.as_str(), "admin-tt-delete-fail", &args),
                )
                .await?;
            }
            Err(e) => {
                warn!(error = %e, "Failed to receive TT delete response");
                let mut args = args.clone();
                args.insert("error".to_string(), "Unknown error".to_string());
                bot.edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    t_args(lang.as_str(), "admin-tt-delete-fail", &args),
                )
                .await?;
            }
        }
    } else if data == "cancel_action" {
        bot.edit_message_text(
            msg.chat().id,
            msg.id(),
            t(lang.as_str(), "admin-panel-title"),
        )
        .reply_markup(crate::tg_bot::keyboards::admin_panel_keyboard(
            &t(lang.as_str(), "btn-delete-user"),
            &t(lang.as_str(), "btn-manage-banlist"),
            &t(lang.as_str(), "btn-list-tt-accounts"),
        ))
        .await?;
    }

    Ok(())
}

pub async fn admin_manual_ban_input(
    bot: Bot,
    msg: Message,
    db: Database,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
) -> HandlerResult {
    let text = msg.text().unwrap_or("");
    let parts: Vec<&str> = text.lines().collect();
    let lang = config.bot_admin_lang.clone();

    if parts.is_empty() {
        return Ok(());
    }

    if let Ok(tg_id) = parts[0].trim().parse::<i64>() {
        let tg_id_typed = TelegramId::new(tg_id);
        let reason = if parts.len() > 1 {
            Some(parts[1])
        } else {
            None
        };
        let args = HashMap::from([("tg_id".to_string(), tg_id.to_string())]);
        if db
            .ban_user(
                tg_id_typed,
                None,
                Some(TelegramId::new(msg.chat.id.0)),
                reason,
            )
            .await
            .is_err()
        {
            bot.send_message(msg.chat.id, t_args(lang.as_str(), "admin-ban-fail", &args))
                .await?;
        } else {
            bot.send_message(
                msg.chat.id,
                t_args(lang.as_str(), "admin-ban-success", &args),
            )
            .await?;
        }
    } else {
        bot.send_message(msg.chat.id, t(lang.as_str(), "admin-ban-invalid"))
            .await?;
    }

    dialogue.update(State::AdminPanel).await?;
    Ok(())
}

pub async fn generate_invite(
    bot: Bot,
    msg: Message,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    if !config.admin_ids.contains(&TelegramId::new(msg.chat.id.0)) {
        return Ok(());
    }
    if !config.telegram_deeplink_registration_enabled {
        bot.send_message(
            msg.chat.id,
            t(config.bot_admin_lang.as_str(), "deeplink-disabled"),
        )
        .await?;
        return Ok(());
    }

    let token = Uuid::new_v4().to_string().replace('-', "");
    let expires = chrono::Utc::now().naive_utc() + chrono::Duration::minutes(5);
    if db
        .create_deeplink(&token, expires, TelegramId::new(msg.chat.id.0))
        .await
        .is_err()
    {
        bot.send_message(
            msg.chat.id,
            t(config.bot_admin_lang.as_str(), "deeplink-generate-error"),
        )
        .await?;
        return Ok(());
    }

    let bot_info = match bot.get_me().await {
        Ok(info) => info,
        Err(e) => {
            warn!(error = %e, "Failed to fetch bot info");
            bot.send_message(
                msg.chat.id,
                t(config.bot_admin_lang.as_str(), "deeplink-generate-error"),
            )
            .await?;
            return Ok(());
        }
    };
    let bot_username = match bot_info.username.clone() {
        Some(v) => v,
        None => {
            bot.send_message(
                msg.chat.id,
                t(
                    config.bot_admin_lang.as_str(),
                    "deeplink-bot-username-missing",
                ),
            )
            .await?;
            return Ok(());
        }
    };
    let link = format!("https://t.me/{}?start={}", bot_username, token);
    bot.send_message(msg.chat.id, link).await?;
    Ok(())
}

async fn notify_admin_decision(
    bot: &Bot,
    config: &AppConfig,
    q: &CallbackQuery,
    decision: &str,
    username: &str,
    registrant_telegram_id: TelegramId,
    source_info: &str,
) {
    let admin_lang = config.bot_admin_lang.clone();
    let source = parse_source_info(source_info);
    let user_lang = source.lang;
    let tg_username = source.tg_username;
    let fullname = source.fullname;
    let admin_name = q.from.full_name();

    let decision_text = if decision == "approved" {
        t(admin_lang.as_str(), "admin-decision-approved")
    } else {
        t(admin_lang.as_str(), "admin-decision-rejected")
    };

    let mut args = HashMap::new();
    args.insert("admin_name".to_string(), admin_name);
    args.insert("admin_id".to_string(), q.from.id.0.to_string());
    args.insert("decision".to_string(), decision_text);
    args.insert("teamtalk_username".to_string(), username.to_string());
    args.insert(
        "registrant_telegram_id".to_string(),
        registrant_telegram_id.to_string(),
    );
    args.insert("registrant_fullname".to_string(), fullname);
    args.insert("registrant_tg_username".to_string(), tg_username);
    args.insert(
        "registrant_lang".to_string(),
        user_lang.as_str().to_string(),
    );

    let mut text = t_args(admin_lang.as_str(), "admin-decision-notify", &args);
    if !args
        .get("registrant_tg_username")
        .unwrap_or(&String::new())
        .is_empty()
    {
        let suffix = t_args(
            admin_lang.as_str(),
            "admin-decision-telegram-username",
            &args,
        );
        text.push_str(&suffix);
    }
    for &admin_id in &config.admin_ids {
        if admin_id.as_i64() != q.from.id.0 as i64 {
            let _ = bot.send_message(ChatId(admin_id.as_i64()), &text).await;
        }
    }
}

pub async fn exit_bot(bot: Bot, msg: Message, config: Arc<AppConfig>) -> HandlerResult {
    if !config.admin_ids.contains(&TelegramId::new(msg.chat.id.0)) {
        return Ok(());
    }
    bot.send_message(
        msg.chat.id,
        t(config.bot_admin_lang.as_str(), "bot-shutdown"),
    )
    .await?;
    std::process::exit(0);
}
