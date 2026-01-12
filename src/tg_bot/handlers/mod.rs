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

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    Start,
    AdminPanel,
    Generate,
    Exit,
    Help,
}

#[derive(Clone, Default, Debug, PartialEq)]
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

pub type MyDialogue = Dialogue<State, InMemStorage<State>>;
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
