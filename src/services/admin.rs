pub struct SourceInfo {
    pub lang: crate::types::LanguageCode,
    pub tg_username: String,
    pub fullname: String,
}

pub fn parse_source_info(source_info: &str) -> SourceInfo {
    let mut lang = crate::types::LanguageCode::default();
    let mut tg_username = String::new();
    let mut fullname = String::new();

    for part in source_info.split(';') {
        let mut iter = part.splitn(2, '=');
        let Some(key) = iter.next() else { continue };
        let Some(val) = iter.next() else { continue };

        match key {
            "lang" => lang = crate::types::LanguageCode::parse_or_default(val),
            "tg_username" => tg_username = val.to_string(),
            "fullname" => fullname = val.to_string(),
            _ => {}
        }
    }

    SourceInfo {
        lang,
        tg_username,
        fullname,
    }
}
