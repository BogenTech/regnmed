//! End-to-end test of the OIDC relying-party layer: a token signed by a
//! locally generated RSA key (published as a JWKS, exactly as an IdP would)
//! must open /me and resolve engagement-based access; forged, expired and
//! malformed tokens must be rejected. Requires DATABASE_URL (skips
//! otherwise) — `scripts/dev-db.sh` + `regnmed migrate` provides it.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{AUDIENCE, ISSUER, KID, TestIdp, test_state, unique_orgnr};
use jsonwebtoken::{Algorithm, Header, encode};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn get_me(state: &AppState, bearer: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().uri("/me");
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = router(state.clone())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn valid_token_resolves_engagement_access() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Seed: an accountant employed by a firm with an active engagement for
    // a client company, plus a direct admin membership in another company.
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Kontrolldame"), None)
        .await
        .unwrap();

    let client = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Klientfirma AS")
        .await
        .unwrap();
    let own = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Eget Selskap AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, own, person, "admin")
        .await
        .unwrap();

    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Tall & Orden AS", "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, person, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, client, "regnskap")
        .await
        .unwrap();

    let (status, body) = get_me(&state, Some(&idp.token(&sub, "Kari Kontrolldame"))).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["sub"], sub.as_str());

    let companies = body["companies"].as_array().expect("companies array");
    assert_eq!(companies.len(), 2, "body: {body}");

    let via_engagement = companies
        .iter()
        .find(|c| c["name"] == "Klientfirma AS")
        .expect("client company present");
    assert_eq!(via_engagement["access"], "bokforing");
    assert_eq!(via_engagement["via"], "Tall & Orden AS");

    let direct = companies
        .iter()
        .find(|c| c["name"] == "Eget Selskap AS")
        .expect("own company present");
    assert_eq!(direct["access"], "admin");
    assert_eq!(direct["via"], "direkte");
}

#[tokio::test]
async fn forged_and_malformed_tokens_are_rejected() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // No Authorization header.
    let (status, _) = get_me(&state, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Garbage token.
    let (status, _) = get_me(&state, Some("not.a.jwt")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Token signed by a different key (forged), same kid and claims shape.
    let forger = TestIdp::new();
    let (status, _) = get_me(&state, Some(&forger.token("test|forger", "Falsk Person"))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Expired token from the real key.
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_string());
    let expired = encode(
        &header,
        &json!({
            "iss": ISSUER,
            "aud": AUDIENCE,
            "sub": "test|expired",
            "exp": chrono::Utc::now().timestamp() - 3600,
        }),
        &idp.encoding_key,
    )
    .unwrap();
    let (status, _) = get_me(&state, Some(&expired)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Wrong audience.
    let wrong_aud = encode(
        &header,
        &json!({
            "iss": ISSUER,
            "aud": "some-other-service",
            "sub": "test|wrongaud",
            "exp": chrono::Utc::now().timestamp() + 3600,
        }),
        &idp.encoding_key,
    )
    .unwrap();
    let (status, _) = get_me(&state, Some(&wrong_aud)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
