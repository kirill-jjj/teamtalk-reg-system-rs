use super::WebState;
use super::register_submit::error_template;
use super::templates::{RegisterForm, RegisterTemplate};
use crate::domain::{Nickname, Password, Username};
use crate::i18n::t;
use crate::services::registration;
use crate::types::{DownloadTokenType, LanguageCode};
use chrono::{Duration, Utc};
use tracing::{error, warn};
use uuid::Uuid;

pub(super) struct WebSuccessParams<'a> {
    pub(super) state: &'a WebState,
    pub(super) lang: &'a LanguageCode,
    pub(super) language_forced: bool,
    pub(super) ip: std::net::IpAddr,
    pub(super) form: &'a RegisterForm,
    pub(super) username: &'a Username,
    pub(super) password: &'a Password,
    pub(super) nickname: &'a Nickname,
}

struct WebBuildContext<'a> {
    state: &'a WebState,
    lang: &'a LanguageCode,
    language_forced: bool,
    form: &'a RegisterForm,
}

pub(super) async fn build_success_template(params: WebSuccessParams<'_>) -> RegisterTemplate {
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

    let mut tpl = super::register_submit::base_template(state, lang, language_forced);
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
