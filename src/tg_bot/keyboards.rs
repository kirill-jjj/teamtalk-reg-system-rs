use crate::i18n::available_languages;
use crate::types::TelegramId;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

/// Keyboard for language selection.
pub fn language_keyboard() -> InlineKeyboardMarkup {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();

    for (code, native_name) in available_languages().iter() {
        current_row.push(InlineKeyboardButton::callback(
            native_name,
            format!("lang_{code}"),
        ));
        if current_row.len() >= 2 {
            rows.push(current_row);
            current_row = Vec::new();
        }
    }

    if !current_row.is_empty() {
        rows.push(current_row);
    }

    InlineKeyboardMarkup::new(rows)
}

/// Keyboard for choosing default or custom nickname.
pub fn nickname_choice_keyboard(yes_text: &str, no_text: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(yes_text, "nick_custom"),
        InlineKeyboardButton::callback(no_text, "nick_default"),
    ]])
}

/// Keyboard for admin approval of a pending registration.
pub fn admin_approval_keyboard(
    yes_text: &str,
    no_text: &str,
    request_id: &str,
) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(yes_text, format!("approve_{request_id}")),
        InlineKeyboardButton::callback(no_text, format!("reject_{request_id}")),
    ]])
}

/// Keyboard for admin panel actions.
pub fn admin_panel_keyboard(
    btn_delete: &str,
    btn_banlist: &str,
    btn_tt_list: &str,
) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(btn_delete, "admin_del")],
        vec![InlineKeyboardButton::callback(
            btn_banlist,
            "admin_banlist_view",
        )],
        vec![InlineKeyboardButton::callback(btn_tt_list, "admin_tt_list")],
    ])
}

/// Keyboard for selecting a registered user.
pub fn admin_user_list_keyboard(
    users: Vec<(TelegramId, String)>,
    nav_row: Option<Vec<InlineKeyboardButton>>,
) -> InlineKeyboardMarkup {
    let mut buttons = vec![];
    for (tg_id, tt_user) in users {
        buttons.push(vec![InlineKeyboardButton::callback(
            format!("TG ID: {tg_id} - TT User: {tt_user}"),
            format!("admin_del_confirm_{}", tg_id.as_i64()),
        )]);
    }
    if let Some(row) = nav_row {
        buttons.push(row);
    }
    InlineKeyboardMarkup::new(buttons)
}

/// Keyboard for banlist entries.
pub fn admin_banlist_keyboard(
    banned_users: Vec<(TelegramId, String)>,
    unban_text: &str,
    manual_text: &str,
    nav_row: Option<Vec<InlineKeyboardButton>>,
) -> InlineKeyboardMarkup {
    let mut buttons = vec![];
    for (tg_id, _reason) in banned_users {
        buttons.push(vec![InlineKeyboardButton::callback(
            format!("{unban_text} ({tg_id})"),
            format!("admin_unban_{}", tg_id.as_i64()),
        )]);
    }
    buttons.push(vec![InlineKeyboardButton::callback(
        manual_text,
        "admin_ban_manual",
    )]);
    if let Some(row) = nav_row {
        buttons.push(row);
    }
    InlineKeyboardMarkup::new(buttons)
}

/// Keyboard for `TeamTalk` accounts list.
pub fn admin_tt_accounts_keyboard(
    accounts: Vec<String>,
    delete_text: &str,
    nav_row: Option<Vec<InlineKeyboardButton>>,
) -> InlineKeyboardMarkup {
    let mut buttons = vec![];
    for acc in accounts {
        buttons.push(vec![InlineKeyboardButton::callback(
            format!("{delete_text} ({acc})"),
            format!("admin_tt_del_prompt_{acc}"),
        )]);
    }
    if let Some(row) = nav_row {
        buttons.push(row);
    }
    InlineKeyboardMarkup::new(buttons)
}

pub fn pagination_row(
    prev_text: &str,
    next_text: &str,
    prev_cb: Option<String>,
    next_cb: Option<String>,
) -> Option<Vec<InlineKeyboardButton>> {
    let mut row = Vec::new();
    if let Some(cb) = prev_cb {
        row.push(InlineKeyboardButton::callback(prev_text.to_string(), cb));
    }
    if let Some(cb) = next_cb {
        row.push(InlineKeyboardButton::callback(next_text.to_string(), cb));
    }
    if row.is_empty() { None } else { Some(row) }
}

/// Keyboard for confirmation actions.
pub fn confirm_keyboard(
    confirm_text: &str,
    cancel_text: &str,
    payload: &str,
) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(confirm_text, format!("confirm_{payload}")),
        InlineKeyboardButton::callback(cancel_text, "cancel_action"),
    ]])
}

/// Keyboard for account type selection.
pub fn admin_account_type_keyboard(admin_text: &str, user_text: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(admin_text, "acct_admin")],
        vec![InlineKeyboardButton::callback(user_text, "acct_user")],
    ])
}
