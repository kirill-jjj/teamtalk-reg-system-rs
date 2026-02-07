use super::WebState;
use super::templates::RegisterForm;
use crate::types::{RegistrationSource, TTWorkerCommand};
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
        return super::register_form::error_template(
            &state,
            &lang,
            language_forced,
            &form,
            "web-err-ip-limit",
        );
    }

    let (username, password, nickname) = match super::register_form::parse_registration_form(
        &state,
        &lang,
        language_forced,
        &form,
    ) {
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
        return super::register_form::error_template(
            &state,
            &lang,
            language_forced,
            &form,
            "web-err-timeout",
        );
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
            super::register_form::error_template(
                &state,
                &lang,
                language_forced,
                &form,
                "web-err-username-taken",
            )
        }
        _ => {
            warn!("TeamTalk create account response failed");
            super::register_form::error_template(
                &state,
                &lang,
                language_forced,
                &form,
                "web-err-timeout",
            )
        }
    }
}
