use super::registration::{notify_db_sync_error, send_registration_assets};
use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::admin::parse_source_info;
use crate::services::registration;
use crate::types::{LanguageCode, RegistrationSource, TTAccountType, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tracing::warn;
use uuid::Uuid;

enum AdminCallback {
    Approve(String),
    Reject(String),
    Panel(AdminPanelAction),
}

enum AdminPanelAction {
    DeleteUsers,
    DeleteConfirm(i64),
    BanlistView,
    Unban(i64),
    BanManual,
    ListTeamTalkUsers,
    TeamTalkDeletePrompt(String),
    TeamTalkDeleteConfirm(String),
    Cancel,
}

struct PendingApproval {
    username: Username,
    password: Password,
    nickname: Nickname,
    req_lang: LanguageCode,
    registrant_id: TelegramId,
    source_info: String,
}

/// Show admin panel entrypoint.
pub async fn admin_panel(
    bot: Bot,
    msg: Message,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
) -> HandlerResult {
    if !config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(msg.chat.id.0))
    {
        return Ok(());
    }
    let lang = config.telegram.bot_admin_lang.clone();
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

/// Handle admin panel callbacks.
pub async fn admin_callback(
    bot: Bot,
    q: CallbackQuery,
    db: Database,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
    tx_tt: Sender<TTWorkerCommand>,
) -> HandlerResult {
    let data = q.data.clone().unwrap_or_default();
    if data.is_empty() {
        warn!("Admin callback query missing data");
        return Ok(());
    }
    let Some(chat_id) = i64::try_from(q.from.id.0).ok() else {
        warn!(user_id = q.from.id.0, "Admin callback user id out of range");
        return Ok(());
    };
    let lang = config.telegram.bot_admin_lang.clone();
    if !config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(chat_id))
    {
        return Ok(());
    }
    match parse_admin_callback(&data) {
        Some(AdminCallback::Approve(req_id)) => {
            handle_admin_approve(AdminApproveInput {
                bot: &bot,
                q: &q,
                db: &db,
                config: &config,
                lang: &lang,
                req_id: &req_id,
                tx_tt,
                chat_id,
            })
            .await?;
        }
        Some(AdminCallback::Reject(req_id)) => {
            handle_admin_reject(&bot, &q, &db, &config, &lang, &req_id).await?;
        }
        Some(AdminCallback::Panel(action)) => {
            bot.answer_callback_query(q.id).await?;
            let Some(msg) = q.message.as_ref().and_then(|m| m.regular_message()) else {
                warn!("Admin callback query missing or inaccessible message");
                return Ok(());
            };
            handle_admin_panel_action(
                AdminPanelContext {
                    bot: &bot,
                    msg,
                    db: &db,
                    lang: &lang,
                    dialogue: &dialogue,
                    tx_tt: &tx_tt,
                    chat_id,
                },
                action,
            )
            .await?;
        }
        None => {
            warn!(data = %data, "Unknown admin callback action");
        }
    }

    Ok(())
}

/// Handle manual ban input from admin.
pub async fn admin_manual_ban_input(
    bot: Bot,
    msg: Message,
    db: Database,
    config: Arc<AppConfig>,
    dialogue: MyDialogue,
) -> HandlerResult {
    let text = msg.text().unwrap_or("");
    let parts: Vec<&str> = text.lines().collect();
    let lang = config.telegram.bot_admin_lang.clone();

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

/// Generate a Telegram deeplink invite token.
pub async fn generate_invite(
    bot: Bot,
    msg: Message,
    db: Database,
    config: Arc<AppConfig>,
) -> HandlerResult {
    if !config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(msg.chat.id.0))
    {
        return Ok(());
    }
    if !config.telegram.telegram_deeplink_registration_enabled {
        bot.send_message(
            msg.chat.id,
            t(config.telegram.bot_admin_lang.as_str(), "deeplink-disabled"),
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
            t(
                config.telegram.bot_admin_lang.as_str(),
                "deeplink-generate-error",
            ),
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
                t(
                    config.telegram.bot_admin_lang.as_str(),
                    "deeplink-generate-error",
                ),
            )
            .await?;
            return Ok(());
        }
    };
    let Some(bot_username) = bot_info.username.clone() else {
        bot.send_message(
            msg.chat.id,
            t(
                config.telegram.bot_admin_lang.as_str(),
                "deeplink-bot-username-missing",
            ),
        )
        .await?;
        return Ok(());
    };
    let link = format!("https://t.me/{bot_username}?start={token}");
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
    let admin_lang = config.telegram.bot_admin_lang.clone();
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
    for &admin_id in &config.telegram.admin_ids {
        if let Ok(sender_id) = i64::try_from(q.from.id.0)
            && admin_id.as_i64() != sender_id
        {
            let _ = bot.send_message(ChatId(admin_id.as_i64()), &text).await;
        }
    }
}

/// Exit command handler.
pub async fn exit_bot(bot: Bot, msg: Message, config: Arc<AppConfig>) -> HandlerResult {
    if !config
        .telegram
        .admin_ids
        .contains(&TelegramId::new(msg.chat.id.0))
    {
        return Ok(());
    }
    bot.send_message(
        msg.chat.id,
        t(config.telegram.bot_admin_lang.as_str(), "bot-shutdown"),
    )
    .await?;
    std::process::exit(0);
}

fn parse_admin_callback(data: &str) -> Option<AdminCallback> {
    if let Some(id) = data.strip_prefix("approve_") {
        return Some(AdminCallback::Approve(id.to_string()));
    }
    if let Some(id) = data.strip_prefix("reject_") {
        return Some(AdminCallback::Reject(id.to_string()));
    }

    let panel = match data {
        "admin_del" => AdminPanelAction::DeleteUsers,
        "admin_banlist_view" => AdminPanelAction::BanlistView,
        "admin_ban_manual" => AdminPanelAction::BanManual,
        "admin_tt_list" => AdminPanelAction::ListTeamTalkUsers,
        "cancel_action" => AdminPanelAction::Cancel,
        _ => {
            if let Some(id) = data.strip_prefix("admin_del_confirm_") {
                let id = id.parse::<i64>().ok()?;
                AdminPanelAction::DeleteConfirm(id)
            } else if let Some(id) = data.strip_prefix("admin_unban_") {
                let id = id.parse::<i64>().ok()?;
                AdminPanelAction::Unban(id)
            } else if let Some(user) = data.strip_prefix("admin_tt_del_prompt_") {
                AdminPanelAction::TeamTalkDeletePrompt(user.to_string())
            } else if let Some(user) = data.strip_prefix("confirm_tt_del_") {
                AdminPanelAction::TeamTalkDeleteConfirm(user.to_string())
            } else {
                return None;
            }
        }
    };

    Some(AdminCallback::Panel(panel))
}

struct AdminApproveInput<'a> {
    bot: &'a Bot,
    q: &'a CallbackQuery,
    db: &'a Database,
    config: &'a AppConfig,
    lang: &'a LanguageCode,
    req_id: &'a str,
    tx_tt: Sender<TTWorkerCommand>,
    chat_id: i64,
}

async fn handle_admin_approve(input: AdminApproveInput<'_>) -> HandlerResult {
    let AdminApproveInput {
        bot,
        q,
        db,
        config,
        lang,
        req_id,
        tx_tt,
        chat_id,
    } = input;
    let Some(pending) = load_pending_approval(bot, q, db, lang, req_id).await? else {
        return Ok(());
    };

    let result = registration::create_teamtalk_account(registration::CreateAccountParams {
        username: &pending.username,
        password: &pending.password,
        nickname: &pending.nickname,
        account_type: TTAccountType::Default,
        source: RegistrationSource::Telegram(pending.registrant_id),
        source_info: Some(pending.source_info.clone()),
        telegram_id: Some(pending.registrant_id),
        tx_tt: tx_tt.clone(),
        db,
        config,
    })
    .await?;

    notify_user_approved(bot, pending.registrant_id, &pending.req_lang).await;
    notify_admin_approve_alert(bot, q, lang, pending.username.as_str()).await?;

    if !result.created {
        notify_admin_approve_failed(bot, chat_id, lang, pending.username.as_str()).await;
        notify_admin_decision(
            bot,
            config,
            q,
            "approved",
            pending.username.as_str(),
            pending.registrant_id,
            &pending.source_info,
        )
        .await;
        db.delete_pending_registration(req_id).await?;
        return Ok(());
    }

    handle_approval_success(
        bot,
        config,
        &pending,
        result.db_sync_error.as_deref(),
        result.assets.as_ref(),
    )
    .await;

    notify_admin_decision(
        bot,
        config,
        q,
        "approved",
        pending.username.as_str(),
        pending.registrant_id,
        &pending.source_info,
    )
    .await;
    db.delete_pending_registration(req_id).await?;
    Ok(())
}

async fn handle_admin_reject(
    bot: &Bot,
    q: &CallbackQuery,
    db: &Database,
    config: &AppConfig,
    lang: &LanguageCode,
    req_id: &str,
) -> HandlerResult {
    if let Ok(Some(req)) = db.get_pending_registration(req_id).await {
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
            bot,
            config,
            q,
            "rejected",
            &username,
            req.registrant_telegram_id,
            &req.source_info,
        )
        .await;
        db.delete_pending_registration(req_id).await?;
    } else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        if let Some(m) = &q.message {
            bot.edit_message_text(m.chat().id, m.id(), t(lang.as_str(), "admin-req-handled"))
                .await?;
        }
    }

    Ok(())
}

async fn load_pending_approval(
    bot: &Bot,
    q: &CallbackQuery,
    db: &Database,
    lang: &LanguageCode,
    req_id: &str,
) -> Result<Option<PendingApproval>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(req) = db.get_pending_registration(req_id).await? else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        if let Some(m) = &q.message {
            bot.edit_message_text(m.chat().id, m.id(), t(lang.as_str(), "admin-req-handled"))
                .await?;
        }
        return Ok(None);
    };

    let Some(username) = Username::parse(&req.username) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };
    let Some(password) = Password::parse(&req.password_cleartext) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };
    let Some(nickname) = Nickname::parse(&req.nickname) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };

    Ok(Some(PendingApproval {
        username,
        password,
        nickname,
        req_lang: parse_source_info(&req.source_info).lang,
        registrant_id: req.registrant_telegram_id,
        source_info: req.source_info.clone(),
    }))
}

async fn notify_user_approved(bot: &Bot, registrant_id: TelegramId, req_lang: &LanguageCode) {
    if let Err(e) = bot
        .send_message(
            ChatId(registrant_id.as_i64()),
            t(req_lang.as_str(), "admin-approved"),
        )
        .await
    {
        warn!(error = %e, "Failed to notify user about approval");
    }
}

async fn notify_admin_approve_alert(
    bot: &Bot,
    q: &CallbackQuery,
    lang: &LanguageCode,
    username: &str,
) -> HandlerResult {
    let alert_args = HashMap::from([("username".to_string(), username.to_string())]);
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
    Ok(())
}

async fn notify_admin_approve_failed(bot: &Bot, chat_id: i64, lang: &LanguageCode, username: &str) {
    if let Err(e) = bot
        .send_message(
            ChatId(chat_id),
            t_args(
                lang.as_str(),
                "admin-approve-failed-critical",
                &HashMap::from([("username".to_string(), username.to_string())]),
            ),
        )
        .await
    {
        warn!(error = %e, "Failed to notify admin about approval failure");
    }
}

async fn handle_approval_success(
    bot: &Bot,
    config: &AppConfig,
    pending: &PendingApproval,
    db_sync_error: Option<&str>,
    assets: Option<&registration::RegistrationAssets>,
) {
    if let Some(err) = db_sync_error {
        notify_db_sync_error(
            bot,
            config,
            ChatId(pending.registrant_id.as_i64()),
            pending.username.as_str(),
            err,
        )
        .await;
        if let Err(e) = bot
            .send_message(
                ChatId(pending.registrant_id.as_i64()),
                t(pending.req_lang.as_str(), "register-success-db-sync-issue"),
            )
            .await
        {
            warn!(error = %e, "Failed to notify user about db sync issue");
        }
    }
    if let Some(assets) = assets
        && let Err(e) = send_registration_assets(
            bot,
            ChatId(pending.registrant_id.as_i64()),
            pending.req_lang.as_str(),
            config,
            pending.username.as_str(),
            pending.password.as_str(),
            assets,
        )
        .await
    {
        warn!(error = %e, "Failed to send registration assets to user");
    }
}

struct AdminPanelContext<'a> {
    bot: &'a Bot,
    msg: &'a Message,
    db: &'a Database,
    lang: &'a LanguageCode,
    dialogue: &'a MyDialogue,
    tx_tt: &'a Sender<TTWorkerCommand>,
    chat_id: i64,
}

async fn handle_admin_panel_action(
    ctx: AdminPanelContext<'_>,
    action: AdminPanelAction,
) -> HandlerResult {
    let AdminPanelContext {
        bot,
        msg,
        db,
        lang,
        dialogue,
        tx_tt,
        chat_id,
    } = ctx;
    match action {
        AdminPanelAction::DeleteUsers => show_admin_delete_users(bot, msg, db, lang).await?,
        AdminPanelAction::DeleteConfirm(target_id) => {
            handle_admin_delete_confirm(bot, msg, db, lang, chat_id, target_id).await?;
        }
        AdminPanelAction::BanlistView => show_admin_banlist(bot, msg, db, lang).await?,
        AdminPanelAction::Unban(target_id) => {
            handle_admin_unban(bot, msg, db, lang, target_id).await?;
        }
        AdminPanelAction::BanManual => {
            bot.send_message(msg.chat.id, t(lang.as_str(), "admin-ban-prompt"))
                .await?;
            dialogue.update(State::AwaitingManualBanInput).await?;
        }
        AdminPanelAction::ListTeamTalkUsers => {
            handle_admin_tt_list(bot, msg, lang, tx_tt).await?;
        }
        AdminPanelAction::TeamTalkDeletePrompt(username) => {
            handle_admin_tt_delete_prompt(bot, msg, lang, &username).await?;
        }
        AdminPanelAction::TeamTalkDeleteConfirm(username) => {
            handle_admin_tt_delete_confirm(bot, msg, lang, tx_tt, &username).await?;
        }
        AdminPanelAction::Cancel => {
            bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-panel-title"))
                .reply_markup(crate::tg_bot::keyboards::admin_panel_keyboard(
                    &t(lang.as_str(), "btn-delete-user"),
                    &t(lang.as_str(), "btn-manage-banlist"),
                    &t(lang.as_str(), "btn-list-tt-accounts"),
                ))
                .await?;
        }
    }
    Ok(())
}

async fn show_admin_delete_users(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
) -> HandlerResult {
    let users = db.get_all_registrations().await?;
    if users.is_empty() {
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-no-users"))
            .await?;
    } else {
        let user_list: Vec<(TelegramId, String)> = users
            .into_iter()
            .map(|u| (u.telegram_id, u.teamtalk_username))
            .collect();
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-select-delete"))
            .reply_markup(crate::tg_bot::keyboards::admin_user_list_keyboard(
                user_list,
            ))
            .await?;
    }
    Ok(())
}

async fn handle_admin_delete_confirm(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
    chat_id: i64,
    target_id: i64,
) -> HandlerResult {
    let tg_id = TelegramId::new(target_id);
    let reg = db.get_registration_by_id(tg_id).await?;
    if db.delete_registration(tg_id).await? {
        let tt_user = reg.map_or_else(|| "Unknown".to_string(), |r| r.teamtalk_username);
        db.ban_user(
            tg_id,
            Some(&tt_user),
            Some(TelegramId::new(chat_id)),
            Some("Deleted via admin panel"),
        )
        .await?;
        let args = HashMap::from([("tg_id".to_string(), target_id.to_string())]);
        bot.edit_message_text(
            msg.chat.id,
            msg.id,
            t_args(lang.as_str(), "admin-user-deleted", &args),
        )
        .await?;
    }
    Ok(())
}

async fn show_admin_banlist(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
) -> HandlerResult {
    let banned = db.get_all_banned_users().await?;
    if banned.is_empty() {
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-banlist-empty"))
            .await?;
        return Ok(());
    }

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
        .edit_message_text(msg.chat.id, msg.id, text)
        .reply_markup(crate::tg_bot::keyboards::admin_banlist_keyboard(
            list,
            &t(lang.as_str(), "btn-unban"),
            &t(lang.as_str(), "btn-add-ban-manual"),
        ))
        .await
        .is_err()
    {
        bot.send_message(msg.chat.id, t(lang.as_str(), "admin-action-refresh-fail"))
            .await?;
    }
    Ok(())
}

async fn handle_admin_unban(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
    target_id: i64,
) -> HandlerResult {
    if target_id == 0 {
        bot.edit_message_text(
            msg.chat.id,
            msg.id,
            t(lang.as_str(), "admin-unban-no-target"),
        )
        .await?;
        return Ok(());
    }
    let args = HashMap::from([("tg_id".to_string(), target_id.to_string())]);
    let edit_result = if db.unban_user(TelegramId::new(target_id)).await? {
        bot.edit_message_text(
            msg.chat.id,
            msg.id,
            t_args(lang.as_str(), "admin-unbanned", &args),
        )
        .await
    } else {
        bot.edit_message_text(
            msg.chat.id,
            msg.id,
            t_args(lang.as_str(), "admin-unban-fail", &args),
        )
        .await
    };
    if edit_result.is_err() {
        bot.send_message(msg.chat.id, t(lang.as_str(), "admin-action-refresh-fail"))
            .await?;
    }
    Ok(())
}

async fn handle_admin_tt_list(
    bot: &Bot,
    msg: &Message,
    lang: &LanguageCode,
    tx_tt: &Sender<TTWorkerCommand>,
) -> HandlerResult {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Err(e) = tx_tt.send(TTWorkerCommand::GetAllUsers { resp: tx }) {
        warn!(error = %e, "Failed to enqueue TeamTalk users list request");
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-tt-list-error"))
            .await?;
        return Ok(());
    }
    match rx.await {
        Ok(users) => {
            if users.is_empty() {
                bot.edit_message_text(
                    msg.chat.id,
                    msg.id,
                    t(lang.as_str(), "admin-tt-no-accounts"),
                )
                .await?;
            } else {
                let mut lines = Vec::new();
                lines.push(t(lang.as_str(), "admin-tt-list-title"));
                for u in &users {
                    lines.push(format!("- {u}"));
                }
                let text = lines.join("\n");
                bot.edit_message_text(msg.chat.id, msg.id, text)
                    .reply_markup(crate::tg_bot::keyboards::admin_tt_accounts_keyboard(
                        users,
                        &t(lang.as_str(), "btn-delete-from-tt"),
                    ))
                    .await?;
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to receive TeamTalk users list");
            bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-tt-list-error"))
                .await?;
        }
    }
    Ok(())
}

async fn handle_admin_tt_delete_prompt(
    bot: &Bot,
    msg: &Message,
    lang: &LanguageCode,
    username: &str,
) -> HandlerResult {
    let args = HashMap::from([("tt_username".to_string(), username.to_string())]);
    bot.edit_message_text(
        msg.chat.id,
        msg.id,
        t_args(lang.as_str(), "admin-tt-delete-prompt", &args),
    )
    .reply_markup(crate::tg_bot::keyboards::confirm_keyboard(
        &t(lang.as_str(), "btn-confirm-delete"),
        &t(lang.as_str(), "btn-cancel"),
        &format!("tt_del_{username}"),
    ))
    .await?;
    Ok(())
}

async fn handle_admin_tt_delete_confirm(
    bot: &Bot,
    msg: &Message,
    lang: &LanguageCode,
    tx_tt: &Sender<TTWorkerCommand>,
    username: &str,
) -> HandlerResult {
    let Some(tt_username) = Username::parse(username) else {
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-tt-list-error"))
            .await?;
        return Ok(());
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Err(e) = tx_tt.send(TTWorkerCommand::DeleteUser {
        username: tt_username,
        resp: tx,
    }) {
        warn!(error = %e, "Failed to enqueue TeamTalk delete user command");
        let mut args = HashMap::from([("tt_username".to_string(), username.to_string())]);
        args.insert("error".to_string(), "Dispatcher error".to_string());
        bot.edit_message_text(
            msg.chat.id,
            msg.id,
            t_args(lang.as_str(), "admin-tt-delete-fail", &args),
        )
        .await?;
        return Ok(());
    }
    let args = HashMap::from([("tt_username".to_string(), username.to_string())]);
    match rx.await {
        Ok(Ok(true)) => {
            bot.edit_message_text(
                msg.chat.id,
                msg.id,
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
                msg.chat.id,
                msg.id,
                t_args(lang.as_str(), "admin-tt-delete-fail", &args),
            )
            .await?;
        }
        Ok(Err(err)) => {
            let mut args = args.clone();
            args.insert("error".to_string(), err);
            bot.edit_message_text(
                msg.chat.id,
                msg.id,
                t_args(lang.as_str(), "admin-tt-delete-fail", &args),
            )
            .await?;
        }
        Err(e) => {
            warn!(error = %e, "Failed to receive TT delete response");
            let mut args = args.clone();
            args.insert("error".to_string(), "Unknown error".to_string());
            bot.edit_message_text(
                msg.chat.id,
                msg.id,
                t_args(lang.as_str(), "admin-tt-delete-fail", &args),
            )
            .await?;
        }
    }
    Ok(())
}
