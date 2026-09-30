//! The browser Client's built assets, served on the operator listener (ADR-0036).

use std::path::Path;

use axum::extract::Request;
use axum::http::header::{ALLOW, CACHE_CONTROL};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt as _;
use tower_http::services::{ServeDir, ServeFile};

const SHELL: &str = "index.html";

/// A path whose last segment names a file is an asset, and answers 404 when absent; any other
/// path is a Client route the shell resolves, so a deep link survives a refresh.
pub async fn served(built: &Path, request: Request) -> Response {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(ALLOW, HeaderValue::from_static("GET, HEAD"))],
        )
            .into_response();
    }

    let names_a_file = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'));

    if names_a_file {
        let assets = ServeDir::new(built).append_index_html_on_directories(false);
        match assets.oneshot(request).await {
            Ok(response) => response.into_response(),
            Err(never) => match never {},
        }
    } else {
        match ServeFile::new(built.join(SHELL)).oneshot(request).await {
            Ok(mut response) => {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
                response.into_response()
            }
            Err(never) => match never {},
        }
    }
}
