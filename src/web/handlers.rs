use super::WebState;
pub(super) use super::downloads::{
    download_client_zip_handler, download_handler, download_tt_handler,
};
pub(super) use super::register_submit::register_post;
use super::request_ctx::resolve_web_lang;
use super::templates::RegisterTemplate;
use crate::types::LanguageCode;
use axum::extract::{Form, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

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
