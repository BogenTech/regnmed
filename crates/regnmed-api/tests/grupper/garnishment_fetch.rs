//! Trekkpålegg fetched from Skatteetaten (docs/trekkpalegg.md §2, #334),
//! through the API: «Hent trekkpålegg» against a mock of the documented
//! list endpoint, the call in the terms' §7.1 register, and a fetched
//! order's (trekkid, trekkversjon) carried from the preview to the
//! booking.
//!
//! The list is in the shape of Skatteetaten's documentation examples
//! (docs/trekkpalegg/liste-fra-dokumentasjonen.json); no live answer exists
//! yet (the scope is on order, SSV-6690).
//!
//! Requires DATABASE_URL — skips politely otherwise.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use regnmed_api::{AppState, router};

#[derive(Default)]
struct Skatteetaten {
    /// What the list endpoint answers; None answers like a failing service.
    list: Option<Value>,
    calls: usize,
    bearer: Option<String>,
}

type Mock = Arc<Mutex<Skatteetaten>>;

async fn mock_skatteetaten() -> (String, Mock) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Mock::default();
    let app = Router::new()
        .route(
            "/token",
            post(|| async { Json(json!({"access_token": "systembruker-token", "expires_in": 120})) }),
        )
        .route(
            "/api/trekkpaalegg/v1",
            get(|State(m): State<Mock>, headers: HeaderMap| async move {
                let mut m = m.lock().unwrap();
                m.calls += 1;
                m.bearer = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                match &m.list {
                    Some(list) => (StatusCode::OK, list.to_string()),
                    None => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        json!({"kode": "KB-002", "melding": "Uventet feil i et bakenforliggende system."})
                            .to_string(),
                    ),
                }
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
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
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
async fn a_fetched_order_is_stored_registered_and_carried_into_the_run() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let orgnumber = std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &orgnumber, "Trekkpålegg AS")
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
    for role in ["admin", "les"] {
        let sub = format!("{role}|{}", Uuid::new_v4());
        let person = regnmed_db::ensure_person(&state.pool, &sub, Some(role), None)
            .await
            .unwrap();
        regnmed_db::ensure_company_member(&state.pool, company, person, role)
            .await
            .unwrap();
        tokens.push(idp.token(&sub, role));
    }
    let (admin, reader) = (&tokens[0], &tokens[1]);
    let base = format!("/companies/{company}");
    let fetch = format!("{base}/payroll/garnishments/fetch");

    let (code, created) = call(
        &state,
        admin,
        "POST",
        &format!("{base}/employees"),
        json!({"fodselsnummer": "26829398612", "navn": "Kari Utvikler",
               "ansatt_fra": "2026-01-01", "manedslonn_ore": 4_000_000,
               "trekk_type": "prosent", "trekk_prosent_bp": 3500}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{created}");
    let kari: Uuid = created["employee_id"].as_str().unwrap().parse().unwrap();

    // Without the rail the button says so.
    let (code, body) = call(&state, admin, "POST", &fetch, Value::Null).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("ikke satt opp"),
        "{body}"
    );

    let (mock, skatteetaten) = mock_skatteetaten().await;
    state.garnishment = Some(Arc::new(regnmed_db::garnishment::Rail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{mock}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::garnishment::SCOPES.into(),
            },
        ),
        client: regnmed_gov::garnishment::GarnishmentClient::new(format!(
            "{mock}/api/trekkpaalegg/v1"
        )),
    }));

    // Who owes what is LONN_LES/LONN_SKRIV territory: `les` may not fetch.
    let (code, _) = call(&state, reader, "POST", &fetch, Value::Null).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
    // A call on the company's behalf needs the terms accepted.
    let (code, body) = call(&state, admin, "POST", &fetch, Value::Null).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("kundevilkårene"),
        "{body}"
    );
    assert_eq!(skatteetaten.lock().unwrap().calls, 0);
    // So does the daily job, which calls the fetch without the handler.
    let rail = state.garnishment.clone().unwrap();
    let e = regnmed_db::garnishment::fetch(&state.pool, company, &rail, "arbeider")
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("kundevilkår"), "{e:#}");
    assert_eq!(skatteetaten.lock().unwrap().calls, 0);
    let (code, body) = call(
        &state,
        admin,
        "POST",
        &format!("{base}/terms/accept"),
        json!({ "versjon": *regnmed_api::terms::VERSION }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");

    // A failing Skatteetaten: the error comes back verbatim, and the
    // request was recorded before the call.
    let (code, body) = call(&state, admin, "POST", &fetch, Value::Null).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("KB-002"), "{body}");

    let mut order: Value = serde_json::from_str::<Value>(include_str!(
        "../../../../docs/trekkpalegg/liste-fra-dokumentasjonen.json"
    ))
    .unwrap()
    .as_array()
    .unwrap()
    .iter()
    .find(|o| o["trekkid"] == "10006")
    .unwrap()
    .clone();
    order["trekkpliktig"] = json!(orgnumber);
    order["skyldner"] = json!("26829398612");
    // The documentation's payee fails MOD11; Skatteetaten's own number.
    order["betalingsinformasjon"]["betalingsmottaker"] = json!("974761076");
    skatteetaten.lock().unwrap().list = Some(json!([order]));
    let (code, body) = call(&state, admin, "POST", &fetch, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(
        (body["antall"].as_u64(), body["nye_versjoner"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(
        skatteetaten.lock().unwrap().bearer.as_deref(),
        Some("Bearer systembruker-token")
    );

    let (code, listed) = call(
        &state,
        admin,
        "GET",
        &format!("{base}/payroll/garnishments"),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{listed}");
    let fetched = &listed["hentede"][0];
    assert_eq!(fetched["trekkid"], "10006");
    assert_eq!(fetched["navn"], "Kari Utvikler");
    assert_eq!(fetched["i_siste_liste"], true);
    assert_eq!(fetched["perioder"][2]["belop_ore"], 700_000);
    let attempts = listed["henting"]["forsok"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0]["ok"], true);
    assert_eq!(attempts[1]["ok"], false);
    // The fetch is now scheduled daily.
    let scheduled: bool = sqlx::query_scalar(
        "select exists (select 1 from schedule where company_id = $1 and kind = 'garnishment_fetch')",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert!(scheduled);

    // Both calls are in the §7.1 register; THAT one failed, never why.
    let (code, register) = call(
        &state,
        reader,
        "GET",
        &format!("{base}/authority-calls"),
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{register}");
    let text = register.to_string();
    assert_eq!(text.matches("\"trekkpalegg\"").count(), 2, "{text}");
    assert!(!text.contains("KB-002"), "{text}");

    // The preview carries (trekkid, trekkversjon), and the booking sends
    // it back.
    let run = json!({"ar": 2026, "maned": 3, "utbetalt_dato": "2026-03-25", "sone": "I",
                     "linjer": [{"employee_id": kari}]});
    let (code, preview) = call(
        &state,
        admin,
        "POST",
        &format!("{base}/payroll?preview=true"),
        run.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["trekkpalegg_sett"],
        json!([{"trekkid": "10006", "trekkversjon": 3}])
    );
    let line = &preview["linjer"][0]["trekkpalegg"][0];
    assert_eq!(line["trekkid"], "10006");
    assert_eq!(line["trukket_ore"], 700_000);
    assert!(line.get("registrering").is_none(), "{line}");

    let (code, body) = call(
        &state,
        admin,
        "POST",
        &format!("{base}/payroll"),
        run.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
    let mut booked = run;
    booked["trekkpalegg_sett"] = preview["trekkpalegg_sett"].clone();
    let (code, body) = call(&state, admin, "POST", &format!("{base}/payroll"), booked).await;
    assert_eq!(code, StatusCode::OK, "{body}");
}
