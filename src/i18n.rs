use fluent_templates::{Loader, fluent_bundle::FluentValue};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

fluent_templates::static_loader! {
    static LOCALES = {
        locales: "./locales",
        fallback_language: "en",
    };
}

pub fn t(lang: &str, key: &str) -> String {
    let lang_id = lang.parse().unwrap_or(unic_langid::langid!("en"));
    LOCALES.lookup(&lang_id, key)
}

pub fn t_args(lang: &str, key: &str, args: &HashMap<String, String>) -> String {
    let lang_id = lang.parse().unwrap_or(unic_langid::langid!("en"));

    let mut fluent_args: HashMap<Cow<str>, FluentValue> = HashMap::new();
    for (k, v) in args {
        fluent_args.insert(Cow::from(k.clone()), FluentValue::from(v.clone()));
    }

    LOCALES.lookup_with_args(&lang_id, key, &fluent_args)
}

static LANG_CACHE: OnceLock<Arc<Vec<(String, String)>>> = OnceLock::new();

pub fn available_languages() -> Arc<Vec<(String, String)>> {
    if let Some(cached) = LANG_CACHE.get() {
        return Arc::clone(cached);
    }

    let mut languages = Vec::new();
    let locales_dir = PathBuf::from("./locales");

    if let Ok(entries) = fs::read_dir(&locales_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(code) = path.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            let lang_id = code.parse().unwrap_or(unic_langid::langid!("en"));
            let native = LOCALES.lookup(&lang_id, "native_language_name");
            let native_name = if native == "native_language_name" {
                code.to_uppercase()
            } else {
                native
            };
            languages.push((code.to_string(), native_name));
        }
    }

    if !languages.iter().any(|(code, _)| code == "en") {
        languages.push(("en".to_string(), "English".to_string()));
    }

    let cached = Arc::new(languages);
    let _ = LANG_CACHE.set(Arc::clone(&cached));
    cached
}
