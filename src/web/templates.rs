use crate::i18n::{t, t_args};
use crate::types::LanguageCode;
use askama::Template;
use askama_derive_axum::IntoResponse;
use serde::Deserialize;
use std::collections::HashMap;

/// Template context for the registration page.
#[derive(Template, IntoResponse)]
#[template(path = "register.html")]
pub struct RegisterTemplate {
    pub message: Option<String>,
    pub message_class: Option<String>,
    pub message_class_safe: String,
    pub additional_message_info: Option<String>,
    pub registration_complete: bool,
    pub server_name: String,
    pub username_val: String,
    pub nickname_val: String,
    pub tt_link: Option<String>,
    pub download_tt_token: Option<String>,
    pub download_client_zip_token: Option<String>,
    pub actual_tt_filename_for_user: Option<String>,
    pub actual_client_zip_filename_for_user: Option<String>,
    pub available_languages: Vec<(String, String)>,
    pub current_lang: String,
    pub language_forced: bool,
    pub generated_file_ttl_seconds: u64,

    pub page_title: String,
    pub page_header: String,
    pub intro_line_1: String,
    pub intro_line_2: String,
    pub label_username: String,
    pub label_nickname: String,
    pub placeholder_nickname: String,
    pub label_password: String,
    pub show_password: String,
    pub btn_register: String,
    pub select_language: String,
    pub language_label: String,
    pub set_language: String,
    pub download_msg: String,
    pub link_tt_text: String,
    pub link_zip_text: String,
    pub quick_link_text: String,
    pub countdown_text: String,
    pub expired_text: String,
    pub second_text: String,
    pub seconds_few_text: String,
    pub seconds_text: String,
}

impl RegisterTemplate {
    /// Build a new registration page template.
    pub fn new(
        server_name: &str,
        lang: &LanguageCode,
        available_languages: Vec<(String, String)>,
        language_forced: bool,
        generated_file_ttl_seconds: u64,
    ) -> Self {
        let mut args = HashMap::new();
        args.insert("server_name".to_string(), server_name.to_string());

        Self {
            message: None,
            message_class: None,
            message_class_safe: "info".to_string(),
            additional_message_info: None,
            registration_complete: false,
            server_name: server_name.to_string(),
            username_val: String::new(),
            nickname_val: String::new(),
            tt_link: None,
            download_tt_token: None,
            download_client_zip_token: None,
            actual_tt_filename_for_user: None,
            actual_client_zip_filename_for_user: None,
            available_languages,
            current_lang: lang.to_string(),
            language_forced,
            generated_file_ttl_seconds,

            page_title: t_args(lang.as_str(), "web-title", &args),
            page_header: t_args(lang.as_str(), "web-header", &args),
            intro_line_1: t(lang.as_str(), "web-intro-line-1"),
            intro_line_2: t(lang.as_str(), "web-intro-line-2"),
            label_username: t(lang.as_str(), "web-label-username"),
            label_nickname: t(lang.as_str(), "web-label-nickname"),
            placeholder_nickname: t(lang.as_str(), "web-placeholder-nickname"),
            label_password: t(lang.as_str(), "web-label-password"),
            show_password: t(lang.as_str(), "web-show-password"),
            btn_register: t(lang.as_str(), "web-btn-register"),
            select_language: t(lang.as_str(), "web-select-language"),
            language_label: t(lang.as_str(), "web-language-label"),
            set_language: t(lang.as_str(), "web-set-language"),
            download_msg: t(lang.as_str(), "web-download-msg"),
            link_tt_text: t(lang.as_str(), "web-link-tt"),
            link_zip_text: t(lang.as_str(), "web-link-zip"),
            quick_link_text: t(lang.as_str(), "web-quick-link"),
            countdown_text: t(lang.as_str(), "web-countdown-text"),
            expired_text: t(lang.as_str(), "web-expired"),
            second_text: t(lang.as_str(), "web-second"),
            seconds_few_text: t(lang.as_str(), "web-seconds-few"),
            seconds_text: t(lang.as_str(), "web-seconds"),
        }
    }
}

/// Registration form payload.
#[derive(Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub nickname: String,
    pub password: String,
}
