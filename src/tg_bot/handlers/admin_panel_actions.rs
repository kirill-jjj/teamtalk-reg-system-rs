use super::HandlerResult;
use crate::db::Database;
use crate::domain::Username;
use crate::i18n::{t, t_args};
use crate::types::{LanguageCode, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use teloxide_ng::prelude::*;
use tracing::warn;

const ADMIN_PAGE_SIZE: usize = 20;

pub(super) async fn show_admin_delete_users(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
    page: usize,
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
        let (page_items, total_pages, page_index) = paginate(&user_list, page, ADMIN_PAGE_SIZE);
        let prev_label = t(lang.as_str(), "btn-prev-page");
        let next_label = t(lang.as_str(), "btn-next-page");
        let nav_row = crate::tg_bot::keyboards::pagination_row(
            &prev_label,
            &next_label,
            if page_index > 0 {
                Some(format!("admin_del_page_{}", page_index - 1))
            } else {
                None
            },
            if page_index + 1 < total_pages {
                Some(format!("admin_del_page_{}", page_index + 1))
            } else {
                None
            },
        );
        let mut text = t(lang.as_str(), "admin-select-delete");
        if total_pages > 1 {
            let suffix = t_args(
                lang.as_str(),
                "admin-list-page",
                &HashMap::from([
                    ("page".to_string(), (page_index + 1).to_string()),
                    ("pages".to_string(), total_pages.to_string()),
                ]),
            );
            text.push('\n');
            text.push_str(&suffix);
        }
        bot.edit_message_text(msg.chat.id, msg.id, text)
            .reply_markup(crate::tg_bot::keyboards::admin_user_list_keyboard(
                page_items, nav_row,
            ))
            .await?;
    }
    Ok(())
}

pub(super) async fn handle_admin_delete_confirm(
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

pub(super) async fn show_admin_banlist(
    bot: &Bot,
    msg: &Message,
    db: &Database,
    lang: &LanguageCode,
    page: usize,
) -> HandlerResult {
    let banned = db.get_all_banned_users().await?;
    if banned.is_empty() {
        bot.edit_message_text(msg.chat.id, msg.id, t(lang.as_str(), "admin-banlist-empty"))
            .await?;
        return Ok(());
    }

    let mut lines = Vec::new();
    lines.push(t(lang.as_str(), "admin-banlist-title"));
    let list: Vec<(TelegramId, String, String)> = banned
        .into_iter()
        .map(|b| {
            let tt_user = b.teamtalk_username.unwrap_or_else(|| "N/A".to_string());
            let reason = b.reason.unwrap_or_else(|| "N/A".to_string());
            (b.telegram_id, tt_user, reason)
        })
        .collect();
    let (page_items, total_pages, page_index) = paginate(&list, page, ADMIN_PAGE_SIZE);
    for (tg_id, tt_user, reason) in &page_items {
        lines.push(format!(
            "TG ID: {tg_id} - TT User: {tt_user} (Reason: {reason})"
        ));
    }
    if total_pages > 1 {
        lines.push(t_args(
            lang.as_str(),
            "admin-list-page",
            &HashMap::from([
                ("page".to_string(), (page_index + 1).to_string()),
                ("pages".to_string(), total_pages.to_string()),
            ]),
        ));
    }
    let text = lines.join("\n");
    let prev_label = t(lang.as_str(), "btn-prev-page");
    let next_label = t(lang.as_str(), "btn-next-page");
    let nav_row = crate::tg_bot::keyboards::pagination_row(
        &prev_label,
        &next_label,
        if page_index > 0 {
            Some(format!("admin_banlist_page_{}", page_index - 1))
        } else {
            None
        },
        if page_index + 1 < total_pages {
            Some(format!("admin_banlist_page_{}", page_index + 1))
        } else {
            None
        },
    );
    if bot
        .edit_message_text(msg.chat.id, msg.id, text)
        .reply_markup(crate::tg_bot::keyboards::admin_banlist_keyboard(
            page_items
                .iter()
                .map(|(tg_id, _, reason)| (*tg_id, reason.clone()))
                .collect(),
            &t(lang.as_str(), "btn-unban"),
            &t(lang.as_str(), "btn-add-ban-manual"),
            nav_row,
        ))
        .await
        .is_err()
    {
        bot.send_message(msg.chat.id, t(lang.as_str(), "admin-action-refresh-fail"))
            .await?;
    }
    Ok(())
}

pub(super) async fn handle_admin_unban(
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

pub(super) async fn handle_admin_tt_list(
    bot: &Bot,
    msg: &Message,
    lang: &LanguageCode,
    tx_tt: &Sender<TTWorkerCommand>,
    page: usize,
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
                let (page_items, total_pages, page_index) = paginate(&users, page, ADMIN_PAGE_SIZE);
                for u in &page_items {
                    lines.push(format!("- {u}"));
                }
                if total_pages > 1 {
                    lines.push(t_args(
                        lang.as_str(),
                        "admin-list-page",
                        &HashMap::from([
                            ("page".to_string(), (page_index + 1).to_string()),
                            ("pages".to_string(), total_pages.to_string()),
                        ]),
                    ));
                }
                let text = lines.join("\n");
                let prev_label = t(lang.as_str(), "btn-prev-page");
                let next_label = t(lang.as_str(), "btn-next-page");
                let nav_row = crate::tg_bot::keyboards::pagination_row(
                    &prev_label,
                    &next_label,
                    if page_index > 0 {
                        Some(format!("admin_tt_list_page_{}", page_index - 1))
                    } else {
                        None
                    },
                    if page_index + 1 < total_pages {
                        Some(format!("admin_tt_list_page_{}", page_index + 1))
                    } else {
                        None
                    },
                );
                bot.edit_message_text(msg.chat.id, msg.id, text)
                    .reply_markup(crate::tg_bot::keyboards::admin_tt_accounts_keyboard(
                        page_items,
                        &t(lang.as_str(), "btn-delete-from-tt"),
                        nav_row,
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

pub(super) async fn handle_admin_tt_delete_prompt(
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

pub(super) async fn handle_admin_tt_delete_confirm(
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

fn paginate<T: Clone>(items: &[T], page: usize, page_size: usize) -> (Vec<T>, usize, usize) {
    if items.is_empty() {
        return (Vec::new(), 0, 0);
    }
    let total_pages = items.len().div_ceil(page_size);
    let page_index = page.min(total_pages.saturating_sub(1));
    let start = page_index * page_size;
    let end = (start + page_size).min(items.len());
    (items[start..end].to_vec(), total_pages, page_index)
}
