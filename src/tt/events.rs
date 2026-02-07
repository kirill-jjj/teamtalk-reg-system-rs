use crate::db::Database;
use crate::i18n::t_args;
use crate::tt::commands::PendingCommand;
use crate::tt::pending_lists::{self, PendingListRequest};
use crate::types::{LanguageCode, TelegramId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::runtime::Handle;
use tokio::task::AbortHandle;
use tracing::{debug, warn};

pub(super) fn handle_cmd_success(
    msg: &teamtalk::Message,
    pending_cmds: &mut HashMap<i32, PendingCommand>,
    pending_lists: &mut HashMap<i32, PendingListRequest>,
) {
    let cmd_id = msg.source();
    debug!(cmd_id, "Command succeeded");
    if let Some(cmd) = pending_cmds.remove(&cmd_id) {
        let _ = cmd.resp.send(Ok(true));
    }
    pending_lists::mark_command_completed(cmd_id, pending_lists);
}

pub(super) fn handle_cmd_error(
    msg: &teamtalk::Message,
    pending_cmds: &mut HashMap<i32, PendingCommand>,
    pending_lists: &mut HashMap<i32, PendingListRequest>,
) {
    let cmd_id = msg.source();
    log_cmd_error(cmd_id, msg);
    if let Some(cmd) = pending_cmds.remove(&cmd_id) {
        let _ = cmd.resp.send(Err("Command failed on server".to_string()));
    }
    pending_lists::fail_command(cmd_id, pending_lists);
}

pub(super) fn handle_user_account_created(
    msg: &teamtalk::Message,
    is_logged_in: bool,
    bot: &Bot,
    admin_ids: &[TelegramId],
    admin_lang: &LanguageCode,
    pending_deletions: &Arc<Mutex<HashMap<String, AbortHandle>>>,
    rt_handle: &Handle,
) {
    if !is_logged_in {
        return;
    }
    let Some(acc) = msg.account() else {
        return;
    };
    let u_name = acc.username;
    let bot_clone = bot.clone();
    let admins_clone = admin_ids.to_vec();
    let pending_dels = pending_deletions.clone();
    let lang_clone = admin_lang.clone();

    rt_handle.spawn(async move {
        let mut is_update = false;
        if let Ok(mut lock) = pending_dels.lock() {
            if let Some(abort_handle) = lock.remove(&u_name) {
                abort_handle.abort();
                is_update = true;
                debug!(
                    username = %u_name,
                    "User recreated or updated quickly. Cancelled ban timer"
                );
            }
        } else {
            warn!(username = %u_name, "Failed to lock pending deletions");
        }

        let msg_key = if is_update {
            "tt-account-changed"
        } else {
            "tt-account-created"
        };
        let args = HashMap::from([("account_username_str".to_string(), u_name.clone())]);
        let msg_text = t_args(lang_clone.as_str(), msg_key, &args);

        for &aid in &admins_clone {
            let _ = bot_clone
                .send_message(ChatId(aid.as_i64()), &msg_text)
                .await;
        }
    });
}

pub(super) fn handle_user_account_removed(
    msg: &teamtalk::Message,
    bot: &Bot,
    db: &Database,
    admin_ids: &[TelegramId],
    admin_lang: &LanguageCode,
    pending_deletions: &Arc<Mutex<HashMap<String, AbortHandle>>>,
    rt_handle: &Handle,
) {
    let Some(acc) = msg.account() else {
        return;
    };
    let u_name = acc.username;
    debug!(
        username = %u_name,
        "User removed from TeamTalk. Starting debounce timer"
    );

    let db_clone = db.clone();
    let bot_clone = bot.clone();
    let admins_clone = admin_ids.to_vec();
    let pending_dels = pending_deletions.clone();
    let u_name_cl = u_name.clone();
    let lang_clone = admin_lang.clone();

    let task = rt_handle.spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;

        if let Ok(mut lock) = pending_dels.lock() {
            lock.remove(&u_name_cl);
        } else {
            warn!(username = %u_name_cl, "Failed to lock pending deletions");
        }

        debug!(
            username = %u_name_cl,
            "Timer passed. Auto-banning user associated with account"
        );

        let removed_text = t_args(
            lang_clone.as_str(),
            "tt-account-removed",
            &HashMap::from([("username".to_string(), u_name_cl.clone())]),
        );
        for &aid in &admins_clone {
            let _ = bot_clone
                .send_message(ChatId(aid.as_i64()), &removed_text)
                .await;
        }

        if let Ok(Some(reg)) = db_clone.get_registration_by_tt_username(&u_name_cl).await {
            let _ = db_clone
                .ban_user(
                    reg.telegram_id,
                    Some(&u_name_cl),
                    None,
                    Some("Account deleted from TeamTalk server"),
                )
                .await;

            let args = HashMap::from([
                ("username".to_string(), u_name_cl),
                ("tg_id".to_string(), reg.telegram_id.to_string()),
            ]);
            let text = t_args(lang_clone.as_str(), "tt-account-removed-banned", &args);

            for &aid in &admins_clone {
                let _ = bot_clone.send_message(ChatId(aid.as_i64()), &text).await;
            }
        } else {
            let args = HashMap::from([("username".to_string(), u_name_cl)]);
            let text = t_args(lang_clone.as_str(), "tt-account-removed-no-link", &args);

            for &aid in &admins_clone {
                let _ = bot_clone.send_message(ChatId(aid.as_i64()), &text).await;
            }
        }
    });

    if let Ok(mut lock) = pending_deletions.lock() {
        lock.insert(u_name, task.abort_handle());
    } else {
        warn!(username = %u_name, "Failed to lock pending deletions");
    }
}

fn log_cmd_error(cmd_id: i32, msg: &teamtalk::Message) {
    let raw = msg.raw();
    let tt_type = raw.ttType as i32;
    if tt_type == teamtalk::client::ffi::TTType::__CLIENTERRORMSG as i32 {
        let err =
            unsafe { teamtalk::types::ErrorMessage::from(raw.__bindgen_anon_1.clienterrormsg) };
        warn!(
            cmd_id,
            code = err.code,
            message = %err.message,
            "Command failed on TeamTalk server"
        );
    } else {
        warn!(cmd_id, tt_type, "Command failed on TeamTalk server");
    }
}
