use super::WebState;
use crate::i18n::t;
use crate::types::DownloadTokenType;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use tokio::fs::File;
use tokio_util::io::ReaderStream;
use tracing::{error, warn};

/// Download handler for generic tokens.
pub(super) async fn download_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    if let Ok(Some(tok_data)) = state.db.get_download_token(&token).await {
        let temp_dir = match std::env::current_dir() {
            Ok(dir) => dir.join("temp_files"),
            Err(e) => {
                error!(error = %e, "Failed to resolve temp dir");
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    t("en", "web-err-invalid-link"),
                )
                    .into_response();
            }
        };
        let path = temp_dir.join(&tok_data.filepath_on_server);

        if path.exists() {
            if let Err(e) = state.db.mark_token_used(&token).await {
                warn!(error = %e, "Failed to mark token used");
            }

            let file = match File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    error!(error = %e, path = %path.display(), "Failed to open file for download");
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-file-not-found"),
                    )
                        .into_response();
                }
            };

            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);

            let mime = mime_guess::from_path(&path).first_or_octet_stream();

            let response = axum::response::Response::builder()
                .header("Content-Type", mime.as_ref())
                .header(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", tok_data.original_filename),
                )
                .body(body);
            return match response {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Failed to build response");
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-invalid-link"),
                    )
                        .into_response()
                }
            };
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        t("en", "web-err-invalid-link"),
    )
        .into_response()
}

/// Download handler for `TeamTalk` `.tt` config files.
pub(super) async fn download_tt_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::TtConfig).await
}

/// Download handler for client ZIP.
pub(super) async fn download_client_zip_handler(
    State(state): State<Arc<WebState>>,
    Path(token): Path<String>,
) -> Response {
    download_by_type(state, token, DownloadTokenType::ClientZip).await
}

async fn download_by_type(
    state: Arc<WebState>,
    token: String,
    token_type: DownloadTokenType,
) -> Response {
    if let Ok(Some(tok_data)) = state.db.get_download_token(&token).await {
        let Ok(stored_type) = DownloadTokenType::try_from(tok_data.token_type.as_str()) else {
            warn!(
                token_type = %tok_data.token_type,
                "Invalid download token type"
            );
            return (
                axum::http::StatusCode::NOT_FOUND,
                t("en", "web-err-invalid-link"),
            )
                .into_response();
        };
        if stored_type != token_type {
            return (
                axum::http::StatusCode::NOT_FOUND,
                t("en", "web-err-invalid-link"),
            )
                .into_response();
        }
        let temp_dir = match std::env::current_dir() {
            Ok(dir) => dir.join("temp_files"),
            Err(e) => {
                error!(error = %e, "Failed to resolve temp dir");
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    t("en", "web-err-invalid-link"),
                )
                    .into_response();
            }
        };
        let path = temp_dir.join(&tok_data.filepath_on_server);

        if path.exists() {
            if let Err(e) = state.db.mark_token_used(&token).await {
                warn!(error = %e, "Failed to mark token used");
            }

            let file = match File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    error!(error = %e, path = %path.display(), "Failed to open download file");
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-file-not-found"),
                    )
                        .into_response();
                }
            };

            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);
            let mime = mime_guess::from_path(&path).first_or_octet_stream();

            let response = axum::response::Response::builder()
                .header("Content-Type", mime.as_ref())
                .header(
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", tok_data.original_filename),
                )
                .body(body);
            return match response {
                Ok(resp) => resp,
                Err(e) => {
                    error!(error = %e, "Failed to build response");
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        t("en", "web-err-invalid-link"),
                    )
                        .into_response()
                }
            };
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        t("en", "web-err-invalid-link"),
    )
        .into_response()
}
