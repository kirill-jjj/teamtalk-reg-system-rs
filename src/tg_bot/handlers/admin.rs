use super::admin_approvals::{AdminApproveInput, handle_admin_approve, handle_admin_reject};
use super::{HandlerResult, MyDialogue, State};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::tg_bot::handlers::admin_panel_actions::{
    handle_admin_delete_confirm, handle_admin_tt_delete_confirm, handle_admin_tt_delete_prompt,
    handle_admin_tt_list, handle_admin_unban, show_admin_banlist, show_admin_delete_users,
};
use crate::types::{LanguageCode, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use teloxide_ng::prelude::*;
use tracing::warn;
use uuid::Uuid;

enum AdminCallback {
    Approve(String),
    Reject(String),
    Panel(AdminPanelAction),
}

enum AdminPanelAction {
    DeleteUsers,
    DeleteUsersPage(usize),
    DeleteConfirm(i64),
    BanlistView,
    BanlistPage(usize),
    Unban(i64),
    BanManual,
    ListTeamTalkUsers,
    ListTeamTalkUsersPage(usize),
    TeamTalkDeletePrompt(String),
    TeamTalkDeleteConfirm(String),
    Cancel,
}

pub(super) struct PendingApproval {
    pub(super) username: Username,
    pub(super) password: Password,
    pub(super) nickname: Nickname,
    pub(super) req_lang: LanguageCode,
    pub(super) registrant_id: TelegramId,
    pub(super) source_info: String,
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

/// Exit command handler.
pub async fn exit_bot(
    bot: Bot,
    msg: Message,
    config: Arc<AppConfig>,
    shutdown: tokio_util::sync::CancellationToken,
) -> HandlerResult {
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
    shutdown.cancel();
    Ok(())
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
            } else if let Some(page) = data.strip_prefix("admin_del_page_") {
                let page = page.parse::<usize>().ok()?;
                AdminPanelAction::DeleteUsersPage(page)
            } else if let Some(id) = data.strip_prefix("admin_unban_") {
                let id = id.parse::<i64>().ok()?;
                AdminPanelAction::Unban(id)
            } else if let Some(page) = data.strip_prefix("admin_banlist_page_") {
                let page = page.parse::<usize>().ok()?;
                AdminPanelAction::BanlistPage(page)
            } else if let Some(user) = data.strip_prefix("admin_tt_del_prompt_") {
                AdminPanelAction::TeamTalkDeletePrompt(user.to_string())
            } else if let Some(user) = data.strip_prefix("confirm_tt_del_") {
                AdminPanelAction::TeamTalkDeleteConfirm(user.to_string())
            } else if let Some(page) = data.strip_prefix("admin_tt_list_page_") {
                let page = page.parse::<usize>().ok()?;
                AdminPanelAction::ListTeamTalkUsersPage(page)
            } else {
                return None;
            }
        }
    };

    Some(AdminCallback::Panel(panel))
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
        AdminPanelAction::DeleteUsers => show_admin_delete_users(bot, msg, db, lang, 0).await?,
        AdminPanelAction::DeleteUsersPage(page) => {
            show_admin_delete_users(bot, msg, db, lang, page).await?;
        }
        AdminPanelAction::DeleteConfirm(target_id) => {
            handle_admin_delete_confirm(bot, msg, db, lang, chat_id, target_id).await?;
        }
        AdminPanelAction::BanlistView => show_admin_banlist(bot, msg, db, lang, 0).await?,
        AdminPanelAction::BanlistPage(page) => {
            show_admin_banlist(bot, msg, db, lang, page).await?;
        }
        AdminPanelAction::Unban(target_id) => {
            handle_admin_unban(bot, msg, db, lang, target_id).await?;
        }
        AdminPanelAction::BanManual => {
            bot.send_message(msg.chat.id, t(lang.as_str(), "admin-ban-prompt"))
                .await?;
            dialogue.update(State::AwaitingManualBanInput).await?;
        }
        AdminPanelAction::ListTeamTalkUsers => {
            handle_admin_tt_list(bot, msg, lang, tx_tt, 0).await?;
        }
        AdminPanelAction::ListTeamTalkUsersPage(page) => {
            handle_admin_tt_list(bot, msg, lang, tx_tt, page).await?;
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
