//! The portal (Svelte 5 + Vite, #76) is served from the binary at the
//! root — the checked-in ui/portal/dist, same binary, same origin.
//! Requires DATABASE_URL (skips politely otherwise, like the other
//! integration tests).

use crate::common::{TestIdp, test_state};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use regnmed_api::{AppState, router};

struct Svar {
    status: StatusCode,
    content_type: String,
    cache_control: String,
    location: String,
    body: String,
}

async fn get(state: &AppState, uri: &str) -> Svar {
    let response = router(state.clone())
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let content_type = header("content-type");
    let cache_control = header("cache-control");
    let location = header("location");
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    Svar {
        status,
        content_type,
        cache_control,
        location,
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

#[tokio::test]
async fn the_portal_is_served_from_the_binary() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // / and /callback both land in the app: routing happens in the hash,
    // and the callback address must complete the PKCE flow in the app.
    let mut index = String::new();
    for uri in ["/", "/callback"] {
        let response = get(&state, uri).await;
        assert_eq!(response.status, StatusCode::OK, "{uri}");
        assert!(response.content_type.starts_with("text/html"), "{uri}");
        assert!(response.body.contains("<div id=\"app\">"), "{uri}");
        index = response.body;
    }

    // Vite writes content-hashed file names — find them in index.html and
    // check that they are actually served, with a long cache (content
    // behind a hashed address never changes).
    let mut assets = 0;
    for del in index.split('"') {
        if !del.starts_with("/assets/") {
            continue;
        }
        let response = get(&state, del).await;
        assert_eq!(response.status, StatusCode::OK, "{del}");
        assert!(response.cache_control.contains("immutable"), "{del}");
        if del.ends_with(".js") {
            assert!(
                response.content_type.starts_with("text/javascript"),
                "{del}"
            );
        }
        if del.ends_with(".css") {
            assert!(response.content_type.starts_with("text/css"), "{del}");
        }
        assets += 1;
    }
    assert!(assets >= 2, "index.html må peke på minst JS + CSS: {index}");

    // An unknown asset is an ERROR, not the app again: a 200 with HTML
    // where the JS should be fails silently in the browser.
    let response = get(&state, "/assets/finnes-ikke.js").await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);

    // /ny was the portal's address during the migration — old bookmarks
    // and open tabs must land on the portal, not in a 404.
    let response = get(&state, "/ny").await;
    assert_eq!(response.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(response.location, "/");

    // The framework-free portal is gone for good.
    for gammel in ["/app.js", "/app.css", "/theme.js"] {
        assert_eq!(
            get(&state, gammel).await.status,
            StatusCode::NOT_FOUND,
            "{gammel}"
        );
    }
}

/// POSTs to the portal's own auth routes (no bearer — these run before
/// there is a token) and returns the status, the Set-Cookie and the body.
async fn post_auth(
    state: &AppState,
    uri: &str,
    cookie: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, String, String) {
    let mut request = Request::builder().method("POST").uri(uri);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    };
    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    (
        status,
        set_cookie,
        String::from_utf8_lossy(&bytes).to_string(),
    )
}

/// A stand-in for regnid's token endpoint that ROTATES like the real
/// one: a refresh token can be spent exactly once, and the response
/// carries its replacement. Returns the base URL.
async fn start_mock_token_endpoint() -> String {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    // The tokens still alive, and a counter so an access token is never
    // named after the refresh token that bought it — otherwise asserting
    // "the refresh token is not in the body" matches its own substring.
    let live: Arc<Mutex<(usize, HashSet<String>)>> = Arc::new(Mutex::new((0, HashSet::new())));
    let app = axum::Router::new().route(
        "/token",
        axum::routing::post(move |form: String| {
            let live = live.clone();
            async move {
                let field = |name: &str| {
                    form.split('&')
                        .find_map(|p| p.strip_prefix(&format!("{name}="))) // form fields are plain here
                        .unwrap_or_default()
                        .to_string()
                };
                let mint = || {
                    let mut live = live.lock().unwrap();
                    live.0 += 1;
                    let n = live.0;
                    live.1.insert(format!("r{n}"));
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "access_token": format!("at{n}"),
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            "refresh_token": format!("r{n}"),
                            "id_token": "idt",
                        })),
                    )
                };
                match field("grant_type").as_str() {
                    // The login itself: the issuer hands out the first
                    // refresh token here, which is where the cookie is born.
                    "authorization_code" => mint(),
                    "refresh_token" => {
                        let spent = field("refresh_token");
                        if !live.lock().unwrap().1.remove(&spent) {
                            // The detail a reused token produces at the
                            // issuer — it must never reach the browser.
                            return (
                                StatusCode::BAD_REQUEST,
                                axum::Json(serde_json::json!({
                                    "error": "invalid_grant",
                                    "error_description":
                                        format!("refresh token already used: {spent}")
                                })),
                            );
                        }
                        mint()
                    }
                    _ => (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": "unsupported_grant_type"})),
                    ),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// THE security property of the whole flow: the 30-day refresh token
/// never reaches the page. It leaves this server only as an HttpOnly
/// cookie, so an XSS can spend the hour that is already running and
/// cannot mint itself another one.
///
/// And the usability property it buys: an hour-old access token does not
/// mean a new login — /auth/refresh spends the cookie and rotates it.
#[tokio::test]
async fn the_refresh_token_leaves_only_as_an_httponly_cookie() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let mock = start_mock_token_endpoint().await;
    // Process-wide env: nextest gives every test its own process.
    unsafe {
        std::env::set_var("OIDC_ISSUER", &mock);
    }

    // Nothing to renew before anyone has logged in, and saying so with
    // 401 rather than 400 is what stops the browser from retrying.
    let (status, _, _) = post_auth(&state, "/auth/refresh", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The login. The issuer returns a refresh token; the browser must not.
    let (status, cookie, body) = post_auth(
        &state,
        "/auth/token",
        None,
        Some(serde_json::json!({"code": "c", "code_verifier": "v", "redirect_uri": "http://x/callback"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("refresh_token") && !body.contains("r1"),
        "the refresh token reached the page: {body}"
    );
    assert!(body.contains("at1"), "{body}");
    for attribute in ["regnmed_refresh=r1", "HttpOnly", "SameSite=Strict"] {
        assert!(cookie.contains(attribute), "{attribute} missing: {cookie}");
    }
    // Only the two routes that spend it ever see it.
    assert!(cookie.contains("Path=/auth"), "{cookie}");

    // An hour later: the cookie buys a new access token, and ROTATES.
    let (status, cookie2, body) =
        post_auth(&state, "/auth/refresh", Some("regnmed_refresh=r1"), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("at2"), "{body}");
    assert!(!body.contains("refresh_token"), "{body}");
    assert!(
        cookie2.contains("regnmed_refresh=r2"),
        "the replacement must reach the browser, or the session dies an \
         hour later with nothing in any log to say why: {cookie2}"
    );

    // Spending the same token twice is what two tabs racing does. The
    // issuer refuses it, and its own detail stays server-side.
    let (status, _, body) =
        post_auth(&state, "/auth/refresh", Some("regnmed_refresh=r1"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!body.contains("invalid_grant"), "{body}");
    assert!(!body.contains("already used"), "{body}");

    // Logout drops the cookie. Without this the next visit on a shared
    // machine signs the previous user back in.
    let (status, cookie, _) = post_auth(&state, "/auth/logout", None, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
}

/// A stand-in for regnid's device-flow endpoints. The token endpoint says
/// "not yet" once and then approves, which is the sequence the page has to
/// survive without a human watching it.
async fn start_mock_device_endpoints() -> String {
    use std::sync::{Arc, Mutex};

    let polls: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let app = axum::Router::new()
        .route(
            "/device_authorization",
            axum::routing::post(|| async {
                axum::Json(serde_json::json!({
                    "device_code": "dc-secret",
                    "user_code": "WDJB-MJHT",
                    "verification_uri": "http://idp.test/device",
                    "verification_uri_complete": "http://idp.test/device?user_code=WDJB-MJHT",
                    "expires_in": 300,
                    "interval": 5,
                }))
            }),
        )
        .route(
            "/token",
            axum::routing::post(move |form: String| {
                let polls = polls.clone();
                async move {
                    assert!(
                        form.contains("device_code=dc-secret"),
                        "the proxy must spend the cookie, not something else: {form}"
                    );
                    let mut polls = polls.lock().unwrap();
                    *polls += 1;
                    if *polls == 1 {
                        return (
                            StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({"error": "authorization_pending"})),
                        );
                    }
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "access_token": "device-at",
                            "token_type": "Bearer",
                            "expires_in": 3600,
                            // The issuer MUST NOT send one for this grant; if
                            // it ever did, the assertion below catches it.
                        })),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Signing in on a screen that cannot take a password: the code and the QR
/// go to the page, the device code does not, and the session that comes
/// out carries NO refresh cookie — which is the whole reason this grant is
/// safe to use on a car display or a borrowed PC.
#[tokio::test]
async fn signing_in_from_another_device_leaves_no_credential_in_the_page() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let mock = start_mock_device_endpoints().await;
    unsafe {
        std::env::set_var("OIDC_ISSUER", &mock);
    }

    // Nothing in flight: there is no code to spend, and 401 is what stops
    // the page from polling forever.
    let (status, _, _) = post_auth(&state, "/auth/device/poll", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, cookie, body) = post_auth(&state, "/auth/device", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("WDJB-MJHT"),
        "the page needs the code: {body}"
    );
    assert!(
        !body.contains("dc-secret"),
        "the device code reached the page: {body}"
    );
    for attribute in ["regnmed_device=dc-secret", "HttpOnly", "SameSite=Strict"] {
        assert!(cookie.contains(attribute), "{attribute} missing: {cookie}");
    }
    // A QR of the approval URL, so nobody has to type anything.
    assert!(body.contains("<svg"), "no QR in {body}");
    assert!(
        body.contains("viewBox=\\\"0 0 37 37\\\""),
        "the 43-byte URL should land on version 3 (29 + 8 quiet): {body}"
    );

    // Nobody has answered yet. 202, not an error — the page keeps waiting.
    let (status, _, _) = post_auth(
        &state,
        "/auth/device/poll",
        Some("regnmed_device=dc-secret"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // Approved.
    let (status, cookie, body) = post_auth(
        &state,
        "/auth/device/poll",
        Some("regnmed_device=dc-secret"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("device-at"), "{body}");
    // THE property: no 30-day credential is left on a screen the user may
    // not own. The session simply ends when the hour does.
    assert!(
        !cookie.contains(REFRESH_COOKIE_NAME),
        "a refresh cookie on a borrowed device is exactly what this grant avoids: {cookie}"
    );
    // The device code is spent, so its cookie goes with it.
    assert!(cookie.contains("regnmed_device=;"), "{cookie}");
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
}

/// Named here rather than imported: the api crate keeps it private, and a
/// test that spelled it differently from the server would pass while the
/// server leaked.
const REFRESH_COOKIE_NAME: &str = "regnmed_refresh";
