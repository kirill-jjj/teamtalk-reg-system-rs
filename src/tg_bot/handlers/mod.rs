use crate::domain::{Nickname, Password, Username};
use crate::types::LanguageCode;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

mod admin;
mod registration;

pub use admin::{admin_callback, admin_manual_ban_input, admin_panel, exit_bot, generate_invite};
pub use registration::{
    receive_account_type, receive_language, receive_nickname, receive_nickname_choice,
    receive_password, receive_username, start,
};

/// Supported bot commands.
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    /// Start the registration flow.
    Start,
    /// Open admin panel.
    AdminPanel,
    /// Generate a one-time invite link.
    Generate,
    /// Gracefully stop the bot.
    Exit,
    /// Show help.
    Help,
}

/// Dialogue states for the Telegram registration flow.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub enum State {
    #[default]
    Start,
    ChoosingLanguage,
    AwaitingUsername {
        lang: LanguageCode,
    },
    AwaitingPassword {
        lang: LanguageCode,
        username: Username,
    },
    AwaitingNicknameChoice {
        lang: LanguageCode,
        username: Username,
        password: Password,
    },
    AwaitingNickname {
        lang: LanguageCode,
        username: Username,
        password: Password,
    },
    AwaitingAccountType {
        lang: LanguageCode,
        username: Username,
        password: Password,
        nickname: Nickname,
    },
    AdminPanel,
    AwaitingManualBanInput,
}

/// Dialogue type used by handlers.
pub type MyDialogue = Dialogue<State, InMemStorage<State>>;
/// Result type returned by handlers.
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
