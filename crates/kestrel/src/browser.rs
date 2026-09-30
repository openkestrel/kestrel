use std::path::Path;

use axum::extract::Request;
use axum::http::header::{ALLOW, CACHE_CONTROL};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt as _;
use tower_http::services::{ServeDir, ServeFile};

const SHELL: &str = "index.html";

/// A build puts files only at its root and under `/assets/`, so any other path is a Client
/// route the shell resolves (ADR-0036), even one naming an Organization with a dot in it.
pub async fn served(built: &Path, request: Request) -> Response {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }

    let path = request.uri().path();
    let names_a_built_file =
        path.starts_with("/assets/") || (path.rfind('/') == Some(0) && path.contains('.'));

    if names_a_built_file {
        let files = ServeDir::new(built).append_index_html_on_directories(false);
        let Ok(file) = files.oneshot(request).await;
        file.into_response()
    } else {
        let Ok(mut shell) = ServeFile::new(built.join(SHELL)).oneshot(request).await;
        shell
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        shell.into_response()
    }
}
