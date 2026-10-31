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
//!
//! The proxy also KEEPS the refresh token: it goes back as an HttpOnly
//! cookie and `POST /auth/refresh` spends it, so the 30-day credential
//! never exists anywhere JavaScript can reach it. See docs/portal.md.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
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
/// Also carries the platform-wide UI settings (migration 0053) — the
/// icon style is locked globally by systemadmin, so the portal reads it
/// here at boot instead of from any per-user storage.
pub async fn portal_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ikonstil = regnmed_db::platform_setting(&state.pool, "ikonstil")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "linje".into());
    Json(json!({
        "issuer": std::env::var("OIDC_ISSUER").unwrap_or_default(),
        "client_id": std::env::var("PORTAL_OIDC_CLIENT_ID")
            .unwrap_or_else(|_| "regnmed-portal".into()),
        "ikonstil": ikonstil,
        // Which build answered — baked in by CI (.github/workflows/
        // images.yml sets REGNMED_VERSION to the tag or sha-<short>).
        // "dev" is the truthful local answer, not a missing value.
        "versjon": option_env!("REGNMED_VERSION").unwrap_or("dev"),
    }))
}

#[derive(Deserialize)]
pub struct TokenExchangeRequest {
    code: String,
    code_verifier: String,
    redirect_uri: String,
}

/// The refresh token NEVER reaches JavaScript. It lives 30 days, which
/// makes it the most valuable thing in the whole session, and the one
/// place a 30-day credential can sit where an XSS cannot read it is an
/// HttpOnly cookie. This proxy is already the only code that talks to
/// the token endpoint, so it is also the natural keeper: the browser is
/// handed the hour-long access token and nothing else.
const REFRESH_COOKIE: &str = "regnmed_refresh";

/// Matches regnid's REFRESH_TTL_DAYS. A cookie that outlives the token
/// only means a refresh that fails; one that dies first logs the user
/// out early, so this errs long.
const REFRESH_COOKIE_MAX_AGE: i64 = 30 * 24 * 3600;

fn portal_client_id() -> String {
    std::env::var("PORTAL_OIDC_CLIENT_ID").unwrap_or_else(|_| "regnmed-portal".into())
}

/// `Secure` always, EXCEPT on a loopback host. Deriving it from
/// X-Forwarded-Proto would fail open the day a proxy stops setting the
/// header; a hostname cannot drift that way, and no deployment of ours
/// is served from localhost.
fn is_local_host(headers: &HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    host == "localhost" || host == "127.0.0.1" || host.ends_with(".localhost")
}

/// `Path=/auth` so the cookie is sent to the two routes that spend it
/// and to nothing else; `SameSite=Strict` because both are POSTed by
/// our own page.
fn refresh_cookie(headers: &HeaderMap, value: &str, max_age: i64) -> String {
    let secure = if is_local_host(headers) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "{REFRESH_COOKIE}={value}; Path=/auth; Max-Age={max_age}; HttpOnly;{secure} SameSite=Strict"
    )
}

/// The device code, while a flow is in flight. It is a credential — spend
/// it after approval and you get a token — so it is kept the same way the
/// refresh token is, out of the page's reach. Five minutes, matching the
/// issuer's own lifetime for it.
const DEVICE_COOKIE: &str = "regnmed_device";
const DEVICE_COOKIE_MAX_AGE: i64 = 300;

fn device_cookie(headers: &HeaderMap, value: &str, max_age: i64) -> String {
    let secure = if is_local_host(headers) {
        ""
    } else {
        " Secure;"
    };
    format!(
        "{DEVICE_COOKIE}={value}; Path=/auth; Max-Age={max_age}; HttpOnly;{secure} SameSite=Strict"
    )
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| {
            c.trim()
                .strip_prefix(name)?
                .strip_prefix('=')
                .map(str::to_string)
        })
}

/// Proxies the authorization-code exchange to the IdP's token endpoint.
pub async fn token_exchange(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TokenExchangeRequest>,
) -> Result<Response, ApiError> {
    let client_id = portal_client_id();
    token_endpoint_exchange(
        &headers,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("code", request.code.as_str()),
            ("redirect_uri", request.redirect_uri.as_str()),
            ("code_verifier", request.code_verifier.as_str()),
        ],
    )
    .await
}

/// Spends the refresh cookie, so an hour-old access token does not mean
/// a new login. Nothing is read from the request body: the credential is
/// the cookie, which the page cannot read, forge or send anywhere else.
///
/// 401 means there is no cookie (nobody is logged in here) and 400 means
/// the issuer refused one — the browser retries only the second, because
/// the issuer ROTATES and a parallel tab may have just won the race.
pub async fn token_refresh(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let refresh = cookie_value(&headers, REFRESH_COOKIE)
        .ok_or(ApiError::Unauthorized("ingen økt å fornye"))?;
    let client_id = portal_client_id();
    token_endpoint_exchange(
        &headers,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh.as_str()),
        ],
    )
    .await
}

/// Starts a device flow (RFC 8628) for a screen that cannot reasonably
/// take a password: a car display, a borrowed PC. Answers with the code to
/// show, a QR of the page that approves it, and how long it is good for.
///
/// The device code itself goes into an HttpOnly cookie, so the page shows
/// the user code and holds nothing it could leak.
pub async fn device_start(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let issuer = std::env::var("OIDC_ISSUER")
        .map_err(|_| ApiError::BadRequest("OIDC_ISSUER is not configured".into()))?;
    let client_id = portal_client_id();

    let response = reqwest::Client::new()
        .post(format!(
            "{}/device_authorization",
            issuer.trim_end_matches('/')
        ))
        .form(&[("client_id", client_id.as_str())])
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("IdP unreachable: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        eprintln!("device authorization failed ({status}): {body}");
        return Err(ApiError::BadRequest(
            "utstederen støtter ikke innlogging fra en annen enhet".into(),
        ));
    }

    let mut parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| ApiError::BadRequest("uventet svar fra utsteder".into()))?;
    let object = parsed
        .as_object_mut()
        .ok_or_else(|| ApiError::BadRequest("uventet svar fra utsteder".into()))?;
    let device_code = object
        .remove("device_code")
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| ApiError::BadRequest("utstederen ga ingen enhetskode".into()))?;

    // The QR encodes the URL that approves THIS code, so scanning it lands
    // on the approval page with nothing left to type.
    if let Some(url) = object
        .get("verification_uri_complete")
        .and_then(|v| v.as_str())
    {
        match regnmed_core::qr::svg(url) {
            Ok(svg) => {
                object.insert("qr".into(), serde_json::Value::String(svg));
            }
            // A URL too long to encode is a configuration we would rather
            // name than hide: the code and the address still work.
            Err(err) => eprintln!("no QR for {url}: {err}"),
        }
    }

    let mut response = (
        [(header::CONTENT_TYPE, "application/json")],
        parsed.to_string(),
    )
        .into_response();
    let cookie = device_cookie(&headers, &device_code, DEVICE_COOKIE_MAX_AGE);
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| ApiError::BadRequest("ugyldig kode fra utsteder".into()))?,
    );
    Ok(response)
}

/// Asks whether the sign-in has been approved yet. 202 means keep waiting
/// — the status code carries that, so the page does not parse prose — and
/// a success answers with the access token, exactly like a login.
///
/// NO refresh cookie is set here, and that is the point of the grant: this
/// screen may not be the user's own, so the session dies with the hour.
pub async fn device_poll(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let device_code = cookie_value(&headers, DEVICE_COOKIE)
        .ok_or(ApiError::Unauthorized("ingen pågående innlogging"))?;
    let issuer = std::env::var("OIDC_ISSUER")
        .map_err(|_| ApiError::BadRequest("OIDC_ISSUER is not configured".into()))?;
    let client_id = portal_client_id();

    let response = reqwest::Client::new()
        .post(format!("{}/token", issuer.trim_end_matches('/')))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id.as_str()),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .map_err(|e| ApiError::BadRequest(format!("IdP unreachable: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if status.is_success() {
        // Approved. The device code is spent, so the cookie goes too.
        let mut response = ([(header::CONTENT_TYPE, "application/json")], body).into_response();
        if let Ok(cookie) = device_cookie(&headers, "", 0).parse() {
            response.headers_mut().insert(header::SET_COOKIE, cookie);
        }
        return Ok(response);
    }

    // RFC 8628 §3.5: one error means "not yet", every other one ends the
    // flow. Translating it to a status code is what lets the page poll
    // without reading the issuer's wording.
    let pending = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .is_some_and(|error| error == "authorization_pending" || error == "slow_down");
    if pending {
        return Ok(StatusCode::ACCEPTED.into_response());
    }

    eprintln!("device poll ended ({status}): {body}");
    let mut response = (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        "{\"error\":\"avbrutt\"}",
    )
        .into_response();
    if let Ok(cookie) = device_cookie(&headers, "", 0).parse() {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    Ok(response)
}

/// Drops the refresh cookie. Without this the cookie would survive the
/// logout and the next visit would silently sign the user back in — on a
/// shared machine, as whoever was there before.
pub async fn auth_logout(headers: HeaderMap) -> Response {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, refresh_cookie(&headers, "", 0))],
    )
        .into_response()
}

/// The one rail to the issuer's token endpoint: the browser never talks
/// to the IdP (no CORS), the IdP's error detail stays in our log while
/// the browser gets a plain failure, and the refresh token is SPLIT OFF
/// into a cookie instead of being passed through to the page.
async fn token_endpoint_exchange(
    headers: &HeaderMap,
    form: &[(&str, &str)],
) -> Result<Response, ApiError> {
    let issuer = std::env::var("OIDC_ISSUER")
        .map_err(|_| ApiError::BadRequest("OIDC_ISSUER is not configured".into()))?;
    let token_endpoint = format!("{}/token", issuer.trim_end_matches('/'));

    let response = reqwest::Client::new()
        .post(&token_endpoint)
        .form(form)
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

    let mut parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| ApiError::BadRequest("uventet svar fra utsteder".into()))?;
    let refresh = parsed
        .as_object_mut()
        .and_then(|o| o.remove("refresh_token"))
        .and_then(|v| v.as_str().map(str::to_string));

    let mut response = (
        [(header::CONTENT_TYPE, "application/json")],
        parsed.to_string(),
    )
        .into_response();
    if let Some(refresh) = refresh {
        let cookie = refresh_cookie(headers, &refresh, REFRESH_COOKIE_MAX_AGE);
        response.headers_mut().insert(
            header::SET_COOKIE,
            cookie
                .parse()
                .map_err(|_| ApiError::BadRequest("ugyldig token fra utsteder".into()))?,
        );
    }
    Ok(response)
}
