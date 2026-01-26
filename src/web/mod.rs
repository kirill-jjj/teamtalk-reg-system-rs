use crate::config::AppConfig;
use crate::db::Database;
use crate::types::TTWorkerCommand;
use axum::Router;
use axum::routing::{get, post};
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

mod handlers;
mod templates;

struct WebState {
    config: AppConfig,
    db: Database,
    tx_tt: Sender<TTWorkerCommand>,
    available_languages: Arc<Vec<(String, String)>>,
}

/// Run the web server for public registration endpoints.
pub async fn run_server(
    config: AppConfig,
    db: Database,
    tx_tt: Sender<TTWorkerCommand>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    let state = Arc::new(WebState {
        config: config.clone(),
        db,
        tx_tt,
        available_languages: crate::i18n::available_languages(),
    });

    let app = build_router(state, &config.web.root_path);

    let addr_str = format!("{}:{}", config.web.web_app_host, config.web.web_app_port);
    info!(addr = %addr_str, "Web server listening");

    let addr: SocketAddr = match addr_str.parse() {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, addr = %addr_str, "Invalid listen address");
            return;
        }
    };

    if config.web.web_app_ssl_enabled {
        if let Err(e) = serve_https(&config, addr, addr_str.clone(), app, shutdown.clone()).await {
            error!(error = %e, "HTTPS server failed");
        }
        return;
    }

    if let Err(e) = serve_http(addr_str, app, shutdown).await {
        error!(error = %e, "HTTP server failed");
    }
}

fn build_router(state: Arc<WebState>, root_path: &str) -> Router {
    let app = Router::new()
        .route(
            "/register",
            get(handlers::register_page).post(handlers::register_post),
        )
        .route(
            "/set_lang_and_reload",
            post(handlers::set_language_and_reload),
        )
        .route("/download/{token}", get(handlers::download_handler))
        .route("/download_tt/{token}", get(handlers::download_tt_handler))
        .route(
            "/download_client_zip/{token}",
            get(handlers::download_client_zip_handler),
        )
        .with_state(state);

    if !root_path.is_empty() && root_path != "/" {
        Router::new().nest(root_path, app)
    } else {
        app
    }
}

async fn serve_http(
    addr_str: String,
    app: Router,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr_str)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind HTTP listener on {addr_str}: {e}"))?;

    let shutdown_wait = shutdown.clone();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_wait.cancelled().await;
    })
    .await
    .map_err(|e| anyhow::anyhow!("HTTP server failed: {e}"))
}

async fn serve_https(
    config: &AppConfig,
    addr: SocketAddr,
    addr_str: String,
    app: Router,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let cert_path = config.web.web_app_ssl_cert_path.clone().unwrap_or_default();
    let key_path = config.web.web_app_ssl_key_path.clone().unwrap_or_default();
    if cert_path.is_empty() || key_path.is_empty() {
        warn!(
            cert_path = %cert_path,
            key_path = %key_path,
            "Web SSL enabled but cert/key paths are missing. Falling back to HTTP"
        );
        return serve_http(addr_str, app, shutdown).await;
    }

    let tls_config = match RustlsConfig::from_pem_file(cert_path, key_path).await {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(error = %e, "Failed to load TLS config. Falling back to HTTP");
            return serve_http(addr_str, app, shutdown).await;
        }
    };

    let shutdown_wait = shutdown.clone();
    tokio::select! {
        res = axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>()) => {
            res.map_err(|e| anyhow::anyhow!("HTTPS server failed: {e}"))
        }
        () = shutdown_wait.cancelled() => Ok(()),
    }
}
