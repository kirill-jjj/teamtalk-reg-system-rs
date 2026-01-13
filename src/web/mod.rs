use crate::config::AppConfig;
use crate::db::Database;
use crate::types::TTWorkerCommand;
use axum::Router;
use axum::routing::{get, post};
use axum_server::tls_rustls::RustlsConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use tracing::{error, info, warn};

mod handlers;
mod templates;

pub struct WebState {
    pub config: AppConfig,
    pub db: Database,
    pub tx_tt: Sender<TTWorkerCommand>,
    pub available_languages: std::sync::Arc<Vec<(String, String)>>,
}

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
    let mut app = Router::new()
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

    if !config.root_path.is_empty() && config.root_path != "/" {
        app = Router::new().nest(&config.root_path, app);
    }

    let addr_str = format!("{}:{}", config.web_app_host, config.web_app_port);
    info!(addr = %addr_str, "Web server listening");

    let addr: SocketAddr = match addr_str.parse() {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, addr = %addr_str, "Invalid listen address");
            return;
        }
    };

    if config.web_app_ssl_enabled {
        let cert_path = config.web_app_ssl_cert_path.clone().unwrap_or_default();
        let key_path = config.web_app_ssl_key_path.clone().unwrap_or_default();
        if cert_path.is_empty() || key_path.is_empty() {
            warn!(
                cert_path = %cert_path,
                key_path = %key_path,
                "Web SSL enabled but cert/key paths are missing. Falling back to HTTP"
            );
            let listener = match tokio::net::TcpListener::bind(&addr_str).await {
                Ok(l) => l,
                Err(e) => {
                    error!(error = %e, addr = %addr_str, "Failed to bind HTTP listener");
                    return;
                }
            };
            let shutdown_wait = shutdown.clone();
            if let Err(e) = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                shutdown_wait.cancelled().await;
            })
            .await
            {
                error!(error = %e, "HTTP server failed");
            }
            return;
        }

        let tls_config = match RustlsConfig::from_pem_file(cert_path, key_path).await {
            Ok(cfg) => cfg,
            Err(e) => {
                warn!(error = %e, "Failed to load TLS config. Falling back to HTTP");
                let listener = match tokio::net::TcpListener::bind(&addr_str).await {
                    Ok(l) => l,
                    Err(e) => {
                        error!(error = %e, addr = %addr_str, "Failed to bind HTTP listener");
                        return;
                    }
                };
                if let Err(e) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                {
                    error!(error = %e, "HTTP server failed");
                }
                return;
            }
        };
        let shutdown_wait = shutdown.clone();
        if let Err(e) = tokio::select! {
            res = axum_server::bind_rustls(addr, tls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>()) => res,
            _ = shutdown_wait.cancelled() => Ok(()),
        } {
            error!(error = %e, "HTTPS server failed");
        }
        return;
    }

    let listener = match tokio::net::TcpListener::bind(&addr_str).await {
        Ok(l) => l,
        Err(e) => {
            error!(error = %e, addr = %addr_str, "Failed to bind HTTP listener");
            return;
        }
    };
    let shutdown_wait = shutdown.clone();
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_wait.cancelled().await;
    })
    .await
    {
        error!(error = %e, "HTTP server failed");
    }
}
