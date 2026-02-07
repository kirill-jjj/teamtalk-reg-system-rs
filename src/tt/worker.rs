use crate::config::AppConfig;
use crate::db::Database;
use crate::i18n::t_args;
use crate::tt::commands::{self, CommandContext, PendingCommand};
use crate::tt::pending_lists::{self, PendingListRequest};
use crate::types::{LanguageCode, TTWorkerCommand, TelegramId};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use teamtalk::client::{ConnectParams, ReconnectConfig, ReconnectHandler};
use teamtalk::types::{ErrorMessage, UserGender, UserPresence, UserStatus};
use teamtalk::{Client, Event};
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::runtime::Handle;
use tokio::task::AbortHandle;
use tracing::instrument;
use tracing::{debug, error, info, warn};

struct TTWorkerConfig {
    host: String,
    tcp_port: i32,
    udp_port: i32,
    encrypted: bool,
    nickname: String,
    username: String,
    password: String,
    client_name: String,
    rights: Vec<String>,
    broadcast_enabled: bool,
    admin_ids: Vec<TelegramId>,
    admin_lang: LanguageCode,
    tt_gender_str: String,
    tt_status_text: String,
}

struct TTWorkerRuntime {
    config: TTWorkerConfig,
    rx: Receiver<TTWorkerCommand>,
    bot: Bot,
    db: Database,
    rt_handle: Handle,
    shutdown: tokio_util::sync::CancellationToken,
    pending_deletions: Arc<Mutex<HashMap<String, AbortHandle>>>,
}

/// Run the `TeamTalk` worker loop.
#[instrument(skip(config, rx, bot, db, rt_handle))]
pub async fn run_tt_worker(
    config: Arc<AppConfig>,
    rx: Receiver<TTWorkerCommand>,
    bot: Bot,
    db: Database,
    rt_handle: Handle,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let host = config.teamtalk.host_name.clone();
    let tcp_port = config.teamtalk.tcp_port;
    let udp_port = config.teamtalk.udp_port.unwrap_or(config.teamtalk.tcp_port);
    let encrypted = config.teamtalk.encrypted;
    let nickname = config.teamtalk.nick_name.clone();
    let username = config.teamtalk.user_name.clone();
    let password = config.teamtalk.password.clone();
    let client_name = config.teamtalk.client_name.clone();
    let rights = config.teamtalk.teamtalk_default_user_rights.clone();
    let broadcast_enabled = config.teamtalk.teamtalk_registration_broadcast_enabled;
    let admin_ids = config.telegram.admin_ids.clone();
    let admin_lang = config.telegram.bot_admin_lang.clone();

    let tt_gender_str = config.teamtalk.tt_gender.clone();
    let tt_status_text = config.teamtalk.tt_status_text.clone();

    let pending_deletions: Arc<Mutex<HashMap<String, AbortHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let worker_config = TTWorkerConfig {
        host,
        tcp_port,
        udp_port,
        encrypted,
        nickname,
        username,
        password,
        client_name,
        rights,
        broadcast_enabled,
        admin_ids,
        admin_lang,
        tt_gender_str,
        tt_status_text,
    };

    std::thread::spawn(move || {
        run_tt_loop(TTWorkerRuntime {
            config: worker_config,
            rx,
            bot,
            db,
            rt_handle,
            shutdown,
            pending_deletions,
        });
    });
}

fn run_tt_loop(runtime: TTWorkerRuntime) {
    let TTWorkerRuntime {
        config,
        rx,
        bot,
        db,
        rt_handle,
        shutdown,
        pending_deletions,
    } = runtime;
    let client = match Client::new() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to init TeamTalk client");
            return;
        }
    };

    let mut reconnect = ReconnectHandler::new(ReconnectConfig::default());
    let connect_params = ConnectParams {
        host: &config.host,
        tcp: config.tcp_port,
        udp: config.udp_port,
        encrypted: config.encrypted,
    };

    info!(host = %config.host, tcp_port = config.tcp_port, "Connecting to TeamTalk server");
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
            rights: &config.rights,
            broadcast_enabled: config.broadcast_enabled,
            admin_lang: &config.admin_lang,
            pending_cmds: &mut pending_cmds,
            pending_lists: &mut pending_lists,
            is_logged_in,
        };
        if !commands::process_commands(&rx, &mut ctx) {
            break;
        }

        while let Some((event, msg)) = client.poll(0) {
            match event {
                Event::ConnectSuccess => handle_connect_success(&client, &mut reconnect, &config),
                Event::ConnectFailed | Event::ConnectionLost => {
                    handle_connection_lost(
                        &mut reconnect,
                        &mut is_logged_in,
                        &mut pending_cmds,
                        &mut pending_lists,
                    );
                }
                Event::MySelfLoggedIn => {
                    handle_logged_in(&client, &mut is_logged_in, &config);
                }
                Event::CmdSuccess => {
                    handle_cmd_success(&msg, &mut pending_cmds, &mut pending_lists);
                }
                Event::CmdError => handle_cmd_error(&msg, &mut pending_cmds, &mut pending_lists),
                Event::UserAccount => {
                    pending_lists::handle_user_account_event(&msg, &mut pending_lists);
                }
                Event::UserAccountCreated => handle_user_account_created(
                    &msg,
                    is_logged_in,
                    &bot,
                    &config,
                    &pending_deletions,
                    &rt_handle,
                ),
                Event::UserAccountRemoved => handle_user_account_removed(
                    &msg,
                    &bot,
                    &db,
                    &config,
                    &pending_deletions,
                    &rt_handle,
                ),
                _ => {}
            }
        }

        pending_lists::flush_completed(&mut pending_lists);

        if !is_logged_in && !client.is_connected() && !client.is_connecting() {
            client.handle_reconnect(&connect_params, &mut reconnect);
        }
    }
}

fn handle_connect_success(
    client: &Client,
    reconnect: &mut ReconnectHandler,
    config: &TTWorkerConfig,
) {
    info!("Connected. Logging in");
    reconnect.mark_connected();
    client.login(
        &config.nickname,
        &config.username,
        &config.password,
        &config.client_name,
    );
}

fn handle_connection_lost(
    reconnect: &mut ReconnectHandler,
    is_logged_in: &mut bool,
    pending_cmds: &mut HashMap<i32, PendingCommand>,
    pending_lists: &mut HashMap<i32, PendingListRequest>,
) {
    warn!("Connection lost");
    *is_logged_in = false;
    reconnect.mark_disconnected();
    for (_, cmd) in pending_cmds.drain() {
        let _ = cmd.resp.send(Err("Connection lost".to_string()));
    }
    let pending_count = pending_lists.len();
    let pending_ids: Vec<i32> = pending_lists.keys().copied().collect();
    for cmd_id in pending_ids {
        pending_lists::fail_command(cmd_id, pending_lists);
    }
    if pending_count > 0 {
        warn!(pending_count, "Dropped pending list requests on disconnect");
    }
}

fn handle_logged_in(client: &Client, is_logged_in: &mut bool, config: &TTWorkerConfig) {
    info!("Logged in as bot");
    *is_logged_in = true;

    let gender = match config.tt_gender_str.to_lowercase().as_str() {
        "male" => UserGender::Male,
        "female" => UserGender::Female,
        _ => UserGender::Neutral,
    };

    let status_mode = UserStatus {
        gender,
        presence: UserPresence::Available,
        ..Default::default()
    };

    client.set_status(status_mode, &config.tt_status_text);
    client.subscribe(client.my_id(), teamtalk::types::Subscriptions::all());
}

fn handle_cmd_success(
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

fn handle_cmd_error(
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

fn log_cmd_error(cmd_id: i32, msg: &teamtalk::Message) {
    let raw = msg.raw();
    let tt_type = raw.ttType as i32;
    if tt_type == teamtalk::client::ffi::TTType::__CLIENTERRORMSG as i32 {
        let err = unsafe { ErrorMessage::from(raw.__bindgen_anon_1.clienterrormsg) };
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

fn handle_user_account_created(
    msg: &teamtalk::Message,
    is_logged_in: bool,
    bot: &Bot,
    config: &TTWorkerConfig,
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
    let admins_clone = config.admin_ids.clone();
    let pending_dels = pending_deletions.clone();
    let lang_clone = config.admin_lang.clone();

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

fn handle_user_account_removed(
    msg: &teamtalk::Message,
    bot: &Bot,
    db: &Database,
    config: &TTWorkerConfig,
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
    let admins_clone = config.admin_ids.clone();
    let pending_dels = pending_deletions.clone();
    let u_name_cl = u_name.clone();
    let lang_clone = config.admin_lang.clone();

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
