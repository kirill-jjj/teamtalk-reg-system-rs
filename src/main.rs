mod config;
mod db;
mod domain;
mod files;
mod i18n;
mod services;
mod tg_bot;
mod tt;
mod types;
mod web;

use clap::Parser;
use config::AppConfig;
use db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;
use tg_bot::handlers::{Command, MyDialogue, State};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let args = Args::parse();
    let config_path = PathBuf::from(&args.config);

    info!("Starting TeamTalk Reg Bot");
    info!(config_path = ?config_path, "Loading config");

    let config = match AppConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {}", e);
            return Ok(());
        }
    };

    let rt_handle = tokio::runtime::Handle::current();

    let db_path = config.get_db_path(&config_path);
    let db_path_str = db_path.to_string_lossy().to_string();
    debug!(db_path = db_path_str, "Database path");

    let config_arc = Arc::new(config.clone());
    let db = Database::new(&db_path_str).await?;
    let (tx_tt, rx_tt) = mpsc::channel();

    let bot = Bot::new(&config.tg_bot_token);

    let db_clean = db.clone();
    let cleanup_interval = config.db_cleanup_interval_seconds;
    let file_ttl = config.generated_file_ttl_seconds;
    let pending_ttl = config.pending_reg_ttl_seconds;
    let registered_ip_ttl = config.registered_ip_ttl_seconds;

    let temp_files_dir = std::env::current_dir()?.join("temp_files");
    if !temp_files_dir.exists() {
        std::fs::create_dir(&temp_files_dir)?;
    }

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(cleanup_interval));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            debug!("Running periodic cleanup");
            if let Err(e) = db_clean.cleanup(pending_ttl, registered_ip_ttl).await {
                error!(error = %e, "DB cleanup failed");
            }
            let temp_dir = temp_files_dir.clone();
            let cleanup_ttl = file_ttl;
            let _ = tokio::task::spawn_blocking(move || {
                if let Ok(entries) = std::fs::read_dir(&temp_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && let Ok(metadata) = std::fs::metadata(&path)
                            && let Ok(modified) = metadata.modified()
                            && let Ok(age) = modified.elapsed()
                            && age.as_secs() > cleanup_ttl
                        {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            })
            .await;
        }
    });

    tokio::spawn(tt::run_tt_worker(
        config_arc.clone(),
        rx_tt,
        bot.clone(),
        db.clone(),
        rt_handle,
    ));

    if config.web_registration_enabled {
        let web_config = config.clone();
        let web_db = db.clone();
        let web_tx = tx_tt.clone();
        tokio::spawn(web::run_server(web_config, web_db, web_tx));
    }

    let handler = Update::filter_message()
        .enter_dialogue::<Message, InMemStorage<State>, State>()
        .branch(dptree::entry().filter_command::<Command>().endpoint(
            |bot: Bot,
             msg: Message,
             cmd: Command,
             db: Database,
             config: Arc<AppConfig>,
             dialogue: MyDialogue| async move {
                match cmd {
                    Command::Start => tg_bot::handlers::start(bot, msg, dialogue, db, config).await,
                    Command::AdminPanel => {
                        tg_bot::handlers::admin_panel(bot, msg, config, dialogue).await
                    }
                    Command::Generate => {
                        tg_bot::handlers::generate_invite(bot, msg, db, config).await
                    }
                    Command::Exit => tg_bot::handlers::exit_bot(bot, msg, config).await,
                    _ => Ok(()),
                }
            },
        ))
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(d.get().await.ok().flatten(), Some(State::Start))
            })
            .endpoint(tg_bot::handlers::start),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingUsername { .. })
                )
            })
            .endpoint(tg_bot::handlers::receive_username),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingPassword { .. })
                )
            })
            .endpoint(tg_bot::handlers::receive_password),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingNickname { .. })
                )
            })
            .endpoint(tg_bot::handlers::receive_nickname),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingManualBanInput)
                )
            })
            .endpoint(tg_bot::handlers::admin_manual_ban_input),
        );

    let callback_handler = Update::filter_callback_query()
        .enter_dialogue::<CallbackQuery, InMemStorage<State>, State>()
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(d.get().await.ok().flatten(), Some(State::ChoosingLanguage))
            })
            .endpoint(tg_bot::handlers::receive_language),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingNicknameChoice { .. })
                )
            })
            .endpoint(tg_bot::handlers::receive_nickname_choice),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                matches!(
                    d.get().await.ok().flatten(),
                    Some(State::AwaitingAccountType { .. })
                )
            })
            .endpoint(tg_bot::handlers::receive_account_type),
        )
        .branch(dptree::entry().endpoint(tg_bot::handlers::admin_callback));

    let schema = dptree::entry().branch(handler).branch(callback_handler);

    Dispatcher::builder(bot, schema)
        .dependencies(dptree::deps![
            db,
            config_arc,
            tx_tt,
            InMemStorage::<State>::new()
        ])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}
