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

/// POSTs JSON to the portal's own auth routes (no bearer — these run
/// before there is a token).
async fn post_json(state: &AppState, uri: &str, body: serde_json::Value) -> Svar {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    Svar {
        status,
        content_type: String::new(),
        cache_control: String::new(),
        location: String::new(),
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

/// A stand-in for regnid's token endpoint that ROTATES like the real one:
/// a refresh token can be spent exactly once, and the response carries
/// its replacement. Returns the base URL.
async fn start_mock_token_endpoint() -> String {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    // The tokens still alive. Spending one consumes it and mints the next.
    let live: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::from(["r1".to_string()])));
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
                if field("grant_type") != "refresh_token" || field("client_id").is_empty() {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": "unsupported_grant_type"})),
                    );
                }
                let spent = field("refresh_token");
                if !live.lock().unwrap().remove(&spent) {
                    // The detail a reused token produces at the issuer —
                    // it must never reach the browser.
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": "invalid_grant",
                            "error_description": format!("refresh token already used: {spent}")
                        })),
                    );
                }
                let next = format!("{spent}-neste");
                live.lock().unwrap().insert(next.clone());
                (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "access_token": format!("at-for-{next}"),
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": next,
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

/// An hour-old access token must not mean a new login: /auth/refresh
/// spends the refresh token at the issuer and hands the whole answer
/// back, rotation included.
#[tokio::test]
async fn a_refresh_exchange_passes_the_rotation_through() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let mock = start_mock_token_endpoint().await;
    // Process-wide env: nextest gives every test its own process.
    unsafe {
        std::env::set_var("OIDC_ISSUER", &mock);
    }

    let response = post_json(
        &state,
        "/auth/refresh",
        serde_json::json!({"refresh_token": "r1"}),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(body["access_token"], "at-for-r1-neste");
    // The REPLACEMENT must reach the client. Dropping it here would log
    // the user out an hour later, with nothing in any log to say why.
    assert_eq!(body["refresh_token"], "r1-neste");

    // Spending the same token twice is what a missing single-flight in
    // the browser does (ui/portal/src/lib/auth.svelte.js). The issuer
    // refuses it, and the browser gets a plain failure: the issuer's own
    // detail stays server-side.
    let response = post_json(
        &state,
        "/auth/refresh",
        serde_json::json!({"refresh_token": "r1"}),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(
        !response.body.contains("invalid_grant"),
        "{}",
        response.body
    );
    assert!(!response.body.contains("already used"), "{}", response.body);

    // The rotated token works, so the session continues from here.
    let response = post_json(
        &state,
        "/auth/refresh",
        serde_json::json!({"refresh_token": "r1-neste"}),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    // The authorization-code route still answers on its own grant: the
    // two share one rail to the issuer, and the mock only knows refresh.
    let response = post_json(
        &state,
        "/auth/token",
        serde_json::json!({"code": "c", "code_verifier": "v", "redirect_uri": "http://x/callback"}),
    )
    .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}
