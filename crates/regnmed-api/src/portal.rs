//! The portal: a static single-page app embedded in the binary
//! (include_str! — the distroless image needs no extra files) and served
//! on the API's own origin, so browser calls need no CORS.
//!
//! Auth: the SPA runs OIDC authorization code + PKCE against regnid; the
//! code→token exchange is proxied here (`POST /auth/token`), server-to-
//! server, so the IdP needs no browser CORS either. regnmed still never
//! sees a password — the proxy only forwards the one-time code.

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::auth::ApiError;

const INDEX_HTML: &str = include_str!("../portal/index.html");
const APP_JS: &str = include_str!("../portal/app.js");
const THEME_JS: &str = include_str!("../portal/theme.js");
const APP_CSS: &str = include_str!("../portal/app.css");

pub async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn app_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        APP_JS,
    )
        .into_response()
}

pub async fn theme_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        THEME_JS,
    )
        .into_response()
}

pub async fn app_css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS).into_response()
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
