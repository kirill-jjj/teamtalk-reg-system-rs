//! `TeamTalk` registration bot and web service entry point.
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

use anyhow::{Context, Result};
use clap::Parser;
use config::AppConfig;
use db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use teloxide::dispatching::UpdateHandler;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;
use tg_bot::handlers::{Command, MyDialogue, State};
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

type HandlerError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = PathBuf::from(&args.config);
    let config = AppConfig::load(&config_path)
        .with_context(|| format!("Failed to load config at {}", config_path.display()))?;

    let (env_filter, log_warning) = build_env_filter(&config);
    init_tracing(env_filter);
    if let Some(message) = log_warning {
        warn!("{message}");
    }

    info!(config_path = ?config_path, "Loading config");
    info!("Starting TeamTalk Reg Bot");

    run_app(config, config_path).await
}

fn init_tracing(env_filter: EnvFilter) {
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}

fn build_env_filter(config: &AppConfig) -> (EnvFilter, Option<String>) {
    config.logging.log_level.as_ref().map_or_else(
        || {
            (
                EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
                None,
            )
        },
        |level| match EnvFilter::try_new(level) {
            Ok(filter) => (filter, None),
            Err(err) => (
                EnvFilter::new("info"),
                Some(format!("Invalid log_level '{level}': {err}")),
            ),
        },
    )
}

async fn run_app(config: AppConfig, config_path: PathBuf) -> Result<()> {
    let shutdown = CancellationToken::new();
    let db = init_db(&config, &config_path).await?;
    let (tx_tt, rx_tt) = mpsc::channel();
    let bot = Bot::new(&config.telegram.tg_bot_token);

    ensure_temp_dir()?;

    let cleanup_handle = spawn_cleanup_task(
        db.clone(),
        shutdown.clone(),
        config.database.db_cleanup_interval_seconds,
        config.database.pending_reg_ttl_seconds,
        config.database.registered_ip_ttl_seconds,
        config.database.generated_file_ttl_seconds,
    );

    let tt_handle = spawn_tt_worker(
        Arc::new(config.clone()),
        rx_tt,
        bot.clone(),
        db.clone(),
        tokio::runtime::Handle::current(),
        shutdown.clone(),
    );

    let web_handle = spawn_web_server(&config, db.clone(), tx_tt.clone(), shutdown.clone());

    let (dispatch_handle, shutdown_task) = spawn_dispatcher(bot, &db, tx_tt, config, shutdown);

    wait_for_tasks(
        dispatch_handle,
        shutdown_task,
        cleanup_handle,
        tt_handle,
        web_handle,
    )
    .await;

    info!("Closing database pool...");
    db.close().await;
    info!("Database pool closed.");

    Ok(())
}

async fn init_db(config: &AppConfig, config_path: &std::path::Path) -> Result<Database> {
    let db_path = config.get_db_path(config_path);
    let db_path_str = db_path.to_string_lossy().to_string();
    debug!(db_path = db_path_str, "Database path");
    Database::new(&db_path_str).await
}

fn ensure_temp_dir() -> Result<()> {
    let temp_files_dir = std::env::current_dir()?.join("temp_files");
    if !temp_files_dir.exists() {
        std::fs::create_dir(&temp_files_dir)?;
    }
    Ok(())
}

fn spawn_cleanup_task(
    db: Database,
    shutdown: CancellationToken,
    cleanup_interval_seconds: u64,
    pending_ttl_seconds: u64,
    registered_ip_ttl_seconds: u64,
    generated_file_ttl_seconds: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(cleanup_interval_seconds));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                _ = interval.tick() => {}
            }
            debug!("Running periodic cleanup");
            if let Err(e) = db
                .cleanup(pending_ttl_seconds, registered_ip_ttl_seconds)
                .await
            {
                tracing::error!(error = %e, "DB cleanup failed");
            }
            cleanup_temp_files(generated_file_ttl_seconds).await;
        }
    })
}

async fn cleanup_temp_files(file_ttl_seconds: u64) {
    let temp_dir = match std::env::current_dir() {
        Ok(dir) => dir.join("temp_files"),
        Err(_) => return,
    };
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Ok(metadata) = std::fs::metadata(&path)
                    && let Ok(modified) = metadata.modified()
                    && let Ok(age) = modified.elapsed()
                    && age.as_secs() > file_ttl_seconds
                {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    })
    .await;
}

fn spawn_tt_worker(
    config: Arc<AppConfig>,
    rx_tt: mpsc::Receiver<types::TTWorkerCommand>,
    bot: Bot,
    db: Database,
    rt_handle: tokio::runtime::Handle,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tt::run_tt_worker(config, rx_tt, bot, db, rt_handle, shutdown).await;
    })
}

fn spawn_web_server(
    config: &AppConfig,
    db: Database,
    tx_tt: mpsc::Sender<types::TTWorkerCommand>,
    shutdown: CancellationToken,
) -> Option<JoinHandle<()>> {
    if !config.web.web_registration_enabled {
        return None;
    }
    let web_config = config.clone();
    Some(tokio::spawn(async move {
        web::run_server(web_config, db, tx_tt, shutdown).await;
    }))
}

fn build_message_handler() -> UpdateHandler<HandlerError> {
    Update::filter_message()
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
                    Command::Help => Ok(()),
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
        )
}

fn build_callback_handler() -> UpdateHandler<HandlerError> {
    Update::filter_callback_query()
        .enter_dialogue::<CallbackQuery, InMemStorage<State>, State>()
        .branch(
            dptree::filter_async(|d: MyDialogue| async move {
                match d.get().await {
                    Ok(state) => matches!(state, Some(State::ChoosingLanguage)),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to read dialogue state (ChoosingLanguage)"
                        );
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
        .branch(dptree::entry().endpoint(tg_bot::handlers::admin_callback))
}

fn spawn_dispatcher(
    bot: Bot,
    db: &Database,
    tx_tt: mpsc::Sender<types::TTWorkerCommand>,
    config: AppConfig,
    shutdown: CancellationToken,
) -> (JoinHandle<()>, JoinHandle<()>) {
    let config_arc = Arc::new(config);
    let schema = dptree::entry()
        .branch(build_message_handler())
        .branch(build_callback_handler());

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

    let shutdown_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        shutdown.cancel();
        if let Ok(fut) = shutdown_token.shutdown() {
            fut.await;
        }
    });

    (dispatch_handle, shutdown_task)
}

async fn wait_for_tasks(
    dispatch_handle: JoinHandle<()>,
    shutdown_task: JoinHandle<()>,
    cleanup_handle: JoinHandle<()>,
    tt_handle: JoinHandle<()>,
    web_handle: Option<JoinHandle<()>>,
) {
    if let Err(e) = dispatch_handle.await {
        tracing::error!(error = ?e, "Dispatcher task failed");
    }
    if let Err(e) = shutdown_task.await {
        tracing::error!(error = ?e, "Shutdown task failed");
    }
    if let Err(e) = cleanup_handle.await {
        tracing::error!(error = ?e, "Cleanup task failed");
    }
    if let Err(e) = tt_handle.await {
        tracing::error!(error = ?e, "TT worker task failed");
    }
    if let Some(handle) = web_handle
        && let Err(e) = handle.await
    {
        tracing::error!(error = ?e, "Web server task failed");
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(sigterm) => sigterm,
        Err(e) => {
            tracing::error!(error = %e, "Failed to register SIGTERM handler");
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
        tracing::error!(error = %e, "Failed to listen for Ctrl+C");
    }
}
