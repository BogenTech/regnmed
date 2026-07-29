//! One bad request must not take the process down for everybody else.
//!
//! The test needs no database: it exercises the layer `router()` mounts
//! (`regnmed_api::catch_panic_layer`) on a route that panics on purpose.
//! The production code has no such route — hence it is built here.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::{Router, routing::get};
use tower::ServiceExt;

async fn panikker() -> &'static str {
    panic!("bevisst panikk i en handler")
}

async fn frisk() -> &'static str {
    "ok"
}

fn app() -> Router {
    Router::new()
        .route("/panikk", get(panikker))
        .route("/frisk", get(frisk))
        .layer(regnmed_api::catch_panic_layer())
}

#[tokio::test]
async fn a_panic_becomes_a_500_and_the_server_lives_on() {
    // A panic in the handler yields a RESPONSE — not a broken connection.
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/panikk")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("gyldig JSON");
    // The client gets a neutral message: panic text can leak internal data.
    assert_eq!(body["error"], "intern feil");
    assert!(
        !String::from_utf8_lossy(&bytes).contains("bevisst panikk"),
        "panikkteksten skal bli i loggen, ikke i svaret"
    );

    // And what matters most: the next request is served as before.
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/frisk")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// The other half of the guarantee: the process does not die from a panic
/// in a tokio task. `panic=unwind` is what gives us this — if this fails,
/// `panic="abort"` has crept into the profile (Cargo.toml).
#[tokio::test]
async fn a_panic_in_one_task_does_not_kill_the_process() {
    let doemt = tokio::spawn(async { panic!("oppgaven ryker") });
    assert!(doemt.await.is_err(), "oppgaven skal ha panikket");

    // Are we still running? With panic="abort" we would never have got here.
    let levende = tokio::spawn(async { 2 + 2 });
    assert_eq!(levende.await.unwrap(), 4);
}
