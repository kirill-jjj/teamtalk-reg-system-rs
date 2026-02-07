use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, warn};

pub(super) enum PendingListKind {
    AllUsers {
        resp: oneshot::Sender<Vec<String>>,
    },
    Exists {
        username: crate::domain::Username,
        resp: oneshot::Sender<bool>,
    },
}

pub(super) struct PendingListRequest {
    kind: PendingListKind,
    accumulated: Vec<String>,
    completed_at: Option<Instant>,
    mismatch_logged: bool,
}

impl PendingListRequest {
    pub(super) const fn all_users(resp: oneshot::Sender<Vec<String>>) -> Self {
        Self {
            kind: PendingListKind::AllUsers { resp },
            accumulated: Vec::new(),
            completed_at: None,
            mismatch_logged: false,
        }
    }

    pub(super) const fn exists(
        username: crate::domain::Username,
        resp: oneshot::Sender<bool>,
    ) -> Self {
        Self {
            kind: PendingListKind::Exists { username, resp },
            accumulated: Vec::new(),
            completed_at: None,
            mismatch_logged: false,
        }
    }
}

pub(super) fn mark_command_completed(
    cmd_id: i32,
    pending_lists: &mut HashMap<i32, PendingListRequest>,
) {
    if let Some(req) = pending_lists.get_mut(&cmd_id)
        && req.completed_at.is_none()
    {
        req.completed_at = Some(Instant::now());
        debug!(cmd_id, "List command completed; waiting for account events");
    }
}

pub(super) fn fail_command(cmd_id: i32, pending_lists: &mut HashMap<i32, PendingListRequest>) {
    if let Some(req) = pending_lists.remove(&cmd_id) {
        respond(req, false);
    }
}

pub(super) fn handle_user_account_event(
    msg: &teamtalk::Message,
    pending_lists: &mut HashMap<i32, PendingListRequest>,
) {
    let cmd_id = msg.source();
    let Some(acc) = msg.account() else {
        return;
    };

    if let Some(req) = pending_lists.get_mut(&cmd_id) {
        debug!(cmd_id, username = %acc.username, "Received user account");
        req.accumulated.push(acc.username);
        if req.completed_at.is_some() {
            req.completed_at = Some(Instant::now());
        }
        return;
    }

    if pending_lists.len() == 1 {
        let Some((_pending_id, req)) = pending_lists.iter_mut().next() else {
            return;
        };
        if !req.mismatch_logged {
            req.mismatch_logged = true;
        }
        req.accumulated.push(acc.username);
        if req.completed_at.is_some() {
            req.completed_at = Some(Instant::now());
        }
        return;
    }

    warn!(cmd_id, "Received user account without pending list request");
}

pub(super) fn flush_completed(pending_lists: &mut HashMap<i32, PendingListRequest>) {
    const LIST_GRACE: Duration = Duration::from_millis(500);
    let now = Instant::now();
    let mut ready = Vec::new();

    for (&cmd_id, req) in pending_lists.iter() {
        if let Some(completed_at) = req.completed_at
            && now.duration_since(completed_at) >= LIST_GRACE
        {
            ready.push(cmd_id);
        }
    }

    for cmd_id in ready {
        if let Some(req) = pending_lists.remove(&cmd_id) {
            debug!(
                cmd_id,
                count = req.accumulated.len(),
                "Finalizing account list"
            );
            respond(req, true);
        }
    }
}

fn respond(req: PendingListRequest, success: bool) {
    match req.kind {
        PendingListKind::AllUsers { resp } => {
            let _ = resp.send(if success { req.accumulated } else { vec![] });
        }
        PendingListKind::Exists { username, resp } => {
            let exists = success && req.accumulated.iter().any(|name| name == username.as_str());
            let _ = resp.send(exists);
        }
    }
}
