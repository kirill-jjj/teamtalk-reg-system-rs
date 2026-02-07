use crate::files::get_user_rights_mask;
use crate::i18n::t_args;
use crate::tt::pending_lists::PendingListRequest;
use crate::types::{LanguageCode, OnlineUser, RegistrationSource, TTAccountType, TTWorkerCommand};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use teamtalk::Client;
use teamtalk::types::UserAccount;
use tokio::sync::oneshot;
use tracing::{debug, warn};

pub(super) struct PendingCommand {
    pub(super) resp: oneshot::Sender<Result<bool, String>>,
}

pub(super) struct CommandContext<'a> {
    pub(super) client: &'a Client,
    pub(super) rights: &'a [String],
    pub(super) broadcast_enabled: bool,
    pub(super) admin_lang: &'a LanguageCode,
    pub(super) pending_cmds: &'a mut HashMap<i32, PendingCommand>,
    pub(super) pending_lists: &'a mut HashMap<i32, PendingListRequest>,
    pub(super) is_logged_in: bool,
}

pub(super) fn process_commands(
    rx: &Receiver<TTWorkerCommand>,
    ctx: &mut CommandContext<'_>,
) -> bool {
    match rx.recv_timeout(Duration::from_millis(100)) {
        Ok(cmd) => {
            handle_command(cmd, ctx);
            while let Ok(cmd) = rx.try_recv() {
                handle_command(cmd, ctx);
            }
        }
        Err(RecvTimeoutError::Timeout) => {}
        Err(RecvTimeoutError::Disconnected) => {
            warn!("TT worker command channel disconnected");
            return false;
        }
    }
    true
}

fn handle_command(cmd: TTWorkerCommand, ctx: &mut CommandContext<'_>) {
    if !ctx.is_logged_in {
        handle_command_disconnected(cmd);
        return;
    }

    handle_command_connected(cmd, ctx);
}

fn handle_command_disconnected(cmd: TTWorkerCommand) {
    match cmd {
        TTWorkerCommand::CreateAccount { resp, .. } | TTWorkerCommand::DeleteUser { resp, .. } => {
            warn!("Rejecting TT command: bot not connected");
            let _ = resp.send(Err("Bot not connected to TeamTalk".to_string()));
        }
        TTWorkerCommand::CheckUserExists { resp, .. } => {
            warn!("Rejecting user existence check: bot not connected");
            let _ = resp.send(false);
        }
        TTWorkerCommand::GetAllUsers { resp } => {
            warn!("Rejecting user list request: bot not connected");
            let _ = resp.send(vec![]);
        }
        TTWorkerCommand::GetOnlineUsers { resp } => {
            warn!("Rejecting online users request: bot not connected");
            let _ = resp.send(vec![]);
        }
    }
}

fn handle_command_connected(cmd: TTWorkerCommand, ctx: &mut CommandContext<'_>) {
    match cmd {
        TTWorkerCommand::CreateAccount {
            username,
            password,
            nickname,
            account_type,
            source,
            source_info,
            resp,
        } => handle_create_account(
            CreateAccountInput {
                username,
                password,
                nickname,
                account_type,
                source,
                source_info,
                resp,
            },
            ctx,
        ),
        TTWorkerCommand::DeleteUser { username, resp } => {
            handle_delete_user(ctx, &username, resp);
        }
        TTWorkerCommand::GetAllUsers { resp } => handle_get_all_users(ctx, resp),
        TTWorkerCommand::CheckUserExists { username, resp } => {
            handle_check_user_exists(ctx, username, resp);
        }
        TTWorkerCommand::GetOnlineUsers { resp } => {
            let users = ctx.client.get_server_users();
            let mapped = users
                .into_iter()
                .map(|u| {
                    let user_type = u8::try_from(u.user_type).unwrap_or_else(|_| {
                        warn!(user_type = u.user_type, "User type out of range");
                        u8::MAX
                    });
                    OnlineUser {
                        id: u.id.0,
                        nickname: u.nickname,
                        username: u.username,
                        channel_id: u.channel_id.0,
                        user_type,
                    }
                })
                .collect();
            let _ = resp.send(mapped);
        }
    }
}

struct CreateAccountInput {
    username: crate::domain::Username,
    password: crate::domain::Password,
    nickname: crate::domain::Nickname,
    account_type: TTAccountType,
    source: RegistrationSource,
    source_info: Option<String>,
    resp: oneshot::Sender<Result<bool, String>>,
}

fn handle_create_account(input: CreateAccountInput, ctx: &mut CommandContext<'_>) {
    let CreateAccountInput {
        username,
        password,
        nickname,
        account_type,
        source,
        source_info,
        resp,
    } = input;
    let source_info = source_info.unwrap_or_else(|| match &source {
        RegistrationSource::Telegram(id) => format!("Telegram ID: {id}"),
        RegistrationSource::Web(ip) => format!("Web IP: {ip}"),
    });
    debug!(
        "Sending CreateAccount for '{}'. Source: {}",
        username.as_str(),
        source_info
    );

    let rights_mask = get_user_rights_mask(ctx.rights);

    let user_type = match account_type {
        TTAccountType::Admin => teamtalk::client::ffi::UserType::USERTYPE_ADMIN as u32,
        TTAccountType::Default => teamtalk::client::ffi::UserType::USERTYPE_DEFAULT as u32,
    };

    let mut acc = UserAccount::builder(username.as_str())
        .password(password.as_str())
        .user_type(user_type)
        .rights(rights_mask)
        .build();
    acc.note = format!("Reg via Bot ({source_info}), nick={}", nickname.as_str());

    let cmd_id = ctx.client.create_user_account(&acc);
    if cmd_id > 0 {
        debug!(cmd_id, "CreateAccount dispatched");
        if ctx.broadcast_enabled {
            let args = HashMap::from([("username".to_string(), username.as_str().to_string())]);
            let msg = t_args(ctx.admin_lang.as_str(), "tt-broadcast-registration", &args);
            ctx.client.send_to_all(&msg);
        }
        ctx.pending_cmds.insert(cmd_id, PendingCommand { resp });
    } else {
        warn!("CreateAccount dispatch failed (cmd_id=0)");
        let _ = resp.send(Err("Client error dispatching command".to_string()));
    }
}

fn handle_delete_user(
    ctx: &mut CommandContext<'_>,
    username: &crate::domain::Username,
    resp: oneshot::Sender<Result<bool, String>>,
) {
    debug!(username = %username.as_str(), "Sending DeleteUser");
    let cmd_id = ctx.client.delete_user_account(username.as_str());
    if cmd_id > 0 {
        debug!(cmd_id, "DeleteUser dispatched");
        ctx.pending_cmds.insert(cmd_id, PendingCommand { resp });
    } else {
        warn!(username = %username.as_str(), "DeleteUser dispatch failed (cmd_id=0)");
        let _ = resp.send(Err("Failed to dispatch command".to_string()));
    }
}

fn handle_get_all_users(ctx: &mut CommandContext<'_>, resp: oneshot::Sender<Vec<String>>) {
    debug!("Requesting full user accounts list");
    let cmd_id = ctx.client.list_user_accounts(0, 10000);
    if cmd_id > 0 {
        debug!(cmd_id, "User accounts list dispatched");
        ctx.pending_lists
            .insert(cmd_id, PendingListRequest::all_users(resp));
    } else {
        warn!("User accounts list dispatch failed (cmd_id=0)");
        let _ = resp.send(vec![]);
    }
}

fn handle_check_user_exists(
    ctx: &mut CommandContext<'_>,
    username: crate::domain::Username,
    resp: oneshot::Sender<bool>,
) {
    debug!(username = %username.as_str(), "Requesting account existence check");
    let cmd_id = ctx.client.list_user_accounts(0, 10000);
    if cmd_id > 0 {
        debug!(cmd_id, "User accounts list dispatched for existence check");
        ctx.pending_lists
            .insert(cmd_id, PendingListRequest::exists(username, resp));
    } else {
        warn!("User accounts list dispatch failed (cmd_id=0)");
        let _ = resp.send(false);
    }
}
