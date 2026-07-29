//! The portal: the Svelte 5 SPA (ui/portal), compiled by Vite and
//! embedded in the binary — `ui/portal/dist` is CHECKED IN, so the Rust
//! build, including the cross-compile in build-images.sh, never needs
//! Node. Served on the API's own origin, so browser calls need no CORS.
//!
//! Vite emits content-hashed asset names (assets/index-XXXX.js), so the
//! files are read with include_dir rather than one include_str! each.
//! The PWA shell (manifest, service worker, icons) is hand-written and
//! still lives in `portal/`.
//!
//! Auth: the SPA runs OIDC authorization code + PKCE against regnid; the
//! code→token exchange is proxied here (`POST /auth/token`), server-to-
//! server, so the IdP needs no browser CORS either. regnmed still never
//! sees a password — the proxy only forwards the one-time code.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{Html, IntoResponse, Redirect, Response};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::auth::ApiError;

static DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../ui/portal/dist");

// PWA (docs/portal.md, #48): the shell is installable, and the service
// worker caches ONLY the shell — never anything from the ledger.
const MANIFEST: &str = include_str!("../portal/manifest.webmanifest");
const SERVICE_WORKER: &str = include_str!("../portal/sw.js");
const ICON_192: &[u8] = include_bytes!("../portal/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../portal/icon-512.png");

pub(crate) fn index_html() -> &'static str {
    DIST.get_file("index.html")
        .and_then(|f| f.contents_utf8())
        .unwrap_or("")
}

/// `/` and `/callback`: always the app. Routing happens in the hash, and
/// the callback address must land in the app to finish the PKCE flow.
pub async fn index() -> Html<&'static str> {
    Html(index_html())
}

/// `/ny` was the app's address while the migration ran (#76). Old
/// bookmarks and open tabs should land on the portal, not a 404.
pub async fn ny_redirect() -> Redirect {
    Redirect::permanent("/")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

/// Vite's build output. The filename carries the content hash, so what
/// lives behind a given address never changes — safe to cache forever.
pub async fn asset(Path(path): Path<String>) -> Response {
    match DIST.get_file(format!("assets/{path}")) {
        Some(file) => (
            [
                (header::CONTENT_TYPE, content_type(&path)),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            file.contents(),
        )
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

pub async fn manifest() -> Response {
    (
        [(header::CONTENT_TYPE, "application/manifest+json")],
        MANIFEST,
    )
        .into_response()
}

pub async fn service_worker() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            // The worker controls the whole origin, so it must be
            // served from the root with no long-lived cache: a stale
            // worker is a stale app nobody can update.
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SERVICE_WORKER,
    )
        .into_response()
}

pub async fn icon_192() -> Response {
    ([(header::CONTENT_TYPE, "image/png")], ICON_192).into_response()
}

pub async fn icon_512() -> Response {
    ([(header::CONTENT_TYPE, "image/png")], ICON_512).into_response()
}

/// What the SPA needs to start the OIDC flow. The client id defaults to
/// the conventional public client; override with PORTAL_OIDC_CLIENT_ID.
pub async fn portal_config() -> Json<serde_json::Value> {
    Json(json!({
        "issuer": std::env::var("OIDC_ISSUER").unwrap_or_default(),
        "client_id": std::env::var("PORTAL_OIDC_CLIENT_ID")
            .unwrap_or_else(|_| "regnmed-portal".into()),
    }))
}

#[derive(Deserialize)]
pub struct TokenExchangeRequest {
    code: String,
    code_verifier: String,
    redirect_uri: String,
}

/// Proxies the authorization-code exchange to the IdP's token endpoint.
pub async fn token_exchange(
    State(_state): State<AppState>,
    Json(request): Json<TokenExchangeRequest>,
) -> Result<Response, ApiError> {
    let issuer = std::env::var("OIDC_ISSUER")
        .map_err(|_| ApiError::BadRequest("OIDC_ISSUER is not configured".into()))?;
    let client_id =
        std::env::var("PORTAL_OIDC_CLIENT_ID").unwrap_or_else(|_| "regnmed-portal".into());
    let token_endpoint = format!("{}/token", issuer.trim_end_matches('/'));

    let response = reqwest::Client::new()
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("code", request.code.as_str()),
            ("redirect_uri", request.redirect_uri.as_str()),
            ("code_verifier", request.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("IdP unreachable: {e}")))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        // Log the IdP's detail server-side; the browser gets a plain failure.
        eprintln!("token exchange failed ({status}): {body}");
        return Err(ApiError::BadRequest("innlogging avvist av utsteder".into()));
    }
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}
