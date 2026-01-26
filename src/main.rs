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
use tokio_util::sync::CancellationToken;
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
    let shutdown = CancellationToken::new();

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

    let cleanup_shutdown = shutdown.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(cleanup_interval));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = cleanup_shutdown.cancelled() => break,
                _ = interval.tick() => {}
            }
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

    let tt_handle = tokio::spawn(tt::run_tt_worker(
        config_arc.clone(),
        rx_tt,
        bot.clone(),
        db.clone(),
        rt_handle,
        shutdown.clone(),
    ));

    let web_handle = if config.web_registration_enabled {
        let web_config = config.clone();
        let web_db = db.clone();
        let web_tx = tx_tt.clone();
        Some(tokio::spawn(web::run_server(
            web_config,
            web_db,
            web_tx,
            shutdown.clone(),
        )))
    } else {
        None
    };

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
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::Start)),
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to read dialogue state (Start)");
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::start),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingUsername { .. })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingUsername)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_username),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingPassword { .. })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingPassword)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_password),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingNickname { .. })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingNickname)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_nickname),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingManualBanInput)),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingManualBanInput)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::admin_manual_ban_input),
        );

    let callback_handler = Update::filter_callback_query()
        .enter_dialogue::<CallbackQuery, InMemStorage<State>, State>()
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::ChoosingLanguage)),
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to read dialogue state (ChoosingLanguage)");
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_language),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingNicknameChoice { .. })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingNicknameChoice)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_nickname_choice),
        )
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::AwaitingAccountType { .. })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (AwaitingAccountType)"
                        );
                        false
                    }
                }
            })
            .endpoint(tg_bot::handlers::receive_account_type),
        )
        .branch(dptree::entry().endpoint(tg_bot::handlers::admin_callback));

    let schema = dptree::entry().branch(handler).branch(callback_handler);

    let mut dispatcher = Dispatcher::builder(bot, schema)
        .dependencies(dptree::deps![
            db.clone(),
            config_arc,
            tx_tt,
            InMemStorage::<State>::new()
        ])
        .build();

    let shutdown_token = dispatcher.shutdown_token();
    let dispatch_handle = tokio::spawn(async move {
        dispatcher.dispatch().await;
    });

    let shutdown_task = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            wait_for_shutdown_signal().await;
            shutdown.cancel();
            if let Ok(fut) = shutdown_token.shutdown() {
                fut.await;
            }
        }
    });

    let _ = dispatch_handle.await;
    let _ = shutdown_task.await;
    let _ = cleanup_handle.await;
    let _ = tt_handle.await;
    if let Some(handle) = web_handle {
        let _ = handle.await;
    }

    info!("Closing database pool...");
    db.close().await;
    info!("Database pool closed.");

    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(sigterm) => sigterm,
        Err(e) => {
            error!(error = %e, "Failed to register SIGTERM handler");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        error!(error = %e, "Failed to listen for Ctrl+C");
    }
}
