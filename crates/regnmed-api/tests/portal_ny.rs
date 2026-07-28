//! Den nye Svelte-portalen (#76) serveres under /ny fra den innsjekkede
//! ui/portal/dist — samme binær, samme origin. Requires DATABASE_URL
//! (skips politely otherwise, like the other integration tests).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state};
use tower::ServiceExt;

use regnmed_api::{AppState, router};

async fn get(state: &AppState, uri: &str) -> (StatusCode, String, String, String) {
    let response = router(state.clone())
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        content_type,
        cache_control,
        String::from_utf8_lossy(&bytes).to_string(),
    )
}

#[tokio::test]
async fn ny_portal_serveres_fra_binaren() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // /ny, /ny/ og /ny/callback lander alle i appen: rutingen skjer i
    // hashen, og callback-adressen må fullføre PKCE-flyten i appen.
    let mut index = String::new();
    for uri in ["/ny", "/ny/", "/ny/callback"] {
        let (status, content_type, _, body) = get(&state, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(content_type.starts_with("text/html"), "{uri}");
        assert!(body.contains("<div id=\"app\">"), "{uri}");
        index = body;
    }

    // Vite skriver innholdshashede filnavn — finn dem i index.html og
    // sjekk at de faktisk serveres, med lang cache (innholdet bak en
    // hashet adresse endrer seg aldri).
    let mut assets = 0;
    for del in index.split('"') {
        if let Some(path) = del.strip_prefix("/ny/") {
            if !path.starts_with("assets/") {
                continue;
            }
            let (status, content_type, cache_control, _) =
                get(&state, &format!("/ny/{path}")).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(cache_control.contains("immutable"), "{path}");
            if path.ends_with(".js") {
                assert!(content_type.starts_with("text/javascript"), "{path}");
            }
            if path.ends_with(".css") {
                assert!(content_type.starts_with("text/css"), "{path}");
            }
            assets += 1;
        }
    }
    assert!(assets >= 2, "index.html må peke på minst JS + CSS: {index}");

    // Ukjent sti under /ny er SPA-fallback til appen, aldri 404.
    let (status, content_type, _, body) = get(&state, "/ny/finnes-ikke").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains("<div id=\"app\">"));

    // Gamle portalen står urørt på roten til flippen.
    let (status, _, _, body) = get(&state, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("/app.js"));
}
