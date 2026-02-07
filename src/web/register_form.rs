use super::WebState;
use super::templates::{RegisterForm, RegisterTemplate};
use crate::domain::{Nickname, Password, Username};
use crate::i18n::t;
use crate::types::LanguageCode;

pub(super) fn base_template(
    state: &WebState,
    lang: &LanguageCode,
    language_forced: bool,
) -> RegisterTemplate {
    RegisterTemplate::new(
        state.config.teamtalk.server_name.as_str(),
        lang,
        state.available_languages.as_ref().clone(),
        language_forced,
        state.config.database.generated_file_ttl_seconds,
    )
}

pub(super) fn error_template(
    state: &WebState,
    lang: &LanguageCode,
    language_forced: bool,
    form: &RegisterForm,
    message_key: &str,
) -> RegisterTemplate {
    let mut tpl = base_template(state, lang, language_forced);
    tpl.message = Some(t(lang.as_str(), message_key));
    tpl.message_class = Some("error".to_string());
    tpl.message_class_safe = "error".to_string();
    tpl.username_val.clone_from(&form.username);
    tpl.nickname_val.clone_from(&form.nickname);
    tpl
}

pub(super) fn parse_registration_form(
    state: &WebState,
    lang: &LanguageCode,
    language_forced: bool,
    form: &RegisterForm,
) -> Result<(Username, Password, Nickname), Box<RegisterTemplate>> {
    let Some(username) = Username::parse(&form.username) else {
        return Err(Box::new(error_template(
            state,
            lang,
            language_forced,
            form,
            "web-err-username-invalid",
        )));
    };
    let Some(password) = Password::parse(&form.password) else {
        return Err(Box::new(error_template(
            state,
            lang,
            language_forced,
            form,
            "web-err-password-invalid",
        )));
    };
    let nickname = if form.nickname.is_empty() {
        let Some(n) = Nickname::parse(username.as_str()) else {
            return Err(Box::new(error_template(
                state,
                lang,
                language_forced,
                form,
                "web-err-nickname-invalid",
            )));
        };
        n
    } else {
        let Some(n) = Nickname::parse(&form.nickname) else {
            return Err(Box::new(error_template(
                state,
                lang,
                language_forced,
                form,
                "web-err-nickname-invalid",
            )));
        };
        n
    };
    Ok((username, password, nickname))
}
