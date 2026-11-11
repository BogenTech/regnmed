//! Sending the a-melding through the API (docs/lonn.md, «Innsendingssporet»),
//! against a mock of Skatteetaten shaped like the real test environment.
//!
//! What must not break: a failed send is retried with the SAME body and
//! idempotency key; a delivered run is not sent a second time unless as a
//! replacement; the feedback is stored; and only LONN_KJOR may send.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use regnmed_api::{AmeldingRail, AppState, router};

#[derive(Default)]
struct Skatteetaten {
    fail_next: bool,
    /// (idempotencyKey, body) per call that reached the API.
    calls: Vec<(String, serde_json::Value)>,
}

async fn mock_skatteetaten() -> (String, Arc<Mutex<Skatteetaten>>) {
    let seen = Arc::new(Mutex::new(Skatteetaten::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let feedback = format!("{base}/tb/v1/forsendelser/t-info");
    let app = Router::new()
        .route(
            "/token",
            post(|| async { Json(serde_json::json!({"access_token": "t", "expires_in": 120})) }),
        )
        .route(
            "/v1/innsending/{period}/{org}",
            post(
                |State(seen): State<Arc<Mutex<Skatteetaten>>>,
                 Path((_period, _org)): Path<(String, String)>,
                 headers: HeaderMap,
                 Json(body): Json<serde_json::Value>| async move {
                    let mut s = seen.lock().unwrap();
                    if std::mem::take(&mut s.fail_next) {
                        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({})));
                    }
                    let melding = body["leveranse"]["meldingsId"].clone();
                    s.calls
                        .push((headers["idempotencyKey"].to_str().unwrap().into(), body));
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "dialogId": "d-1", "forsendelseId": "t-sub", "meldingsId": melding
                        })),
                    )
                },
            ),
        )
        .route(
            "/dp/enduser/dialogs/{id}",
            get(move || {
                let feedback = feedback.clone();
                async move {
                    Json(serde_json::json!({"transmissions": [{
                        "type": "Information", "relatedTransmissionId": "t-sub",
                        "attachments": [{"urls": [{"url": feedback, "consumerType": "Api"}]}]
                    }]}))
                }
            }),
        )
        .route(
            "/tb/v1/forsendelser/t-info",
            get(|State(seen): State<Arc<Mutex<Skatteetaten>>>| async move {
                let melding = seen.lock().unwrap().calls[0].1["leveranse"]["meldingsId"].clone();
                Json(serde_json::json!({"mottak": [{"mottattLeveranse": [
                    {"meldingsId": melding, "mottakstatus": "mottatt", "avvik": []}
                ]}]}))
            }),
        )
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, seen)
}

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let code = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        code,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn an_amelding_is_sent_once_retried_unchanged_and_its_feedback_stored() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let (base, skatteetaten) = mock_skatteetaten().await;
    state.amelding = Some(Arc::new(AmeldingRail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{base}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::amelding::SCOPES.into(),
            },
        ),
        client: regnmed_gov::amelding::AmeldingClient::new(base.clone(), format!("{base}/dp")),
    }));

    // A real company has a MOD11-valid number; the systembruker token needs one.
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Innsendingstest AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("5000", "Lønn"),
        ("5090", "Feriepenger"),
        ("5400", "Arbeidsgiveravgift"),
        ("2600", "Forskuddstrekk"),
        ("2770", "Skyldig arbeidsgiveravgift"),
        ("2930", "Skyldig lønn"),
        ("2940", "Skyldige feriepenger"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let mut tokens = Vec::new();
    for role in ["admin", "revisor"] {
        let sub = format!("{role}|{}", Uuid::new_v4());
        let person = regnmed_db::ensure_person(&state.pool, &sub, Some(role), None)
            .await
            .unwrap();
        regnmed_db::ensure_company_member(&state.pool, company, person, role)
            .await
            .unwrap();
        tokens.push(idp.token(&sub, role));
    }
    let (admin, auditor) = (&tokens[0], &tokens[1]);

    // 40 000 kr at 35 % and sone I: whole kroner throughout.
    let (code, created) = call(
        &state,
        "POST",
        &format!("/companies/{company}/employees"),
        admin,
        r#"{"fodselsnummer":"26829398612","navn":"Kari Utvikler","ansatt_fra":"2026-01-01",
            "manedslonn_ore":4000000,"trekk_type":"prosent","trekk_prosent_bp":3500,
            "virksomhet_orgnr":"974760673","yrkeskode":"2512101","stillingsprosent_bp":10000,
            "timer_per_uke_hundredeler":3750,"arbeidstidsordning":"ikkeSkift",
            "ansettelsesform":"fast","siste_lonnsendring":"2026-01-01"}"#,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{created}");
    let employee: Uuid = created["employee_id"].as_str().unwrap().parse().unwrap();
    let run = regnmed_db::payroll::run_payroll(
        &state.pool,
        company,
        2026,
        3,
        chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap(),
        "I",
        &[regnmed_db::payroll::PayrollEntry {
            employee_id: employee,
            gross_ore: None,
            holiday_pay_ore: 0,
            from_timesheet: false,
        }],
        "Test",
    )
    .await
    .unwrap();
    let submissions = format!(
        "/companies/{company}/payroll/{}/amelding/submissions",
        run.id
    );

    // Only LONN_KJOR sends; the revisor may read the record.
    let (code, _) = call(&state, "POST", &submissions, auditor, "{}").await;
    assert_eq!(code, StatusCode::FORBIDDEN);

    // Skatteetaten is down: refused, recorded, nothing counted.
    skatteetaten.lock().unwrap().fail_next = true;
    let (code, failed) = call(&state, "POST", &submissions, admin, "{}").await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{failed}");
    assert!(
        failed["error"].as_str().unwrap().contains("sendes uendret"),
        "{failed}"
    );
    let (_, listed) = call(&state, "GET", &submissions, auditor, "").await;
    assert_eq!(listed["innsendinger"][0]["status"], "ikke_levert");
    assert!(
        listed["innsendinger"][0]["siste_feil"]
            .as_str()
            .unwrap()
            .contains("503")
    );

    // The retry is the SAME melding: one submission, delivered.
    let (code, sent) = call(&state, "POST", &submissions, admin, "{}").await;
    assert_eq!(code, StatusCode::OK, "{sent}");
    assert_eq!(sent["status"], "levert");
    assert_eq!(sent["id"], listed["innsendinger"][0]["id"]);
    {
        let s = skatteetaten.lock().unwrap();
        assert_eq!(s.calls.len(), 1);
        assert_eq!(s.calls[0].1["leveranse"]["meldingsId"], sent["melding_id"]);
    }

    // A second plain send would ADD to the month: refused.
    let (code, again) = call(&state, "POST", &submissions, admin, "{}").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        again["error"].as_str().unwrap().contains("allerede levert"),
        "{again}"
    );

    // Feedback: fetched, stored, this melding's verdict surfaced.
    let (code, fed) = call(
        &state,
        "POST",
        &format!("{submissions}/{}/feedback", sent["id"].as_str().unwrap()),
        admin,
        "",
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{fed}");
    assert_eq!(fed["status"], "tilbakemelding");
    assert_eq!(fed["tilbakemelding"]["mottakstatus"], "mottatt");
    assert_eq!(fed["tilbakemelding"]["avvik"], serde_json::json!([]));

    // A replacement: a new melding pointing at the first, new key.
    let (code, replaced) = call(&state, "POST", &submissions, admin, r#"{"erstatt":true}"#).await;
    assert_eq!(code, StatusCode::OK, "{replaced}");
    assert_eq!(replaced["erstatter_melding_id"], sent["melding_id"]);
    let s = skatteetaten.lock().unwrap();
    assert_eq!(s.calls.len(), 2);
    assert_ne!(
        s.calls[0].0, s.calls[1].0,
        "a replacement has its own idempotency key"
    );
    assert_eq!(
        s.calls[1].1["leveranse"]["erstatterMeldingsId"],
        sent["melding_id"]
    );
}
