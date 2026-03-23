//! Embedded Vite/React frontend served from `frontend/dist`.
//!
//! The dashboard is built during `cargo build` by `build.rs`, then embedded
//! into the API binary so system builds automatically ship the latest frontend.

use axum::body::Body;
use axum::extract::OriginalUri;
use axum::http::{header, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use include_dir::{include_dir, Dir};

/// Compile-time ETag based on the crate version.
const ETAG: &str = concat!("\"silicrew-", env!("CARGO_PKG_VERSION"), "\"");

/// Embedded Vite build output.
static FRONTEND_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../frontend/dist");

/// Fallback assets preserved from the legacy static dashboard.
const FALLBACK_FAVICON_ICO: &[u8] = include_bytes!("../static/favicon.ico");
const FALLBACK_MANIFEST_JSON: &str = include_str!("../static/manifest.json");
const FALLBACK_SW_JS: &str = include_str!("../static/sw.js");
const FALLBACK_LOGO_PNG: &[u8] = include_bytes!("../static/logo.png");

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn cache_control_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "public, max-age=3600, must-revalidate"
    } else if path.contains("/assets/") || path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=86400, immutable"
    }
}

fn response_from_bytes(path: &str, bytes: Vec<u8>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for(path))
        .header(header::CACHE_CONTROL, cache_control_for(path))
        .header(header::ETAG, ETAG)
        .body(Body::from(bytes))
        .expect("frontend asset response")
}

fn response_from_embedded_file(path: &str) -> Option<Response<Body>> {
    FRONTEND_DIST
        .get_file(path)
        .map(|file| response_from_bytes(path, file.contents().to_vec()))
}

fn frontend_index_response() -> Response<Body> {
    response_from_embedded_file("index.html")
        .unwrap_or_else(|| response_from_bytes("index.html", b"Frontend build missing".to_vec()))
}

/// GET /logo.png — Serve the frontend logo.
pub async fn logo_png() -> impl IntoResponse {
    response_from_embedded_file("logo.png")
        .unwrap_or_else(|| response_from_bytes("logo.png", FALLBACK_LOGO_PNG.to_vec()))
}

/// GET /favicon.ico — Serve the favicon.
pub async fn favicon_ico() -> impl IntoResponse {
    response_from_embedded_file("favicon.ico")
        .unwrap_or_else(|| response_from_bytes("favicon.ico", FALLBACK_FAVICON_ICO.to_vec()))
}

/// GET /manifest.json — Serve the PWA manifest.
pub async fn manifest_json() -> impl IntoResponse {
    response_from_embedded_file("manifest.json").unwrap_or_else(|| {
        response_from_bytes("manifest.json", FALLBACK_MANIFEST_JSON.as_bytes().to_vec())
    })
}

/// GET /sw.js — Serve the service worker.
pub async fn sw_js() -> impl IntoResponse {
    response_from_embedded_file("sw.js")
        .unwrap_or_else(|| response_from_bytes("sw.js", FALLBACK_SW_JS.as_bytes().to_vec()))
}

/// GET / — Serve the Vite SPA shell.
pub async fn webchat_page() -> impl IntoResponse {
    frontend_index_response()
}

/// SPA/static fallback for the compiled frontend.
///
/// - Known asset files are served directly from the embedded `frontend/dist`
/// - Unknown extension-less paths return `index.html` for React Router
/// - Unknown API-ish paths stay 404 instead of accidentally returning HTML
pub async fn frontend_fallback(OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() {
        return frontend_index_response();
    }

    if path.starts_with("api/")
        || path.starts_with("hooks/")
        || path == "mcp"
        || path.starts_with("mcp/")
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(response) = response_from_embedded_file(path) {
        return response;
    }

    if path.contains('.') {
        return (
            StatusCode::NOT_FOUND,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            "Not Found",
        )
            .into_response();
    }

    frontend_index_response()
}
