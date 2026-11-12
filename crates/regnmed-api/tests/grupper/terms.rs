//! Customer terms (#185, docs/kundevilkar.md §1.4): the text is public,
//! only a company admin can accept it, an acceptance names the exact text,
//! and a stale version is refused. The submission gate itself is tested
//! where the submissions are (share_register.rs, amelding.rs).
//!
//! Requires DATABASE_URL — skips politely otherwise.

use crate::common::{TestIdp, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    token: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(v) => {
            request = request.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn only_an_admin_accepts_and_only_the_text_they_saw() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Vilkår AS")
        .await
        .unwrap();
    let mut tokens = Vec::new();
    for role in ["admin", "bokforing"] {
        let sub = format!("{role}|{}", Uuid::new_v4());
        let person = regnmed_db::ensure_person(&state.pool, &sub, Some(role), None)
            .await
            .unwrap();
        regnmed_db::ensure_company_member(&state.pool, company, person, role)
            .await
            .unwrap();
        tokens.push(idp.token(&sub, role));
    }
    let (admin, bookkeeper) = (tokens[0].as_str(), tokens[1].as_str());
    let base = format!("/companies/{company}/terms");

    // The text is public and carries its own version.
    let (status, terms) = call(&state, None, "GET", "/terms", None).await;
    assert_eq!(status, StatusCode::OK);
    let version = terms["versjon"].as_str().unwrap().to_string();
    assert_eq!(version.len(), 64);
    assert!(terms["tekst"].as_str().unwrap().contains("BogenTech AS"));

    let (_, status_before) = call(&state, Some(bookkeeper), "GET", &base, None).await;
    assert_eq!(status_before["godtatt"], false);

    // A bookkeeper cannot bind the company.
    let accept = format!("{base}/accept");
    let (status, _) = call(
        &state,
        Some(bookkeeper),
        "POST",
        &accept,
        Some(json!({ "versjon": version })),
    )
    .await;
    assert!(status.is_client_error(), "{status}");
    assert!(
        regnmed_db::terms::acceptance(&state.pool, company, &version)
            .await
            .unwrap()
            .is_none()
    );

    // A text the admin did not see is refused.
    let (status, body) = call(
        &state,
        Some(admin),
        "POST",
        &accept,
        Some(json!({ "versjon": "0".repeat(64) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("endret"), "{body}");

    let (status, body) = call(
        &state,
        Some(admin),
        "POST",
        &accept,
        Some(json!({ "versjon": version })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["godtatt"], true);
    assert_eq!(body["godtatt_av"], "admin");
    let first = body["godtatt_tidspunkt"].clone();

    // Accepting again keeps the first acceptance.
    let (_, again) = call(
        &state,
        Some(admin),
        "POST",
        &accept,
        Some(json!({ "versjon": version })),
    )
    .await;
    assert_eq!(again["godtatt_tidspunkt"], first);

    // The acceptance is evidence: the database refuses to change it.
    let changed =
        sqlx::query("update terms_acceptance set accepted_by = 'x' where company_id = $1")
            .bind(company)
            .execute(&state.pool)
            .await;
    assert!(changed.is_err());
}
