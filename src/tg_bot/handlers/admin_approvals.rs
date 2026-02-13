use super::HandlerResult;
use super::admin::PendingApproval;
use super::registration::{notify_db_sync_error, send_registration_assets};
use crate::config::AppConfig;
use crate::db::Database;
use crate::domain::{Nickname, Password, Username};
use crate::i18n::{t, t_args};
use crate::services::admin::parse_source_info;
use crate::services::registration;
use crate::types::{LanguageCode, RegistrationSource, TTAccountType, TTWorkerCommand};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use teloxide_ng::prelude::*;
use teloxide_ng::types::ChatId;
use tracing::warn;

pub(super) struct AdminApproveInput<'a> {
    pub(super) bot: &'a Bot,
    pub(super) q: &'a CallbackQuery,
    pub(super) db: &'a Database,
    pub(super) config: &'a AppConfig,
    pub(super) lang: &'a LanguageCode,
    pub(super) req_id: &'a str,
    pub(super) tx_tt: Sender<TTWorkerCommand>,
    pub(super) chat_id: i64,
}

pub(super) async fn handle_admin_approve(input: AdminApproveInput<'_>) -> HandlerResult {
    let AdminApproveInput {
        bot,
        q,
        db,
        config,
        lang,
        req_id,
        tx_tt,
        chat_id,
    } = input;
    let Some(pending) = load_pending_approval(bot, q, db, lang, req_id).await? else {
        return Ok(());
    };

    let result = registration::create_teamtalk_account(registration::CreateAccountParams {
        username: &pending.username,
        password: &pending.password,
        nickname: &pending.nickname,
        account_type: TTAccountType::Default,
        source: RegistrationSource::Telegram(pending.registrant_id),
        source_info: Some(pending.source_info.clone()),
        telegram_id: Some(pending.registrant_id),
        tx_tt: tx_tt.clone(),
        db,
        config,
    })
    .await?;

    notify_user_approved(bot, pending.registrant_id, &pending.req_lang).await;
    notify_admin_approve_alert(bot, q, lang, pending.username.as_str()).await?;

    if !result.created {
        notify_admin_approve_failed(bot, chat_id, lang, pending.username.as_str()).await;
        notify_admin_decision(
            bot,
            config,
            q,
            "approved",
            pending.username.as_str(),
            pending.registrant_id,
            &pending.source_info,
        )
        .await;
        db.delete_pending_registration(req_id).await?;
        return Ok(());
    }

    handle_approval_success(
        bot,
        config,
        &pending,
        result.db_sync_error.as_deref(),
        result.assets.as_ref(),
    )
    .await;

    notify_admin_decision(
        bot,
        config,
        q,
        "approved",
        pending.username.as_str(),
        pending.registrant_id,
        &pending.source_info,
    )
    .await;
    db.delete_pending_registration(req_id).await?;
    Ok(())
}

pub(super) async fn handle_admin_reject(
    bot: &Bot,
    q: &CallbackQuery,
    db: &Database,
    config: &AppConfig,
    lang: &LanguageCode,
    req_id: &str,
) -> HandlerResult {
    if let Ok(Some(req)) = db.get_pending_registration(req_id).await {
        let username = req.username.clone();
        let req_lang = parse_source_info(&req.source_info).lang;
        bot.send_message(
            ChatId(req.registrant_telegram_id.as_i64()),
            t(req_lang.as_str(), "admin-rejected"),
        )
        .await?;
        let alert_args = HashMap::from([("username".to_string(), username.clone())]);
        bot.answer_callback_query(q.id.clone())
            .text(t_args(
                lang.as_str(),
                "admin-req-rejected-alert",
                &alert_args,
            ))
            .await?;
        if let Some(m) = &q.message
            && let Err(e) = bot.delete_message(m.chat().id, m.id()).await
        {
            warn!(error = %e, "Failed to delete admin request message");
        }
        notify_admin_decision(
            bot,
            config,
            q,
            "rejected",
            &username,
            req.registrant_telegram_id,
            &req.source_info,
        )
        .await;
        db.delete_pending_registration(req_id).await?;
    } else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        if let Some(m) = &q.message {
            bot.edit_message_text(m.chat().id, m.id(), t(lang.as_str(), "admin-req-handled"))
                .await?;
        }
    }

    Ok(())
}

async fn notify_admin_decision(
    bot: &Bot,
    config: &AppConfig,
    q: &CallbackQuery,
    decision: &str,
    username: &str,
    registrant_telegram_id: crate::types::TelegramId,
    source_info: &str,
) {
    let admin_lang = config.telegram.bot_admin_lang.clone();
    let source = parse_source_info(source_info);
    let user_lang = source.lang;
    let tg_username = source.tg_username;
    let fullname = source.fullname;
    let admin_name = q.from.full_name();

    let decision_text = if decision == "approved" {
        t(admin_lang.as_str(), "admin-decision-approved")
    } else {
        t(admin_lang.as_str(), "admin-decision-rejected")
    };

    let mut args = HashMap::new();
    args.insert("admin_name".to_string(), admin_name);
    args.insert("admin_id".to_string(), q.from.id.0.to_string());
    args.insert("decision".to_string(), decision_text);
    args.insert("teamtalk_username".to_string(), username.to_string());
    args.insert(
        "registrant_telegram_id".to_string(),
        registrant_telegram_id.to_string(),
    );
    args.insert("registrant_fullname".to_string(), fullname);
    args.insert("registrant_tg_username".to_string(), tg_username);
    args.insert(
        "registrant_lang".to_string(),
        user_lang.as_str().to_string(),
    );

    let mut text = t_args(admin_lang.as_str(), "admin-decision-notify", &args);
    if !args
        .get("registrant_tg_username")
        .unwrap_or(&String::new())
        .is_empty()
    {
        let suffix = t_args(
            admin_lang.as_str(),
            "admin-decision-telegram-username",
            &args,
        );
        text.push_str(&suffix);
    }
    for &admin_id in &config.telegram.admin_ids {
        if let Ok(sender_id) = i64::try_from(q.from.id.0)
            && admin_id.as_i64() != sender_id
        {
            let _ = bot.send_message(ChatId(admin_id.as_i64()), &text).await;
        }
    }
}

async fn load_pending_approval(
    bot: &Bot,
    q: &CallbackQuery,
    db: &Database,
    lang: &LanguageCode,
    req_id: &str,
) -> Result<Option<PendingApproval>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(req) = db.get_pending_registration(req_id).await? else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        if let Some(m) = &q.message {
            bot.edit_message_text(m.chat().id, m.id(), t(lang.as_str(), "admin-req-handled"))
                .await?;
        }
        return Ok(None);
    };

    let Some(username) = Username::parse(&req.username) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };
    let Some(password) = Password::parse(&req.password) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };
    let Some(nickname) = Nickname::parse(&req.nickname) else {
        bot.answer_callback_query(q.id.clone())
            .text(t(lang.as_str(), "admin-req-not-found"))
            .await?;
        return Ok(None);
    };

    Ok(Some(PendingApproval {
        username,
        password,
        nickname,
        req_lang: parse_source_info(&req.source_info).lang,
        registrant_id: req.registrant_telegram_id,
        source_info: req.source_info.clone(),
    }))
}

async fn notify_user_approved(
    bot: &Bot,
    registrant_id: crate::types::TelegramId,
    req_lang: &LanguageCode,
) {
    if let Err(e) = bot
        .send_message(
            ChatId(registrant_id.as_i64()),
            t(req_lang.as_str(), "admin-approved"),
        )
        .await
    {
        warn!(error = %e, "Failed to notify user about approval");
    }
}

async fn notify_admin_approve_alert(
    bot: &Bot,
    q: &CallbackQuery,
    lang: &LanguageCode,
    username: &str,
) -> HandlerResult {
    let alert_args = HashMap::from([("username".to_string(), username.to_string())]);
    bot.answer_callback_query(q.id.clone())
        .text(t_args(
            lang.as_str(),
            "admin-req-approved-alert",
            &alert_args,
        ))
        .await?;
    if let Some(m) = &q.message
        && let Err(e) = bot.delete_message(m.chat().id, m.id()).await
    {
        warn!(error = %e, "Failed to delete admin request message");
    }
    Ok(())
}

async fn notify_admin_approve_failed(bot: &Bot, chat_id: i64, lang: &LanguageCode, username: &str) {
    if let Err(e) = bot
        .send_message(
            ChatId(chat_id),
            t_args(
                lang.as_str(),
                "admin-approve-failed-critical",
                &HashMap::from([("username".to_string(), username.to_string())]),
            ),
        )
        .await
    {
        warn!(error = %e, "Failed to notify admin about approval failure");
    }
}

async fn handle_approval_success(
    bot: &Bot,
    config: &AppConfig,
    pending: &PendingApproval,
    db_sync_error: Option<&str>,
    assets: Option<&registration::RegistrationAssets>,
) {
    if let Some(err) = db_sync_error {
        notify_db_sync_error(
            bot,
            config,
            ChatId(pending.registrant_id.as_i64()),
            pending.username.as_str(),
            err,
        )
        .await;
        if let Err(e) = bot
            .send_message(
                ChatId(pending.registrant_id.as_i64()),
                t(pending.req_lang.as_str(), "register-success-db-sync-issue"),
            )
            .await
        {
            warn!(error = %e, "Failed to notify user about db sync issue");
        }
    }
    if let Some(assets) = assets
        && let Err(e) = send_registration_assets(
            bot,
            ChatId(pending.registrant_id.as_i64()),
            pending.req_lang.as_str(),
            config,
            pending.username.as_str(),
            pending.password.as_str(),
            assets,
        )
        .await
    {
        warn!(error = %e, "Failed to send registration assets to user");
    }
}
