use crate::config::AppConfig;
use crate::db::Database;
use crate::files::get_user_rights_mask;
use crate::i18n::t_args;
use crate::types::{LanguageCode, OnlineUser, RegistrationSource, TTAccountType, TTWorkerCommand};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use teamtalk::client::{ConnectParams, ReconnectConfig, ReconnectHandler};
use teamtalk::types::{UserAccount, UserGender, UserPresence, UserStatus};
use teamtalk::{Client, Event};
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tracing::instrument;
use tracing::{debug, error, info, warn};

struct PendingCommand {
    resp: oneshot::Sender<Result<bool, String>>,
}

struct PendingListRequest {
    resp: oneshot::Sender<Vec<String>>,
    accumulated: Vec<String>,
}

struct CommandContext<'a> {
    client: &'a Client,
    rights: &'a [String],
    broadcast_enabled: bool,
    admin_lang: &'a LanguageCode,
    pending_cmds: &'a mut HashMap<i32, PendingCommand>,
    pending_lists: &'a mut HashMap<i32, PendingListRequest>,
    is_logged_in: bool,
}

fn handle_command(cmd: TTWorkerCommand, ctx: &mut CommandContext<'_>) {
    if !ctx.is_logged_in {
        match cmd {
            TTWorkerCommand::CreateAccount { resp, .. }
            | TTWorkerCommand::DeleteUser { resp, .. } => {
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
        return;
    }

    match cmd {
        TTWorkerCommand::CreateAccount {
            username,
            password,
            nickname,
            account_type,
            source,
            source_info,
            resp,
        } => {
            let source_info = source_info.unwrap_or_else(|| match &source {
                RegistrationSource::Telegram(id) => format!("Telegram ID: {}", id),
                RegistrationSource::Web(ip) => format!("Web IP: {}", ip),
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
            acc.note = format!("Reg via Bot ({}), nick={}", source_info, nickname.as_str());

            let cmd_id = ctx.client.create_user_account(&acc);
            if cmd_id > 0 {
                debug!(cmd_id, "CreateAccount dispatched");
                if ctx.broadcast_enabled {
                    let args =
                        HashMap::from([("username".to_string(), username.as_str().to_string())]);
                    let msg = t_args(ctx.admin_lang.as_str(), "tt-broadcast-registration", &args);
                    ctx.client.send_to_all(&msg);
                }
                ctx.pending_cmds.insert(cmd_id, PendingCommand { resp });
            } else {
                warn!("CreateAccount dispatch failed (cmd_id=0)");
                let _ = resp.send(Err("Client error dispatching command".to_string()));
            }
        }
        TTWorkerCommand::DeleteUser { username, resp } => {
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
        TTWorkerCommand::GetAllUsers { resp } => {
            debug!("Requesting full user accounts list");
            let cmd_id = ctx.client.list_user_accounts(0, 10000);
            if cmd_id > 0 {
                debug!(cmd_id, "User accounts list dispatched");
                ctx.pending_lists.insert(
                    cmd_id,
                    PendingListRequest {
                        resp,
                        accumulated: Vec::new(),
                    },
                );
            } else {
                warn!("User accounts list dispatch failed (cmd_id=0)");
                let _ = resp.send(vec![]);
            }
        }
        TTWorkerCommand::CheckUserExists { username, resp } => {
            let exists = ctx.client.get_user_by_username(username.as_str()).is_some();
            let _ = resp.send(exists);
        }
        TTWorkerCommand::GetOnlineUsers { resp } => {
            let users = ctx.client.get_server_users();
            let mapped = users
                .into_iter()
                .map(|u| OnlineUser {
                    id: u.id.0,
                    nickname: u.nickname,
                    username: u.username,
                    channel_id: u.channel_id.0,
                    user_type: u.user_type as u8,
                })
                .collect();
            let _ = resp.send(mapped);
        }
    }
}

#[instrument(skip(config, rx, bot, db, rt_handle))]
pub async fn run_tt_worker(
    config: Arc<AppConfig>,
    rx: Receiver<TTWorkerCommand>,
    bot: Bot,
    db: Database,
    rt_handle: Handle,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let host = config.host_name.clone();
    let tcp_port = config.tcp_port;
    let udp_port = config.udp_port.unwrap_or(config.tcp_port);
    let encrypted = config.encrypted;
    let nickname = config.nick_name.clone();
    let username = config.user_name.clone();
    let password = config.password.clone();
    let client_name = config.client_name.clone();
    let rights = config.teamtalk_default_user_rights.clone();
    let broadcast_enabled = config.teamtalk_registration_broadcast_enabled;
    let admin_ids = config.admin_ids.clone();
    let admin_lang = config.bot_admin_lang.clone();

    let tt_gender_str = config.tt_gender.clone();
    let tt_status_text = config.tt_status_text.clone();

    let pending_deletions: Arc<Mutex<HashMap<String, AbortHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));

    std::thread::spawn(move || {
        let client = match Client::new() {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "Failed to init TeamTalk client");
                return;
            }
        };

        let mut reconnect = ReconnectHandler::new(ReconnectConfig::default());
        let connect_params = ConnectParams {
            host: &host,
            tcp: tcp_port,
            udp: udp_port,
            encrypted,
        };

        info!(host = %host, tcp_port, "Connecting to TeamTalk server");
        let _ = client.connect(
            connect_params.host,
            connect_params.tcp,
            connect_params.udp,
            connect_params.encrypted,
        );

        let mut is_logged_in = false;
        let mut pending_cmds: HashMap<i32, PendingCommand> = HashMap::new();
        let mut pending_lists: HashMap<i32, PendingListRequest> = HashMap::new();

        loop {
            if shutdown.is_cancelled() {
                let _ = client.disconnect();
                break;
            }
            let mut ctx = CommandContext {
                client: &client,
                rights: &rights,
                broadcast_enabled,
                admin_lang: &admin_lang,
                pending_cmds: &mut pending_cmds,
                pending_lists: &mut pending_lists,
                is_logged_in,
            };
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => {
                    handle_command(cmd, &mut ctx);
                    while let Ok(cmd) = rx.try_recv() {
                        handle_command(cmd, &mut ctx);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    warn!("TT worker command channel disconnected");
                    break;
                }
            }

            while let Some((event, msg)) = client.poll(0) {
                match event {
                    Event::ConnectSuccess => {
                        info!("Connected. Logging in");
                        reconnect.mark_connected();
                        client.login(&nickname, &username, &password, &client_name);
                    }
                    Event::ConnectFailed | Event::ConnectionLost => {
                        warn!("Connection lost");
                        is_logged_in = false;
                        reconnect.mark_disconnected();
                        for (_, cmd) in pending_cmds.drain() {
                            let _ = cmd.resp.send(Err("Connection lost".to_string()));
                        }
                        let pending_count = pending_lists.len();
                        for (_, req) in pending_lists.drain() {
                            let _ = req.resp.send(vec![]);
                        }
                        if pending_count > 0 {
                            warn!(pending_count, "Dropped pending list requests on disconnect");
                        }
                    }
                    Event::MySelfLoggedIn => {
                        info!("Logged in as bot");
                        is_logged_in = true;

                        let gender = match tt_gender_str.to_lowercase().as_str() {
                            "male" => UserGender::Male,
                            "female" => UserGender::Female,
                            _ => UserGender::Neutral,
                        };

                        let status_mode = UserStatus {
                            gender,
                            presence: UserPresence::Available,
                            ..Default::default()
                        };

                        client.set_status(status_mode, &tt_status_text);
                        client.subscribe(client.my_id(), teamtalk::types::Subscriptions::all());
                    }
                    Event::CmdSuccess => {
                        let cmd_id = msg.source();
                        debug!(cmd_id, "Command succeeded");
                        if let Some(cmd) = pending_cmds.remove(&cmd_id) {
                            let _ = cmd.resp.send(Ok(true));
                        }
                        if let Some(req) = pending_lists.remove(&cmd_id) {
                            let _ = req.resp.send(req.accumulated);
                        }
                    }
                    Event::CmdError => {
                        let cmd_id = msg.source();
                        warn!(cmd_id, "Command failed on TeamTalk server");
                        if let Some(cmd) = pending_cmds.remove(&cmd_id) {
                            let _ = cmd.resp.send(Err("Command failed on server".to_string()));
                        }
                        if let Some(req) = pending_lists.remove(&cmd_id) {
                            let _ = req.resp.send(vec![]);
                        }
                    }
                    Event::UserAccount => {
                        let cmd_id = msg.source();
                        if let Some(req) = pending_lists.get_mut(&cmd_id)
                            && let Some(acc) = msg.account()
                        {
                            debug!(cmd_id, username = %acc.username, "Received user account");
                            req.accumulated.push(acc.username.clone());
                        }
                    }

                    Event::UserAccountCreated => {
                        if is_logged_in && let Some(acc) = msg.account() {
                            let u_name = acc.username.clone();
                            let bot_clone = bot.clone();
                            let admins_clone = admin_ids.clone();
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
                                let args = HashMap::from([(
                                    "account_username_str".to_string(),
                                    u_name.clone(),
                                )]);
                                let msg_text = t_args(lang_clone.as_str(), msg_key, &args);

                                for &aid in &admins_clone {
                                    let _ = bot_clone
                                        .send_message(ChatId(aid.as_i64()), &msg_text)
                                        .await;
                                }
                            });
                        }
                    }

                    Event::UserAccountRemoved => {
                        if let Some(acc) = msg.account() {
                            let u_name = acc.username.clone();
                            debug!(
                                username = %u_name,
                                "User removed from TeamTalk. Starting debounce timer"
                            );

                            let db_clone = db.clone();
                            let bot_clone = bot.clone();
                            let admins_clone = admin_ids.clone();
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
                                    let _ =
                                        bot_clone.send_message(ChatId(aid.as_i64()), &removed_text).await;
                                }

                                if let Ok(Some(reg)) =
                                    db_clone.get_registration_by_tt_username(&u_name_cl).await
                                {
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
                                    let text =
                                        t_args(lang_clone.as_str(), "tt-account-removed-banned", &args);

                                    for &aid in &admins_clone {
                                        let _ = bot_clone.send_message(ChatId(aid.as_i64()), &text).await;
                                    }
                                } else {
                                    let args = HashMap::from([("username".to_string(), u_name_cl)]);
                                    let text =
                                        t_args(lang_clone.as_str(), "tt-account-removed-no-link", &args);

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
                    }
                    _ => {}
                }
            }

            if !is_logged_in && !client.is_connected() && !client.is_connecting() {
                client.handle_reconnect(&connect_params, &mut reconnect);
            }
        }
    });
}
