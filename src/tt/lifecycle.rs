use crate::tt::commands::PendingCommand;
use crate::tt::pending_lists::{self, PendingListRequest};
use std::collections::HashMap;
use teamtalk::Client;
use teamtalk::client::ReconnectHandler;
use teamtalk::types::{UserGender, UserPresence, UserStatus};
use tracing::{info, warn};

pub(super) struct LoginConfig<'a> {
    pub(super) nickname: &'a str,
    pub(super) username: &'a str,
    pub(super) password: &'a str,
    pub(super) client_name: &'a str,
}

pub(super) struct StatusConfig<'a> {
    pub(super) gender: &'a str,
    pub(super) status_text: &'a str,
}

pub(super) fn handle_connect_success(
    client: &Client,
    reconnect: &mut ReconnectHandler,
    config: &LoginConfig<'_>,
) {
    info!("Connected. Logging in");
    reconnect.mark_connected();
    client.login(
        config.nickname,
        config.username,
        config.password,
        config.client_name,
    );
}

pub(super) fn handle_connection_lost(
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

pub(super) fn handle_logged_in(
    client: &Client,
    is_logged_in: &mut bool,
    status: &StatusConfig<'_>,
) {
    info!("Logged in as bot");
    *is_logged_in = true;

    let gender = match status.gender.to_lowercase().as_str() {
        "male" => UserGender::Male,
        "female" => UserGender::Female,
        _ => UserGender::Neutral,
    };

    let status_mode = UserStatus {
        gender,
        presence: UserPresence::Available,
        ..Default::default()
    };

    client.set_status(status_mode, status.status_text);
    client.subscribe(client.my_id(), teamtalk::types::Subscriptions::all());
}
