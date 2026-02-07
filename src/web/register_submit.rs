use super::WebState;
use super::templates::{RegisterForm, RegisterTemplate};
use crate::domain::{Nickname, Password, Username};
use crate::i18n::t;
use crate::types::{LanguageCode, RegistrationSource, TTWorkerCommand};
use axum::extract::{ConnectInfo, Form, State};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, warn};

pub(super) async fn register_post(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Form(form): Form<RegisterForm>,
) -> impl axum::response::IntoResponse {
    let ip = super::request_ctx::resolve_client_ip(&state, &headers, addr.ip());
    let (lang, language_forced) = super::request_ctx::resolve_web_lang(&state.config, &headers);
    if state
        .db
        .is_ip_registered(&ip.to_string())
        .await
        .unwrap_or(false)
    {
        return error_template(&state, &lang, language_forced, &form, "web-err-ip-limit");
    }

    let (username, password, nickname) =
        match parse_registration_form(&state, &lang, language_forced, &form) {
            Ok(parsed) => parsed,
            Err(tpl) => return *tpl,
        };

    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = TTWorkerCommand::CreateAccount {
        username: username.clone(),
        password: password.clone(),
        nickname: nickname.clone(),
        account_type: crate::types::TTAccountType::Default,
        source: RegistrationSource::Web(ip),
        source_info: None,
        resp: tx,
    };

    if let Err(e) = state.tx_tt.send(cmd) {
        error!(error = %e, ip = %ip, "Failed to enqueue TeamTalk create command");
        return error_template(&state, &lang, language_forced, &form, "web-err-timeout");
    }

    match rx.await {
        Ok(Ok(true)) => {
            super::register_success::build_success_template(
                super::register_success::WebSuccessParams {
                    state: &state,
                    lang: &lang,
                    language_forced,
                    ip,
                    form: &form,
                    username: &username,
                    password: &password,
                    nickname: &nickname,
                },
            )
            .await
        }
        Ok(Ok(false)) => {
            warn!("TeamTalk create account returned false");
            error_template(
                &state,
                &lang,
                language_forced,
                &form,
                "web-err-username-taken",
            )
        }
        _ => {
            warn!("TeamTalk create account response failed");
            error_template(&state, &lang, language_forced, &form, "web-err-timeout")
        }
    }
}

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

fn parse_registration_form(
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
