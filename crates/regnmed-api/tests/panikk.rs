//! Én dårlig forespørsel skal ikke rive ned prosessen for alle andre.
//!
//! Testen trenger ingen database: den prøver laget `router()` monterer
//! (`regnmed_api::catch_panic_layer`) på en rute som panikker med vilje.
//! Produksjonskoden har ingen slik rute — derfor bygges den her.

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
async fn panikk_blir_500_og_serveren_lever_videre() {
    // En panikk i håndteringen gir et SVAR — ikke en brutt forbindelse.
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
    // Klienten får en nøytral melding: en panikktekst kan røpe interne data.
    assert_eq!(body["error"], "intern feil");
    assert!(
        !String::from_utf8_lossy(&bytes).contains("bevisst panikk"),
        "panikkteksten skal bli i loggen, ikke i svaret"
    );

    // Og det som betyr mest: neste forespørsel betjenes som før.
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

/// Den andre halvdelen av garantien: prosessen dør ikke av en panikk i
/// en tokio-oppgave. Det er `panic=unwind` som gir oss dette — feiler
/// denne, er `panic="abort"` sneket inn i profilen (Cargo.toml).
#[tokio::test]
async fn panikk_i_en_oppgave_dreper_ikke_prosessen() {
    let doemt = tokio::spawn(async { panic!("oppgaven ryker") });
    assert!(doemt.await.is_err(), "oppgaven skal ha panikket");

    // Kjører vi fortsatt? Med panic="abort" hadde vi aldri kommet hit.
    let levende = tokio::spawn(async { 2 + 2 });
    assert_eq!(levende.await.unwrap(), 4);
}
