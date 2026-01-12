use super::WebState;
use super::templates::{RegisterForm, RegisterTemplate};
use crate::domain::{Nickname, Password, Username};
use crate::i18n::t;
use crate::services::registration;
use crate::types::{DownloadTokenType, LanguageCode, RegistrationSource, TTWorkerCommand};
use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Path, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use tracing::{error, warn};
use uuid::Uuid;

pub async fn register_page(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (lang, language_forced) = resolve_web_lang(&state.config, &headers);
    let available_languages = state.available_languages.as_ref().clone();
    RegisterTemplate::new(
        state.config.server_name.clone(),
        &lang,
        available_languages,
        language_forced,
        state.config.generated_file_ttl_seconds,
    )
}

pub async fn register_post(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    let ip = addr.ip();
    let (lang, language_forced) = resolve_web_lang(&state.config, &headers);
    if state
        .db
        .is_ip_registered(&ip.to_string())
        .await
        .unwrap_or(false)
    {
        let mut tpl = RegisterTemplate::new(
            state.config.server_name.clone(),
            &lang,
            state.available_languages.as_ref().clone(),
            language_forced,
            state.config.generated_file_ttl_seconds,
        );
        tpl.message = Some(t(lang.as_str(), "web-err-ip-limit"));
        tpl.message_class = Some("error".to_string());
        tpl.message_class_safe = "error".to_string();
        tpl.username_val = form.username.clone();
        tpl.nickname_val = form.nickname.clone();
        return tpl;
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    let Some(username) = Username::parse(&form.username) else {
        let mut tpl = RegisterTemplate::new(
            state.config.server_name.clone(),
            &lang,
            state.available_languages.as_ref().clone(),
            language_forced,
            state.config.generated_file_ttl_seconds,
        );
        tpl.message = Some(t(lang.as_str(), "web-err-username-taken"));
        tpl.message_class = Some("error".to_string());
        tpl.message_class_safe = "error".to_string();
        tpl.username_val = form.username;
        tpl.nickname_val = form.nickname;
        return tpl;
    };
    let Some(password) = Password::parse(&form.password) else {
        let mut tpl = RegisterTemplate::new(
            state.config.server_name.clone(),
            &lang,
            state.available_languages.as_ref().clone(),
            language_forced,
            state.config.generated_file_ttl_seconds,
        );
        tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
        tpl.message_class = Some("error".to_string());
        tpl.message_class_safe = "error".to_string();
        tpl.username_val = form.username;
        tpl.nickname_val = form.nickname;
        return tpl;
    };
    let nickname = if form.nickname.is_empty() {
        match Nickname::parse(username.as_str()) {
            Some(n) => n,
            None => {
                let mut tpl = RegisterTemplate::new(
                    state.config.server_name.clone(),
                    &lang,
                    state.available_languages.as_ref().clone(),
                    language_forced,
                    state.config.generated_file_ttl_seconds,
                );
                tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                tpl.message_class = Some("error".to_string());
                tpl.message_class_safe = "error".to_string();
                tpl.username_val = form.username;
                tpl.nickname_val = form.nickname;
                return tpl;
            }
        }
    } else {
        match Nickname::parse(&form.nickname) {
            Some(n) => n,
            None => {
                let mut tpl = RegisterTemplate::new(
                    state.config.server_name.clone(),
                    &lang,
                    state.available_languages.as_ref().clone(),
                    language_forced,
                    state.config.generated_file_ttl_seconds,
                );
                tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                tpl.message_class = Some("error".to_string());
                tpl.message_class_safe = "error".to_string();
                tpl.username_val = form.username;
                tpl.nickname_val = form.nickname;
                return tpl;
            }
        }
    };

    let cmd = TTWorkerCommand::CreateAccount {
        username: username.clone(),
        password: password.clone(),
        nickname: nickname.clone(),
        account_type: crate::types::TTAccountType::Default,
        source: RegistrationSource::Web(ip),
        resp: tx,
    };

    if state.tx_tt.send(cmd).is_err() {
        let mut tpl = RegisterTemplate::new(
            state.config.server_name.clone(),
            &lang,
            state.available_languages.as_ref().clone(),
            language_forced,
            state.config.generated_file_ttl_seconds,
        );
        tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
        tpl.message_class = Some("error".to_string());
        tpl.message_class_safe = "error".to_string();
        tpl.username_val = form.username;
        tpl.nickname_val = form.nickname;
        return tpl;
    }

    match rx.await {
        Ok(Ok(true)) => {
            if let Err(e) = state
                .db
                .add_registered_ip(&ip.to_string(), Some(&form.username))
                .await
            {
                warn!(error = %e, ip = %ip, "Failed to store registered IP");
            }

            let temp_dir = match std::env::current_dir() {
                Ok(dir) => dir.join("temp_files"),
                Err(e) => {
                    error!(error = %e, "Failed to resolve temp dir");
                    let mut tpl = RegisterTemplate::new(
                        state.config.server_name.clone(),
                        &lang,
                        state.available_languages.as_ref().clone(),
                        language_forced,
                        state.config.generated_file_ttl_seconds,
                    );
                    tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                    tpl.message_class = Some("error".to_string());
                    tpl.message_class_safe = "error".to_string();
                    tpl.username_val = form.username;
                    tpl.nickname_val = form.nickname;
                    return tpl;
                }
            };
            let unique_id = Uuid::new_v4().to_string();
            let assets = registration::build_assets(
                &state.config,
                username.as_str(),
                password.as_str(),
                nickname.as_str(),
            );
            let safe_tt_path = temp_dir.join(format!("{}_{}", unique_id, assets.tt_filename));
            let tt_content = assets.tt_content.clone();
            if let Err(e) = tokio::fs::write(&safe_tt_path, &tt_content).await {
                error!(error = %e, path = ?safe_tt_path, "Failed to write TT file");
                let mut tpl = RegisterTemplate::new(
                    state.config.server_name.clone(),
                    &lang,
                    state.available_languages.as_ref().clone(),
                    language_forced,
                    state.config.generated_file_ttl_seconds,
                );
                tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                tpl.message_class = Some("error".to_string());
                tpl.message_class_safe = "error".to_string();
                tpl.username_val = form.username;
                tpl.nickname_val = form.nickname;
                return tpl;
            }

            let token_tt = Uuid::new_v4().to_string();
            let expires = Utc::now().naive_utc()
                + Duration::seconds(state.config.generated_file_ttl_seconds as i64);

            let tt_path_name = match safe_tt_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => {
                    error!(path = ?safe_tt_path, "Invalid TT file name");
                    let mut tpl = RegisterTemplate::new(
                        state.config.server_name.clone(),
                        &lang,
                        state.available_languages.as_ref().clone(),
                        language_forced,
                        state.config.generated_file_ttl_seconds,
                    );
                    tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                    tpl.message_class = Some("error".to_string());
                    tpl.message_class_safe = "error".to_string();
                    tpl.username_val = form.username;
                    tpl.nickname_val = form.nickname;
                    return tpl;
                }
            };
            if let Err(e) = state
                .db
                .add_download_token(
                    &token_tt,
                    tt_path_name,
                    &assets.tt_filename,
                    DownloadTokenType::TtConfig,
                    expires,
                )
                .await
            {
                warn!(error = %e, "Failed to persist download token");
            }

            let mut zip_token: Option<String> = None;
            let zip_name = format!("{}_TeamTalk.zip", username.as_str());
            let safe_zip_path = temp_dir.join(format!("{}_{}", unique_id, zip_name));
            if registration::try_create_client_zip_async(&state.config, &safe_zip_path, &assets)
                .await
            {
                let z_tok = Uuid::new_v4().to_string();
                let zip_path_name = match safe_zip_path.file_name().and_then(|n| n.to_str()) {
                    Some(name) => name,
                    None => {
                        error!(path = ?safe_zip_path, "Invalid ZIP file name");
                        let mut tpl = RegisterTemplate::new(
                            state.config.server_name.clone(),
                            &lang,
                            state.available_languages.as_ref().clone(),
                            language_forced,
                            state.config.generated_file_ttl_seconds,
                        );
                        tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
                        tpl.message_class = Some("error".to_string());
                        tpl.message_class_safe = "error".to_string();
                        tpl.username_val = form.username;
                        tpl.nickname_val = form.nickname;
                        return tpl;
                    }
                };
                if let Err(e) = state
                    .db
                    .add_download_token(
                        &z_tok,
                        zip_path_name,
                        &zip_name,
                        DownloadTokenType::ClientZip,
                        expires,
                    )
                    .await
                {
                    warn!(error = %e, "Failed to persist ZIP token");
                }
                zip_token = Some(z_tok);
            }

            let mut tpl = RegisterTemplate::new(
                state.config.server_name.clone(),
                &lang,
                state.available_languages.as_ref().clone(),
                language_forced,
                state.config.generated_file_ttl_seconds,
            );
            tpl.registration_complete = true;
            tpl.message = Some(t(lang.as_str(), "web-success-title"));
            tpl.message_class = Some("success".to_string());
            tpl.message_class_safe = "success".to_string();
            tpl.download_tt_token = Some(token_tt);
            tpl.tt_link = Some(assets.tt_link);
            tpl.actual_tt_filename_for_user = Some(assets.tt_filename);
            if let Some(zt) = zip_token {
                tpl.download_client_zip_token = Some(zt);
                tpl.actual_client_zip_filename_for_user =
                    Some(format!("{}_TeamTalk.zip", username.as_str()));
            }
            tpl
        }
        Ok(Ok(false)) => {
            let mut tpl = RegisterTemplate::new(
                state.config.server_name.clone(),
                &lang,
                state.available_languages.as_ref().clone(),
                language_forced,
                state.config.generated_file_ttl_seconds,
            );
            tpl.message = Some(t(lang.as_str(), "web-err-username-taken"));
            tpl.message_class = Some("error".to_string());
            tpl.message_class_safe = "error".to_string();
            tpl.username_val = form.username;
            tpl.nickname_val = form.nickname;
            tpl
        }
        _ => {
            let mut tpl = RegisterTemplate::new(
                state.config.server_name.clone(),
                &lang,
                state.available_languages.as_ref().clone(),
                language_forced,
                state.config.generated_file_ttl_seconds,
            );
            tpl.message = Some(t(lang.as_str(), "web-err-timeout"));
            tpl.message_class = Some("error".to_string());
            tpl.message_class_safe = "error".to_string();
            tpl.username_val = form.username;
            tpl.nickname_val = form.nickname;
            tpl
        }
    }
}

pub async fn set_language_and_reload(
    State(_state): State<Arc<WebState>>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let lang = form
        .get("lang_code")
        .map(|v| LanguageCode::parse_or_default(v))
        .unwrap_or_default();
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("user_web_lang={}; Path=/", lang.as_str())) {
        headers.insert(axum::http::header::SET_COOKIE, value);
    }
    (headers, Redirect::to("/register"))
}

pub async fn download_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    if let Ok(Some(tok_data)) = state.db.get_download_token(&token).await {
        let temp_dir = match std::env::current_dir() {
            Ok(dir) => dir.join("temp_files"),
            Err(e) => {
                error!(error = %e, "Failed to resolve temp dir");
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    t("en", "web-err-invalid-link"),
                )
                    .into_response();
            }
        };
        let path = temp_dir.join(&tok_data.filepath_on_server);

        if path.exists() {
            if let Err(e) = state.db.mark_token_used(&token).await {
                warn!(error = %e, "Failed to mark token used");
            }

            let file = match File::open(&path).await {
                Ok(f) => f,
                Err(_) => {
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-file-not-found"),
                    )
                        .into_response();
                }
            };

            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);

            let mime = mime_guess::from_path(&path).first_or_octet_stream();

            let response = axum::response::Response::builder()
                .header("Content-Type", mime.as_ref())
                .header(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", tok_data.original_filename),
                )
                .body(body);
            return match response {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Failed to build response");
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-invalid-link"),
                    )
                        .into_response()
                }
            };
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        t("en", "web-err-invalid-link"),
    )
        .into_response()
}

pub async fn download_tt_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::TtConfig).await
}

pub async fn download_client_zip_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::ClientZip).await
}

fn resolve_web_lang(
    config: &crate::config::AppConfig,
    headers: &HeaderMap,
) -> (LanguageCode, bool) {
    if let Some(forced) = &config.force_user_lang {
        let translated = t(forced.as_str(), "web-label-username");
        if translated != "web-label-username" || forced.as_str() == "en" {
            return (forced.clone(), true);
        }
    }

    if let Some(cookie) = headers.get(axum::http::header::COOKIE)
        && let Ok(cookie_str) = cookie.to_str()
    {
        for part in cookie_str.split(';') {
            let trimmed = part.trim();
            if let Some(value) = trimmed.strip_prefix("user_web_lang=") {
                return (LanguageCode::parse_or_default(value), false);
            }
        }
    }

    (LanguageCode::default(), false)
}

async fn download_by_type(
    state: Arc<WebState>,
    token: String,
    token_type: DownloadTokenType,
) -> Response {
    if let Ok(Some(tok_data)) = state.db.get_download_token(&token).await {
        let stored_type = match DownloadTokenType::try_from(tok_data.token_type.as_str()) {
            Ok(t) => t,
            Err(_) => {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    t("en", "web-err-invalid-link"),
                )
                    .into_response();
            }
        };
        if stored_type != token_type {
            return (
                axum::http::StatusCode::NOT_FOUND,
                t("en", "web-err-invalid-link"),
            )
                .into_response();
        }
        let temp_dir = match std::env::current_dir() {
            Ok(dir) => dir.join("temp_files"),
            Err(e) => {
                error!(error = %e, "Failed to resolve temp dir");
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    t("en", "web-err-invalid-link"),
                )
                    .into_response();
            }
        };
        let path = temp_dir.join(&tok_data.filepath_on_server);

        if path.exists() {
            if let Err(e) = state.db.mark_token_used(&token).await {
                warn!(error = %e, "Failed to mark token used");
            }

            let file = match File::open(&path).await {
                Ok(f) => f,
                Err(_) => {
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-file-not-found"),
                    )
                        .into_response();
                }
            };

            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);
            let mime = mime_guess::from_path(&path).first_or_octet_stream();

            let response = axum::response::Response::builder()
                .header("Content-Type", mime.as_ref())
                .header(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", tok_data.original_filename),
                )
                .body(body);
            return match response {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Failed to build response");
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-invalid-link"),
                    )
                        .into_response()
                }
            };
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        t("en", "web-err-invalid-link"),
    )
        .into_response()
}
