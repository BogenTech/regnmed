//! A company approves regnmed in Altinn (docs/gov.md): requested once,
//! checked afterwards, and never without the customer terms or admin right.
//! Requires DATABASE_URL; skips otherwise.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, test_state, unique_orgnumber};
use regnmed_api::{AppState, router};

struct Filed {
    id: String,
    status: String,
    body: Value,
}

/// Behaves like Altinn's controller: one request per default externalRef
/// until it times out or is deleted.
#[derive(Default)]
struct Altinn {
    requests: Vec<Filed>,
    created: usize,
    refuse_with: Option<&'static str>,
}

impl Altinn {
    fn set_status(&mut self, status: &str) {
        self.requests.last_mut().unwrap().status = status.into();
    }
}

type Mock = Arc<Mutex<Altinn>>;

fn answer(request: &Filed) -> Value {
    json!({
        "id": request.id,
        "status": request.status,
        "confirmUrl": format!("https://authn.ui.tt02.altinn.no/confirm?id={}", request.id),
    })
}

async fn mock_altinn() -> (String, Mock) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Mock::default();
    let path = "/authentication/api/v1/systemuser/request/vendor";
    let app = Router::new()
        .route(
            "/token",
            post(|| async { Json(json!({"access_token": "t", "expires_in": 120})) }),
        )
        .route(
            path,
            post(
                |State(m): State<Mock>, Json(body): Json<Value>| async move {
                    let mut m = m.lock().unwrap();
                    if let Some(code) = m.refuse_with {
                        return (StatusCode::BAD_REQUEST, Json(json!({ "code": code })));
                    }
                    assert!(body.get("externalRef").is_none(), "tokens need the default");
                    if let Some(open) = m.requests.iter().find(|r| r.status != "Timedout") {
                        return (StatusCode::OK, Json(answer(open)));
                    }
                    m.created += 1;
                    let filed = Filed {
                        id: Uuid::new_v4().to_string(),
                        status: "New".into(),
                        body,
                    };
                    let reply = answer(&filed);
                    m.requests.push(filed);
                    (StatusCode::CREATED, Json(reply))
                },
            ),
        )
        .route(
            &format!("{path}/{{id}}"),
            get(|State(m): State<Mock>, Path(id): Path<String>| async move {
                let m = m.lock().unwrap();
                match m.requests.iter().find(|r| r.id == id) {
                    Some(r) => (StatusCode::OK, Json(answer(r))),
                    None => (StatusCode::NOT_FOUND, Json(json!({ "code": "AUTH-00010" }))),
                }
            })
            .delete(|State(m): State<Mock>, Path(id): Path<String>| async move {
                m.lock().unwrap().requests.retain(|r| r.id != id);
                StatusCode::ACCEPTED
            }),
        )
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, mock)
}

async fn call(
    state: &AppState,
    token: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn rail(idp: &TestIdp, base: &str) -> Arc<regnmed_api::SystemUserRail> {
    Arc::new(regnmed_api::SystemUserRail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{base}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::altinn_system_user::SCOPES.into(),
            },
        ),
        client: regnmed_gov::altinn_system_user::SystemUserClient::new(base, "935115086_regnmed"),
    })
}

async fn company_with(state: &AppState, idp: &TestIdp, role: &str) -> (Uuid, String) {
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Altinn AS")
        .await
        .unwrap();
    let sub = format!("user|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Bruker"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, role)
        .await
        .unwrap();
    (company, idp.token(&sub, "Bruker"))
}

async fn accept_terms(state: &AppState, token: &str, company: Uuid) {
    let (status, body) = call(
        state,
        token,
        "POST",
        &format!("/companies/{company}/terms/accept"),
        json!({ "versjon": *regnmed_api::terms::VERSION }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_company_requests_approval_once_and_sees_it_complete() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    state.portal_base = Some("https://portal.example/".into());
    let (company, token) = company_with(&state, &idp, "admin").await;
    let uri = format!("/companies/{company}/altinn/system-user");

    let (status, body) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("kundevilkår"),
        "{body}"
    );
    assert_eq!(altinn.lock().unwrap().created, 0);

    accept_terms(&state, &token, company).await;
    let (status, body) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["godkjent"], false);
    assert!(
        body["godkjenningslenke"]
            .as_str()
            .unwrap()
            .contains("altinn.no")
    );
    {
        let m = altinn.lock().unwrap();
        assert_eq!(m.requests[0].body["redirectUrl"], "https://portal.example/");
        assert_eq!(m.requests[0].body["rights"].as_array().unwrap().len(), 3);
    }

    // Checking again does not open a second request.
    let (_, body) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(body["status"], "New");
    assert!(body["godkjenningslenke"].is_string());
    assert_eq!(altinn.lock().unwrap().created, 1);

    altinn.lock().unwrap().set_status("Accepted");
    let (_, body) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(body["godkjent"], true);
    assert!(body["godkjenningslenke"].is_null());
    let (_, read) = call(&state, &token, "GET", &uri, Value::Null).await;
    assert_eq!(read["godkjent"], true);
    assert_eq!(altinn.lock().unwrap().created, 1);

    // The customer removes the system user in Altinn, which removes the request.
    altinn.lock().unwrap().requests.clear();
    let (_, revoked) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(revoked["godkjent"], false);
    assert!(revoked["godkjenningslenke"].is_string());
    assert_eq!(altinn.lock().unwrap().created, 2);
}

#[tokio::test]
async fn a_revoked_approval_is_shown_without_requesting_a_new_one() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    let check = format!("{uri}/check");
    call(&state, &token, "POST", &uri, json!({})).await;
    altinn.lock().unwrap().set_status("Accepted");

    let (status, approved) = call(&state, &token, "POST", &check, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["godkjent"], true);

    altinn.lock().unwrap().requests.clear();
    let (_, revoked) = call(&state, &token, "POST", &check, json!({})).await;
    assert_eq!(revoked["status"], "Revoked");
    assert_eq!(revoked["godkjent"], false);
    assert!(revoked["godkjenningslenke"].is_null());
    let (_, stored) = call(&state, &token, "GET", &uri, Value::Null).await;
    assert_eq!(stored["status"], "Revoked");
    let (_, unchanged) = call(&state, &token, "POST", &check, json!({})).await;
    assert_eq!(unchanged["status"], "Revoked");
    assert_eq!(altinn.lock().unwrap().created, 1);

    let (_, again) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(again["status"], "New");
    assert!(again["godkjenningslenke"].is_string());
    assert_eq!(altinn.lock().unwrap().created, 2);
}

#[tokio::test]
async fn a_stray_not_found_does_not_leave_an_approval_revoked() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    let check = format!("{uri}/check");
    call(&state, &token, "POST", &uri, json!({})).await;
    altinn.lock().unwrap().set_status("Accepted");
    call(&state, &token, "POST", &check, json!({})).await;

    let filed = std::mem::take(&mut altinn.lock().unwrap().requests);
    let (_, gone) = call(&state, &token, "POST", &check, json!({})).await;
    assert_eq!(gone["status"], "Revoked");

    altinn.lock().unwrap().requests = filed;
    let (_, back) = call(&state, &token, "POST", &check, json!({})).await;
    assert_eq!(back["godkjent"], true);
}

#[tokio::test]
async fn an_approval_without_a_request_is_not_called_revoked() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    altinn.lock().unwrap().refuse_with = Some("AUTH-00006");
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    call(&state, &token, "POST", &uri, json!({})).await;

    let (_, checked) = call(&state, &token, "POST", &format!("{uri}/check"), json!({})).await;
    assert_eq!(checked["godkjent"], true);
    assert_eq!(checked["kontrollerbar"], false);
}

#[tokio::test]
async fn an_approval_made_after_a_rejection_does_not_inherit_its_request() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    call(&state, &token, "POST", &uri, json!({})).await;
    altinn.lock().unwrap().set_status("Rejected");
    call(&state, &token, "POST", &uri, json!({})).await;

    // The customer creates the system user directly in Altinn.
    altinn.lock().unwrap().refuse_with = Some("AUTH-00004");
    let (_, approved) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(approved["godkjent"], true);
    assert!(altinn.lock().unwrap().requests.is_empty());

    let (_, checked) = call(&state, &token, "POST", &format!("{uri}/check"), json!({})).await;
    assert_eq!(checked["status"], "Accepted");
}

#[tokio::test]
async fn a_rejected_request_can_be_sent_again() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    call(&state, &token, "POST", &uri, json!({})).await;
    altinn.lock().unwrap().set_status("Rejected");
    let (_, rejected) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(rejected["status"], "Rejected");
    assert!(rejected["godkjenningslenke"].is_null());

    let (_, again) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(again["status"], "New");
    assert!(again["godkjenningslenke"].is_string());
    let m = altinn.lock().unwrap();
    assert_eq!((m.created, m.requests.len()), (2, 1));
}

#[tokio::test]
async fn a_request_altinn_no_longer_has_can_be_sent_again() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    call(&state, &token, "POST", &uri, json!({})).await;
    altinn.lock().unwrap().requests.clear();

    let (status, expired) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{expired}");
    assert_eq!(expired["status"], "Timedout");
    assert!(expired["godkjenningslenke"].is_null());

    let (_, again) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(again["status"], "New");
    assert_eq!(altinn.lock().unwrap().created, 2);
}

#[tokio::test]
async fn an_existing_approval_in_altinn_is_recognised() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    altinn.lock().unwrap().refuse_with = Some("AUTH-00006");
    state.system_user = Some(rail(&idp, &base));
    let (company, token) = company_with(&state, &idp, "admin").await;
    accept_terms(&state, &token, company).await;
    let uri = format!("/companies/{company}/altinn/system-user");
    let (status, body) = call(&state, &token, "POST", &uri, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["godkjent"], true);
}

#[tokio::test]
async fn only_a_company_admin_can_connect_altinn() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, altinn) = mock_altinn().await;
    state.system_user = Some(rail(&idp, &base));
    let (company, reader) = company_with(&state, &idp, "bokforing").await;
    let (other, _) = company_with(&state, &idp, "admin").await;
    let uri = format!("/companies/{company}/altinn/system-user");
    let (status, _) = call(&state, &reader, "POST", &uri, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&state, &reader, "POST", &format!("{uri}/check"), json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let foreign = format!("/companies/{other}/altinn/system-user");
    let (status, _) = call(&state, &reader, "GET", &foreign, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(altinn.lock().unwrap().created, 0);
}
