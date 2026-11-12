//! Skattekort from Skatteetaten (docs/lonn.md, #46): ordered for every
//! employee of the year, the answer stored verbatim, and only what payroll
//! computes correctly applied -- the rest kept with the reason. The answer
//! is the one the TEST environment really sent (docs/lonn/).
//!
//! Requires DATABASE_URL — skips politely otherwise.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use regnmed_api::{AppState, router};

#[derive(Default)]
struct Skatteetaten {
    orders: Vec<Value>,
    answer_calls: usize,
    /// Real references are unique; the database holds them unique too.
    reference: String,
    /// Refuse the next order the way Skatteetaten does (FOR_001).
    refuse_next: bool,
}

type Mock = Arc<Mutex<Skatteetaten>>;

async fn mock_skatteetaten() -> (String, Mock) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Mock::default();
    mock.lock().unwrap().reference = format!("BR{}", Uuid::new_v4().as_u128() % 1_000_000_000);
    let app = Router::new()
        .route(
            "/token",
            post(|| async { Json(json!({"access_token": "t", "expires_in": 120})) }),
        )
        .route(
            "/bestillSkattekort",
            post(
                |State(m): State<Mock>, Json(body): Json<Value>| async move {
                    let mut m = m.lock().unwrap();
                    if std::mem::take(&mut m.refuse_next) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"feilkode": "FOR_001", "feilmelding": "validerer ikke"})),
                        );
                    }
                    m.orders.push(body);
                    (
                        StatusCode::OK,
                        Json(json!({"bestillingsreferanse": m.reference,
                                    "dialogreferanse": "d-1"})),
                    )
                },
            ),
        )
        .route(
            "/skattekortTilArbeidsgiver/svar/{reference}",
            get(
                |State(m): State<Mock>, Path(reference): Path<String>| async move {
                    let mut m = m.lock().unwrap();
                    assert_eq!(reference, m.reference);
                    m.answer_calls += 1;
                    // Not ready on the first poll.
                    if m.answer_calls == 1 {
                        return (StatusCode::NO_CONTENT, String::new());
                    }
                    (
                        StatusCode::OK,
                        include_str!("../../../../docs/lonn/skattekort-svar-test-2026.json")
                            .to_string(),
                    )
                },
            ),
        )
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, mock)
}

async fn call(state: &AppState, token: &str, method: &str, uri: &str) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::empty())
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

async fn post_json(state: &AppState, token: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router(state.clone())
        .oneshot(
            Request::post(uri)
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

#[tokio::test]
async fn skattekort_are_fetched_stored_and_applied_only_where_payroll_is_right() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Skattekort AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    let sub = format!("admin|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Lønnsansvarlig"), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Lønnsansvarlig");
    let base = format!("/companies/{company}");
    // The current year only: next year's card would replace the
    // withholding this year's runs use.
    let year = chrono::Datelike::year(&chrono::Utc::now());
    let order = format!("{base}/tax-cards?year={year}");

    // Four of Skatteetaten's test persons, all on 35 % to begin with; one
    // left the year before and gets no card.
    let mut ids = std::collections::HashMap::new();
    for (nin, name, from, to) in [
        ("15929998666", "Prosent", "2026-01-01", None),
        ("20857597940", "Tabell", "2026-01-01", None),
        ("05850298728", "Kildeskatt", "2026-03-01", None),
        ("19910298057", "Uten kort", "2026-01-01", None),
        ("26829398612", "Sluttet", "2024-01-01", Some("2025-06-30")),
    ] {
        let (status, body) = post_json(
            &state,
            &token,
            &format!("{base}/employees"),
            json!({"fodselsnummer": nin, "navn": name, "ansatt_fra": from,
                   "manedslonn_ore": 4_000_000, "trekk_type": "prosent",
                   "trekk_prosent_bp": 3500}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let id: Uuid = body["employee_id"].as_str().unwrap().parse().unwrap();
        if let Some(to) = to {
            sqlx::query("update employee set ansatt_til = $2::date where id = $1")
                .bind(id)
                .bind(to)
                .execute(&state.pool)
                .await
                .unwrap();
        }
        ids.insert(nin, id);
    }

    // No rail: says so.
    let (status, body) = call(&state, &token, "POST", &order).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("ikke satt opp"),
        "{body}"
    );

    let (mock, skatteetaten) = mock_skatteetaten().await;
    state.tax_card = Some(Arc::new(regnmed_api::TaxCardRail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{mock}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::tax_card::SCOPES.into(),
            },
        ),
        client: regnmed_gov::tax_card::TaxCardClient::new(mock),
    }));

    // A call to Skatteetaten on the company's behalf: terms first.
    let (status, body) = call(&state, &token, "POST", &order).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("kundevilkårene"),
        "{body}"
    );
    assert!(skatteetaten.lock().unwrap().orders.is_empty());
    let (status, body) = post_json(
        &state,
        &token,
        &format!("{base}/terms/accept"),
        json!({ "versjon": *regnmed_api::terms::VERSION }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/tax-cards?year={}", year + 1),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("inneværende år"),
        "{body}"
    );

    // Refused by Skatteetaten: still in the §7.1 register, because the
    // request is written before the call.
    skatteetaten.lock().unwrap().refuse_next = true;
    let (status, body) = call(&state, &token, "POST", &order).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("FOR_001"),
        "{body}"
    );
    assert!(skatteetaten.lock().unwrap().orders.is_empty());

    let (status, body) = call(&state, &token, "POST", &order).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["besvart"], true, "answered on the second poll: {body}");
    assert_eq!(body["antall_ansatte"], 4);

    // What was ordered: this year's employees, the company, a contact.
    let sent = skatteetaten.lock().unwrap().orders[0].clone();
    assert_eq!(sent["inntektsaar"], year);
    assert_eq!(sent["bestillingstype"], "HENT_ALLE_OPPGITTE");
    let employer = &sent["forespoerselOmSkattekortTilArbeidsgiver"]["arbeidsgiver"][0];
    assert_eq!(
        employer["arbeidsgiveridentifikator"]["organisasjonsnummer"],
        orgnumber
    );
    let mut nins: Vec<_> = employer["arbeidstakeridentifikator"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    nins.sort();
    assert_eq!(
        nins,
        ["05850298728", "15929998666", "19910298057", "20857597940"]
    );
    assert!(
        sent["kontaktinformasjon"]["epostadresse"]
            .as_str()
            .unwrap()
            .contains('@')
    );

    let card = |name: &str| {
        body["skattekort"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["navn"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(card("Prosent")["satt"], "prosent");
    assert_eq!(card("Prosent")["trekk_prosent_bp"], 3800);
    assert_eq!(card("Prosent")["skattekort_id"], 326999);
    assert_eq!(card("Tabell")["satt"], "tabell");
    assert_eq!(card("Tabell")["trekk_tabell"], 9040);
    assert!(card("Kildeskatt")["satt"].is_null());
    assert!(
        card("Kildeskatt")["merknad"]
            .as_str()
            .unwrap()
            .contains("12 måneder")
    );
    assert!(
        card("Uten kort")["merknad"]
            .as_str()
            .unwrap()
            .contains("50 %")
    );

    // Applied on the employee in the same transaction; kept ones untouched.
    let trekk = |id: Uuid| {
        let pool = state.pool.clone();
        async move {
            let r = sqlx::query(
                "select trekk_type, trekk_prosent_bp, trekk_tabell from employee where id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            (
                r.get::<String, _>("trekk_type"),
                r.get::<Option<i32>, _>("trekk_prosent_bp"),
                r.get::<Option<i32>, _>("trekk_tabell"),
            )
        }
    };
    assert_eq!(
        trekk(ids["15929998666"]).await,
        ("prosent".into(), Some(3800), None)
    );
    assert_eq!(trekk(ids["20857597940"]).await.2, Some(9040));
    assert_eq!(
        trekk(ids["05850298728"]).await,
        ("prosent".into(), Some(3500), None)
    );
    assert_eq!(
        trekk(ids["19910298057"]).await,
        ("prosent".into(), Some(3500), None)
    );

    // Asking for the answer again fetches nothing and changes nothing.
    let order_id = body["id"].as_str().unwrap();
    let (status, again) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/tax-cards/{order_id}/answer"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["skattekort"], body["skattekort"]);
    assert_eq!(skatteetaten.lock().unwrap().answer_calls, 2);

    let (_, listed) = call(&state, &token, "GET", &format!("{base}/tax-cards")).await;
    assert_eq!(listed["henting_mulig"], true);
    assert_eq!(listed["bestillinger"].as_array().unwrap().len(), 1);

    // In the terms §7.1 register: the accepted order with its receipt and
    // answer, and the refused one -- that it failed, not Skatteetaten's text.
    let (_, register) = call(&state, &token, "GET", &format!("{base}/authority-calls")).await;
    let register = register.as_array().unwrap().clone();
    assert_eq!(register.len(), 2, "{register:?}");
    assert!(register.iter().all(|e| e["skjema"] == "skattekort"
        && e["gjelder"].as_str().and_then(|g| g.parse().ok()) == Some(year)));
    let kinds = |e: &Value| -> Vec<String> {
        e["hendelser"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["hendelse"].as_str().unwrap().to_string())
            .collect()
    };
    let entry = &register[0];
    assert_eq!(kinds(entry), ["receipt", "answer"]);
    let reference = skatteetaten.lock().unwrap().reference.clone();
    assert_eq!(entry["hendelser"][0]["forsendelse_id"], reference.as_str());
    assert_eq!(kinds(&register[1]), ["failed"]);
    assert!(
        !Value::Array(register.clone())
            .to_string()
            .contains("FOR_001")
    );
    assert_eq!(body["referanse"], reference.as_str());

    // The card is evidence: the database refuses to change it.
    let changed = sqlx::query("update tax_card set result = 'x' where company_id = $1")
        .bind(company)
        .execute(&state.pool)
        .await;
    assert!(changed.is_err());
}
