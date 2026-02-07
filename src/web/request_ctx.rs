use crate::types::LanguageCode;
use axum::http::HeaderMap;

pub(super) fn resolve_web_lang(
    config: &crate::config::AppConfig,
    headers: &HeaderMap,
) -> (LanguageCode, bool) {
    if let Some(forced) = &config.web.force_user_lang {
        let translated = crate::i18n::t(forced.as_str(), "web-label-username");
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

pub(super) fn resolve_client_ip(
    state: &super::WebState,
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
                tracing::warn!(error = %e, "Invalid x-forwarded-for header encoding");
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
                tracing::warn!(error = %e, "Invalid x-real-ip header encoding");
            }
        }
    }

    fallback
}
