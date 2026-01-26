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

/// Render the registration page.
pub(super) async fn register_page(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (lang, language_forced) = resolve_web_lang(&state.config, &headers);
    let available_languages = state.available_languages.as_ref().clone();
    RegisterTemplate::new(
        state.config.teamtalk.server_name.as_str(),
        &lang,
        available_languages,
        language_forced,
        state.config.database.generated_file_ttl_seconds,
    )
}

/// Handle registration form submission.
pub(super) async fn register_post(
    State(state): State<Arc<WebState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> impl IntoResponse {
    let ip = resolve_client_ip(&state, &headers, addr.ip());
    let (lang, language_forced) = resolve_web_lang(&state.config, &headers);
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
            build_success_template(WebSuccessParams {
                state: &state,
                lang: &lang,
                language_forced,
                ip,
                form: &form,
                username: &username,
                password: &password,
                nickname: &nickname,
            })
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

fn base_template(state: &WebState, lang: &LanguageCode, language_forced: bool) -> RegisterTemplate {
    RegisterTemplate::new(
        state.config.teamtalk.server_name.as_str(),
        lang,
        state.available_languages.as_ref().clone(),
        language_forced,
        state.config.database.generated_file_ttl_seconds,
    )
}

fn error_template(
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

struct WebSuccessParams<'a> {
    state: &'a WebState,
    lang: &'a LanguageCode,
    language_forced: bool,
    ip: std::net::IpAddr,
    form: &'a RegisterForm,
    username: &'a Username,
    password: &'a Password,
    nickname: &'a Nickname,
}

struct WebBuildContext<'a> {
    state: &'a WebState,
    lang: &'a LanguageCode,
    language_forced: bool,
    form: &'a RegisterForm,
}

async fn build_success_template(params: WebSuccessParams<'_>) -> RegisterTemplate {
    let WebSuccessParams {
        state,
        lang,
        language_forced,
        ip,
        form,
        username,
        password,
        nickname,
    } = params;
    let ctx = WebBuildContext {
        state,
        lang,
        language_forced,
        form,
    };
    if let Err(e) = state
        .db
        .add_registered_ip(&ip.to_string(), Some(username.as_str()))
        .await
    {
        warn!(error = %e, ip = %ip, "Failed to store registered IP");
    }

    let temp_dir = match temp_dir_or_error(&ctx) {
        Ok(dir) => dir,
        Err(tpl) => return *tpl,
    };

    let unique_id = Uuid::new_v4().to_string();
    let assets = registration::build_assets(
        &state.config,
        username.as_str(),
        password.as_str(),
        nickname.as_str(),
    );
    let safe_tt_path = match write_tt_file(&ctx, &temp_dir, &unique_id, &assets).await {
        Ok(path) => path,
        Err(tpl) => return tpl,
    };
    let expires = build_token_expiry(state);
    let token_tt = persist_tt_token(state, &safe_tt_path, &assets, expires).await;

    let zip_token =
        match try_create_zip_token(&ctx, &temp_dir, &unique_id, username, &assets, expires).await {
            Ok(token) => token,
            Err(tpl) => return tpl,
        };

    let mut tpl = base_template(state, lang, language_forced);
    tpl.registration_complete = true;
    tpl.message = Some(t(lang.as_str(), "web-success-title"));
    tpl.message_class = Some("success".to_string());
    tpl.message_class_safe = "success".to_string();
    tpl.download_tt_token = Some(token_tt);
    tpl.tt_link = Some(assets.link);
    tpl.actual_tt_filename_for_user = Some(assets.filename);
    if let Some(zt) = zip_token {
        tpl.download_client_zip_token = Some(zt);
        tpl.actual_client_zip_filename_for_user = Some(format!("{username}_TeamTalk.zip"));
    }
    tpl
}

fn temp_dir_or_error(
    ctx: &WebBuildContext<'_>,
) -> Result<std::path::PathBuf, Box<RegisterTemplate>> {
    match std::env::current_dir() {
        Ok(dir) => Ok(dir.join("temp_files")),
        Err(e) => {
            error!(error = %e, "Failed to resolve temp dir");
            Err(Box::new(error_template(
                ctx.state,
                ctx.lang,
                ctx.language_forced,
                ctx.form,
                "web-err-timeout",
            )))
        }
    }
}

async fn write_tt_file(
    ctx: &WebBuildContext<'_>,
    temp_dir: &std::path::Path,
    unique_id: &str,
    assets: &registration::RegistrationAssets,
) -> Result<std::path::PathBuf, RegisterTemplate> {
    let safe_tt_path = temp_dir.join(format!("{unique_id}_{}", assets.filename));
    let tt_content = assets.content.clone();
    if let Err(e) = tokio::fs::write(&safe_tt_path, &tt_content).await {
        error!(error = %e, path = ?safe_tt_path, "Failed to write TT file");
        return Err(error_template(
            ctx.state,
            ctx.lang,
            ctx.language_forced,
            ctx.form,
            "web-err-timeout",
        ));
    }
    Ok(safe_tt_path)
}

fn build_token_expiry(state: &WebState) -> chrono::NaiveDateTime {
    let ttl_seconds = i64::try_from(state.config.database.generated_file_ttl_seconds)
        .unwrap_or_else(|_| {
            warn!(
                ttl = state.config.database.generated_file_ttl_seconds,
                "generated_file_ttl_seconds too large for i64, clamping"
            );
            i64::MAX
        });
    Utc::now().naive_utc() + Duration::seconds(ttl_seconds)
}

async fn persist_tt_token(
    state: &WebState,
    safe_tt_path: &std::path::Path,
    assets: &registration::RegistrationAssets,
    expires: chrono::NaiveDateTime,
) -> String {
    let token_tt = Uuid::new_v4().to_string();
    let Some(tt_path_name) = safe_tt_path.file_name().and_then(|n| n.to_str()) else {
        error!(path = ?safe_tt_path, "Invalid TT file name");
        return token_tt;
    };
    if let Err(e) = state
        .db
        .add_download_token(
            &token_tt,
            tt_path_name,
            &assets.filename,
            DownloadTokenType::TtConfig,
            expires,
        )
        .await
    {
        warn!(error = %e, "Failed to persist download token");
    }
    token_tt
}

async fn try_create_zip_token(
    ctx: &WebBuildContext<'_>,
    temp_dir: &std::path::Path,
    unique_id: &str,
    username: &Username,
    assets: &registration::RegistrationAssets,
    expires: chrono::NaiveDateTime,
) -> Result<Option<String>, RegisterTemplate> {
    let zip_name = format!("{username}_TeamTalk.zip");
    let safe_zip_path = temp_dir.join(format!("{unique_id}_{zip_name}"));
    if registration::try_create_client_zip_async(&ctx.state.config, &safe_zip_path, assets).await {
        let z_tok = Uuid::new_v4().to_string();
        let Some(zip_path_name) = safe_zip_path.file_name().and_then(|n| n.to_str()) else {
            error!(path = ?safe_zip_path, "Invalid ZIP file name");
            return Err(error_template(
                ctx.state,
                ctx.lang,
                ctx.language_forced,
                ctx.form,
                "web-err-timeout",
            ));
        };
        if let Err(e) = ctx
            .state
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
        return Ok(Some(z_tok));
    }
    Ok(None)
}

/// Persist selected language and redirect back to the form.
pub(super) async fn set_language_and_reload(
    State(_state): State<Arc<WebState>>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let lang = form
        .get("lang_code")
        .map(|v| LanguageCode::parse_or_default(v))
        .unwrap_or_default();
    let mut headers = HeaderMap::new();
    match HeaderValue::from_str(&format!("user_web_lang={}; Path=/", lang.as_str())) {
        Ok(value) => {
            headers.insert(axum::http::header::SET_COOKIE, value);
        }
        Err(e) => {
            warn!(error = %e, "Failed to build user_web_lang cookie header");
        }
    }
    (headers, Redirect::to("/register"))
}

/// Download handler for generic tokens.
pub(super) async fn download_handler(
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
                Err(e) => {
                    error!(error = %e, path = %path.display(), "Failed to open file for download");
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

/// Download handler for `TeamTalk` `.tt` config files.
pub(super) async fn download_tt_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::TtConfig).await
}

/// Download handler for client ZIP.
pub(super) async fn download_client_zip_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::ClientZip).await
}

fn resolve_web_lang(
    config: &crate::config::AppConfig,
    headers: &HeaderMap,
) -> (LanguageCode, bool) {
    if let Some(forced) = &config.web.force_user_lang {
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

fn resolve_client_ip(
    state: &WebState,
    headers: &HeaderMap,
    fallback: std::net::IpAddr,
) -> std::net::IpAddr {
    if !state.config.web.web_app_proxy_headers {
        return fallback;
    }

    let allow = state.config.web.web_app_forwarded_allow_ips.trim();
    if allow != "*"
        && !allow
            .split(',')
            .map(str::trim)
            .any(|s| s == fallback.to_string())
    {
        return fallback;
    }

    if let Some(raw) = headers.get("x-forwarded-for") {
        match raw.to_str() {
            Ok(value) => {
                if let Some(first) = value.split(',').next().map(str::trim)
                    && let Ok(ip) = first.parse()
                {
                    return ip;
                }
            }
            Err(e) => {
                warn!(error = %e, "Invalid x-forwarded-for header encoding");
            }
        }
    }

    if let Some(raw) = headers.get("x-real-ip") {
        match raw.to_str() {
            Ok(value) => {
                if let Ok(ip) = value.parse() {
                    return ip;
                }
            }
            Err(e) => {
                warn!(error = %e, "Invalid x-real-ip header encoding");
            }
        }
    }

    fallback
}

async fn download_by_type(
    state: Arc<WebState>,
    token: String,
    token_type: DownloadTokenType,
) -> Response {
    if let Ok(Some(tok_data)) = state.db.get_download_token(&token).await {
        let Ok(stored_type) = DownloadTokenType::try_from(tok_data.token_type.as_str()) else {
            warn!(
                token_type = %tok_data.token_type,
                "Invalid download token type"
            );
            return (
                axum::http::StatusCode::NOT_FOUND,
                t("en", "web-err-invalid-link"),
            )
                .into_response();
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
                Err(e) => {
                    error!(error = %e, path = %path.display(), "Failed to open download file");
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
