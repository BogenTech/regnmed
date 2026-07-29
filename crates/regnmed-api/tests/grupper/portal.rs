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
    let header = |navn: &str| {
        response
            .headers()
            .get(navn)
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
        let svar = get(&state, uri).await;
        assert_eq!(svar.status, StatusCode::OK, "{uri}");
        assert!(svar.content_type.starts_with("text/html"), "{uri}");
        assert!(svar.body.contains("<div id=\"app\">"), "{uri}");
        index = svar.body;
    }

    // Vite writes content-hashed file names — find them in index.html and
    // check that they are actually served, with a long cache (content
    // behind a hashed address never changes).
    let mut assets = 0;
    for del in index.split('"') {
        if !del.starts_with("/assets/") {
            continue;
        }
        let svar = get(&state, del).await;
        assert_eq!(svar.status, StatusCode::OK, "{del}");
        assert!(svar.cache_control.contains("immutable"), "{del}");
        if del.ends_with(".js") {
            assert!(svar.content_type.starts_with("text/javascript"), "{del}");
        }
        if del.ends_with(".css") {
            assert!(svar.content_type.starts_with("text/css"), "{del}");
        }
        assets += 1;
    }
    assert!(assets >= 2, "index.html må peke på minst JS + CSS: {index}");

    // An unknown asset is an ERROR, not the app again: a 200 with HTML
    // where the JS should be fails silently in the browser.
    let svar = get(&state, "/assets/finnes-ikke.js").await;
    assert_eq!(svar.status, StatusCode::NOT_FOUND);

    // /ny was the portal's address during the migration — old bookmarks
    // and open tabs must land on the portal, not in a 404.
    let svar = get(&state, "/ny").await;
    assert_eq!(svar.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(svar.location, "/");

    // The framework-free portal is gone for good.
    for gammel in ["/app.js", "/app.css", "/theme.js"] {
        assert_eq!(
            get(&state, gammel).await.status,
            StatusCode::NOT_FOUND,
            "{gammel}"
        );
    }
}
