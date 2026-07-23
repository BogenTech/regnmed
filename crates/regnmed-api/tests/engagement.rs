//! The full marketplace loop over the web API: a company admin finds a
//! verified firm in the directory and requests an oppdrag; the
//! accountant sees and accepts it; access flows through the engagement
//! immediately; the company later ends the oppdrag and access is gone —
//! with permission and duplicate checks along the way.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

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
async fn directory_request_accept_work_end() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // Owner (company admin), accountant (member of a verified firm), and
    // the firm itself — created via the verified path so it is listed.
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let accountant_sub = format!("test|{}", Uuid::new_v4());
    let owner = regnmed_db::ensure_person(&state.pool, &owner_sub, Some("Eva Eier"), None)
        .await
        .unwrap();
    let accountant =
        regnmed_db::ensure_person(&state.pool, &accountant_sub, Some("Kari Bokfører"), None)
            .await
            .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Oppdragsklient AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, owner, "admin")
        .await
        .unwrap();
    let firm_name = format!("Direktoratet Regnskap {} AS", &unique_orgnr()[..4]);
    let firm_id = regnmed_db::create_verified_firm(
        &state.pool,
        &unique_orgnr(),
        &firm_name,
        "regnskap",
        "test-verifisering",
        accountant,
    )
    .await
    .unwrap();

    let owner_token = idp.token(&owner_sub, "Eva Eier");
    let accountant_token = idp.token(&accountant_sub, "Kari Bokfører");

    // The directory lists the verified firm.
    let (status, directory) = request(
        &state,
        "GET",
        "/directory/firms?kind=regnskap",
        &owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        directory["firms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == firm_name.as_str()),
        "verified firm is listed"
    );

    // The accountant (not company admin) cannot request on the company's
    // behalf; the owner can.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/engagement-requests"),
        &accountant_token,
        Some(serde_json::json!({ "firm_id": firm_id })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no access at all → 404");

    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/engagement-requests"),
        &owner_token,
        Some(serde_json::json!({ "firm_id": firm_id, "message": "Trenger hjelp fra nyttår" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Duplicate pending request is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/engagement-requests"),
        &owner_token,
        Some(serde_json::json!({ "firm_id": firm_id })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The accountant sees the request; an outsider does not.
    let (status, requests) = request(
        &state,
        "GET",
        &format!("/firms/{firm_id}/requests"),
        &accountant_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pending = requests["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["status"] == "pending" && r["company"] == "Oppdragsklient AS")
        .expect("pending request visible")
        .clone();
    assert_eq!(pending["message"], "Trenger hjelp fra nyttår");

    let (status, _) = request(
        &state,
        "GET",
        &format!("/firms/{firm_id}/requests"),
        &owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-member sees nothing");

    // Accept → engagement opens → the accountant can reach the company.
    let (status, _) = request(
        &state,
        "POST",
        &format!(
            "/firms/{firm_id}/requests/{}/decision",
            pending["request_id"].as_str().unwrap()
        ),
        &accountant_token,
        Some(serde_json::json!({ "accept": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, me) = request(&state, "GET", "/me", &accountant_token, None).await;
    assert_eq!(status, StatusCode::OK);
    let client = me["companies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Oppdragsklient AS")
        .expect("engagement grants access");
    assert_eq!(client["access"], "bokforing");
    assert_eq!(client["via"], firm_name.as_str());

    // The firm's client list shows the oppdrag; the company's view too.
    let (_, clients) = request(
        &state,
        "GET",
        &format!("/firms/{firm_id}/clients"),
        &accountant_token,
        None,
    )
    .await;
    let engagement_id = clients["clients"][0]["engagement_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The company ends the oppdrag; the accountant's access is gone.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/engagements/{engagement_id}/end"),
        &owner_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");

    let (_, me_after) = request(&state, "GET", "/me", &accountant_token, None).await;
    assert!(
        !me_after["companies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "Oppdragsklient AS"),
        "ended engagement revokes access immediately"
    );
}
