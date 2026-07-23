//! End-to-end marketplace onboarding against mocked registries: a local
//! server plays Enhetsregisteret and Finanstilsynets register
//! (BRREG_API_URL / FINANSTILSYNET_API_URL point at it), so the whole
//! flow — preview, company onboarding with seeded kontoplan, firm
//! creation gated on autorisasjon — runs without the internet.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

/// Valid orgnrs for the mock registry (checksum must pass regardless).
const COMPANY_ORGNR: &str = "923609016"; // in BRREG, no autorisasjon
const FIRM_ORGNR: &str = "974760673"; // in BRREG, autorisert regnskap
const DELETED_ORGNR: &str = "971524960"; // slettet in BRREG

/// One mock serving both registries' URL shapes.
async fn start_mock_registries() -> String {
    use axum::routing::get;
    let app = axum::Router::new()
        .route(
            "/enheter/{orgnr}",
            get(|axum::extract::Path(orgnr): axum::extract::Path<String>| async move {
                let body = match orgnr.as_str() {
                    COMPANY_ORGNR => serde_json::json!({
                        "organisasjonsnummer": COMPANY_ORGNR,
                        "navn": "TESTSELSKAP AS",
                        "organisasjonsform": {"kode": "AS", "beskrivelse": "Aksjeselskap"},
                        "naeringskode1": {"kode": "62.010", "beskrivelse": "Programmeringstjenester"},
                        "registrertIMvaregisteret": true,
                        "konkurs": false
                    }),
                    FIRM_ORGNR => serde_json::json!({
                        "organisasjonsnummer": FIRM_ORGNR,
                        "navn": "TALL & ORDEN REGNSKAP AS",
                        "organisasjonsform": {"kode": "AS", "beskrivelse": "Aksjeselskap"},
                        "registrertIMvaregisteret": true,
                        "konkurs": false
                    }),
                    DELETED_ORGNR => serde_json::json!({
                        "organisasjonsnummer": DELETED_ORGNR,
                        "navn": "SLETTET AS",
                        "slettedato": "2020-01-01"
                    }),
                    _ => return (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({}))),
                };
                (StatusCode::OK, axum::Json(body))
            }),
        )
        .route(
            "/virksomheter/{orgnr}",
            get(|axum::extract::Path(orgnr): axum::extract::Path<String>| async move {
                if orgnr == FIRM_ORGNR {
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "autorisasjoner": [{"kode": "Regnskapsforerselskap", "aktiv": true}]
                        })),
                    )
                } else {
                    (StatusCode::NOT_FOUND, axum::Json(serde_json::json!({})))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(
            builder
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn onboarding_from_registries_end_to_end() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let mock = start_mock_registries().await;
    // Process-wide env: this is the only test in the binary, so no race.
    unsafe {
        std::env::set_var("BRREG_API_URL", &mock);
        std::env::set_var("FINANSTILSYNET_API_URL", &mock);
    }

    let sub = format!("test|{}", Uuid::new_v4());
    let token = idp.token(&sub, "Grunnlegger Gro");

    // Clean slate: earlier runs may have onboarded these orgnrs.
    for orgnr in [COMPANY_ORGNR, FIRM_ORGNR] {
        sqlx::query("update company set orgnr = $2 where orgnr = $1")
            .bind(orgnr)
            .bind(unique_orgnr())
            .execute(&state.pool)
            .await
            .unwrap();
        sqlx::query("update firm set orgnr = $2 where orgnr = $1")
            .bind(orgnr)
            .bind(unique_orgnr())
            .execute(&state.pool)
            .await
            .unwrap();
    }

    // Preview shows registry facts and autorisasjon flags.
    let (status, preview) = request(
        &state,
        "GET",
        &format!("/registry/enheter/{COMPANY_ORGNR}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {preview}");
    assert_eq!(preview["navn"], "TESTSELSKAP AS");
    assert_eq!(preview["autorisasjon"]["regnskap"], false);

    // Invalid checksum is rejected before any lookup.
    let (status, _) = request(&state, "GET", "/registry/enheter/923609017", &token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Onboard: company created with registry name, creator is admin,
    // kontoplan seeded with reskontro flags.
    let (status, onboarded) = request(
        &state,
        "POST",
        "/companies",
        &token,
        Some(serde_json::json!({ "orgnr": COMPANY_ORGNR })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {onboarded}");
    assert_eq!(onboarded["navn"], "TESTSELSKAP AS");
    assert_eq!(onboarded["seeded_accounts"], 10);
    let company_id = onboarded["company_id"].as_str().unwrap();

    let (status, me) = request(&state, "GET", "/me", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    let mine = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["company_id"] == company_id)
        .expect("creator has access");
    assert_eq!(mine["access"], "admin");

    let kunde_flag: Option<String> = sqlx::query_scalar(
        "select reskontro_kind from account
         where company_id = $1::uuid and number = '1500'",
    )
    .bind(company_id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(kunde_flag.as_deref(), Some("kunde"));

    // Double onboarding and deleted enheter are rejected.
    let (status, _) = request(
        &state,
        "POST",
        "/companies",
        &token,
        Some(serde_json::json!({ "orgnr": COMPANY_ORGNR })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already onboarded");
    let (status, _) = request(
        &state,
        "POST",
        "/companies",
        &token,
        Some(serde_json::json!({ "orgnr": DELETED_ORGNR })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "slettet enhet");

    // Firm creation is gated on Finanstilsynet: the company orgnr (no
    // autorisasjon) is refused; the firm orgnr passes and is recorded.
    let (status, _) = request(
        &state,
        "POST",
        "/firms",
        &token,
        Some(serde_json::json!({ "orgnr": COMPANY_ORGNR, "kind": "regnskap" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "no autorisasjon");

    let (status, firm) = request(
        &state,
        "POST",
        "/firms",
        &token,
        Some(serde_json::json!({ "orgnr": FIRM_ORGNR, "kind": "regnskap" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {firm}");
    assert_eq!(firm["navn"], "TALL & ORDEN REGNSKAP AS");
    let verified_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("select autorisasjon_verified_at from firm where orgnr = $1")
            .bind(FIRM_ORGNR)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert!(verified_at.is_some(), "verification moment recorded");
}
