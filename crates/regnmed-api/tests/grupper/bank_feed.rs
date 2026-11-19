//! Direct bank connection (docs/bank.md tier 2, #70) against a mock of the
//! provider. What must not break: no provider configured means no feature;
//! no add-on means no connecting; the account the bank returns must be the
//! one the user stated; a fetch never imports a transaction twice; and a
//! consent the bank says is gone ends the connection instead of failing
//! forever.
//!
//! The mock answers with the shapes in Enable Banking's published API
//! reference. They have not been checked against live traffic yet.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{TestIdp, test_state, unique_orgnumber};
use regnmed_api::{AppState, router};

/// The BBAN of the standard Norwegian example IBAN, NO93 8601 1117 947.
const BBAN: &str = "86011117947";
const APP_ID: &str = "test-app";

#[derive(Default)]
struct Bank {
    transactions: Vec<Value>,
    expired: bool,
    /// The `kid` of the last token seen: proves the calls were signed as us.
    last_kid: Option<String>,
}

type Shared = Arc<Mutex<Bank>>;

fn kid(headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    jsonwebtoken::decode_header(token).ok()?.kid
}

async fn mock_bank(shared: Shared) -> String {
    async fn aspsps(State(s): State<Shared>, headers: HeaderMap) -> Json<Value> {
        s.lock().unwrap().last_kid = kid(&headers);
        Json(json!({ "aspsps": [
            { "name": "Testbank", "country": "NO", "maximum_consent_validity": 180 * 86400,
              "psu_types": ["business"] }
        ]}))
    }
    async fn auth(Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(body["psu_type"], "business");
        assert!(
            body["redirect_url"]
                .as_str()
                .unwrap()
                .ends_with("/bank/callback")
        );
        Json(json!({
            "url": format!("https://bank.test/auth?state={}", body["state"].as_str().unwrap()),
            "authorization_id": Uuid::new_v4(),
        }))
    }
    async fn sessions(Json(body): Json<Value>) -> Json<Value> {
        // Code "wrong": the user picked another account at the bank.
        let accounts = if body["code"] == "wrong" {
            json!([{ "uid": "acc-other", "currency": "NOK",
                     "account_id": { "iban": "SE4550000000058398257466" } }])
        } else {
            json!([
                { "uid": "acc-other", "currency": "NOK",
                  "account_id": { "other": { "identification": "12345678903", "scheme_name": "BBAN" } } },
                { "uid": "acc-1", "currency": "NOK", "account_id": { "iban": "NO9386011117947" } }
            ])
        };
        Json(json!({
            "session_id": Uuid::new_v4(),
            "accounts": accounts,
            "access": { "valid_until": (Utc::now() + Duration::days(180)).to_rfc3339() },
        }))
    }
    async fn transactions(
        State(s): State<Shared>,
        Path(uid): Path<String>,
        Query(q): Query<std::collections::HashMap<String, String>>,
    ) -> (StatusCode, Json<Value>) {
        let bank = s.lock().unwrap();
        if bank.expired {
            return (
                StatusCode::UNAUTHORIZED,
                Json(
                    json!({ "message": "Session is expired", "code": 401, "error": "EXPIRED_SESSION" }),
                ),
            );
        }
        assert_eq!(uid, "acc-1", "only the bound account is ever read");
        assert!(q.contains_key("date_from"));
        // Two pages, to exercise the continuation key.
        let (first, rest) = bank.transactions.split_at(bank.transactions.len().min(2));
        let body = match q.get("continuation_key") {
            None if !rest.is_empty() => {
                json!({ "transactions": first, "continuation_key": "page-2" })
            }
            None => json!({ "transactions": first }),
            Some(_) => json!({ "transactions": rest }),
        };
        (StatusCode::OK, Json(body))
    }
    let app = Router::new()
        .route("/aspsps", get(aspsps))
        .route("/auth", post(auth))
        .route("/sessions", post(sessions))
        .route(
            "/sessions/{id}",
            axum::routing::delete(|| async { Json(json!({ "message": "OK" })) }),
        )
        .route("/accounts/{uid}/transactions", get(transactions))
        .with_state(shared);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn booked(reference: &str, amount: &str, direction: &str, text: &str) -> Value {
    json!({
        "entry_reference": reference,
        "transaction_amount": { "currency": "NOK", "amount": amount },
        "credit_debit_indicator": direction,
        "status": "BOOK",
        "booking_date": Utc::now().date_naive().to_string(),
        "debtor": { "name": text }, "creditor": { "name": text },
        "remittance_information": ["Faktura"],
    })
}

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Value,
) -> (StatusCode, Value) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn member(state: &AppState, idp: &TestIdp, company: Uuid, role: &str) -> String {
    let sub = format!("{role}|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some(role), None)
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, role)
        .await
        .unwrap();
    idp.token(&sub, role)
}

/// Connect: returns the pending connection's id (the `state` in the bank URL).
async fn connect(state: &AppState, base: &str, admin: &str) -> Uuid {
    let (code, body) = call(
        state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        admin,
        json!({ "account_number": "1920", "bank": "Testbank", "kontonummer": "8601 11 17947" }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let url = body["url"].as_str().unwrap();
    Uuid::parse_str(url.rsplit("state=").next().unwrap()).unwrap()
}

#[tokio::test]
async fn the_bank_feed_connects_one_account_and_imports_each_transaction_once() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Banktest AS")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let admin = member(&state, &idp, company, "admin").await;
    let reader = member(&state, &idp, company, "les").await;
    let base = format!("/companies/{company}");
    for account in ["1920", "1950"] {
        let (code, why) = call(
            &state,
            "POST",
            &format!("{base}/accounts"),
            &admin,
            json!({ "number": account }),
        )
        .await;
        assert!(code.is_success(), "{why}");
    }

    // (1) No provider configured: no feature, and every action says so.
    let (_, status) = call(
        &state,
        "GET",
        &format!("{base}/bank/feed"),
        &admin,
        Value::Null,
    )
    .await;
    assert_eq!(status["konfigurert"], false);
    let (code, _) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        &admin,
        json!({ "account_number": "1920", "bank": "Testbank" }),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);

    let shared: Shared = Default::default();
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
    state.bank_feed = Some(Arc::new(
        regnmed_gov::enablebanking::Client::new(
            APP_ID,
            pem.as_bytes(),
            Some(&mock_bank(shared.clone()).await),
        )
        .unwrap(),
    ));
    state.portal_base = Some("https://portal.test".into());

    // (2) No add-on: configured, but the company cannot connect.
    let (_, status) = call(
        &state,
        "GET",
        &format!("{base}/bank/feed"),
        &admin,
        Value::Null,
    )
    .await;
    assert_eq!(
        (&status["konfigurert"], &status["tillegg"]),
        (&json!(true), &json!(false))
    );
    let (code, why) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        &admin,
        json!({ "account_number": "1920", "bank": "Testbank", "kontonummer": BBAN }),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{why}");
    assert!(
        why["error"].as_str().unwrap().contains("tillegget"),
        "{why}"
    );

    // The add-on is granted by systemadmin, never by support.
    let grant = |role: &'static str| {
        let (pool, idp) = (state.pool.clone(), &idp);
        async move {
            let sub = format!("plattform|{}", Uuid::new_v4());
            let id = regnmed_db::ensure_person(
                &pool,
                &sub,
                Some(role),
                Some(&format!("{}@test.invalid", Uuid::new_v4())),
            )
            .await
            .unwrap();
            regnmed_db::grant_platform_role(
                &pool,
                id,
                role,
                (Utc::now() + Duration::days(30)).date_naive(),
                "test",
                None,
            )
            .await
            .unwrap();
            idp.token(&sub, role)
        }
    };
    let addon = json!({ "addon": "bank", "note": "pilot" });
    let uri = format!("/platform/companies/{company}/addons");
    assert_eq!(
        call(&state, "POST", &uri, &grant("support").await, addon.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&state, "POST", &uri, &admin, addon.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    let sysadmin = grant("systemadmin").await;
    assert_eq!(
        call(
            &state,
            "POST",
            &uri,
            &sysadmin,
            json!({ "addon": "basis", "note": "x" })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "a package is not an add-on"
    );
    assert_eq!(
        call(&state, "POST", &uri, &sysadmin, addon).await.0,
        StatusCode::CREATED
    );

    // A read-only member sees the status and may not connect.
    assert_eq!(
        call(
            &state,
            "GET",
            &format!("{base}/bank/feed"),
            &reader,
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &state,
            "POST",
            &format!("{base}/bank/feed/connect"),
            &reader,
            json!({ "account_number": "1920", "bank": "Testbank", "kontonummer": BBAN })
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // Connect: the user is sent to the bank, and the calls are signed as our application.
    let pending = connect(&state, &base, &admin).await;
    assert_eq!(shared.lock().unwrap().last_kid.as_deref(), Some(APP_ID));

    // The bank's redirect only hands the parameters to the portal.
    let redirect = |uri: String| {
        let state = state.clone();
        async move {
            let response = router(state)
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        }
    };
    assert_eq!(
        redirect(format!("/bank/callback?code=ok&state={pending}")).await,
        format!("/#/c/{company}/bank?bankkode=ok&banktilstand={pending}")
    );
    assert_eq!(
        redirect(format!("/bank/callback?code=ok&state={}", Uuid::new_v4())).await,
        "/"
    );
    assert!(
        redirect(format!(
            "/bank/callback?error=access_denied&state={pending}"
        ))
        .await
        .contains("bankfeil=access_denied")
    );

    // (3) The account must be the one stated: a consent for another account is refused.
    let complete = format!("{base}/bank/feed/complete");
    let (code, why) = call(
        &state,
        "POST",
        &complete,
        &admin,
        json!({ "code": "wrong", "state": pending }),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{why}");
    assert!(why["error"].as_str().unwrap().contains(BBAN), "{why}");
    // Not by a reader, not for another company's connection.
    assert_eq!(
        call(
            &state,
            "POST",
            &complete,
            &reader,
            json!({ "code": "ok", "state": pending })
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let other = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Annet AS")
        .await
        .unwrap();
    let other_admin = member(&state, &idp, other, "admin").await;
    regnmed_db::bank_feed::grant_addon(&state.pool, other, "bank", "pilot", "test")
        .await
        .unwrap();
    let (code, _) = call(
        &state,
        "POST",
        &format!("/companies/{other}/bank/feed/complete"),
        &other_admin,
        json!({ "code": "ok", "state": pending }),
    )
    .await;
    assert_eq!(
        code,
        StatusCode::BAD_REQUEST,
        "a state belongs to the company that started it"
    );

    let (code, done) = call(
        &state,
        "POST",
        &complete,
        &admin,
        json!({ "code": "ok", "state": pending }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{done}");
    assert_eq!(done["status"], "aktiv");
    let (code, _) = call(
        &state,
        "POST",
        &complete,
        &admin,
        json!({ "code": "ok", "state": pending }),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "a code completes once");

    // A ledger entry the fetch should match: 12 500 kr into 1920 today.
    let today = Utc::now().date_naive();
    let (code, why) = call(&state, "POST", &format!("{base}/vouchers"), &admin, json!({
        "journal_code": "GL", "date": today, "description": "Innbetaling",
        "lines": [{ "account": "1920", "amount_ore": 12_500_00 }, { "account": "1950", "amount_ore": -12_500_00 }]
    })).await;
    assert_eq!(code, StatusCode::OK, "{why}");

    // Fetch: three booked transactions over two pages; the pending one is skipped.
    shared.lock().unwrap().transactions = vec![
        booked("A1", "12500.00", "CRDT", "Fjellvik Bygg AS"),
        booked("A2", "640.00", "DBIT", "Nordlys Kontor AS"),
        json!({ "entry_reference": "P1", "status": "PDNG", "credit_debit_indicator": "DBIT",
                "transaction_amount": { "currency": "NOK", "amount": "99.00" },
                "booking_date": today.to_string() }),
        booked("A3", "1850.00", "DBIT", "Husleie"),
    ];
    let sync = format!("{base}/bank/feed/{pending}/sync");
    let (code, first) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{first}");
    assert_eq!(first["nye"], 3);
    assert_eq!(first["auto_matchet"], 1, "matched by the ordinary engine");
    let amounts: Vec<i64> = sqlx::query_scalar(
        "select t.amount_ore from bank_transaction t join bank_statement s on s.id = t.statement_id
         where s.company_id = $1 order by t.amount_ore",
    )
    .bind(company)
    .fetch_all(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        amounts,
        vec![-1_850_00, -640_00, 12_500_00],
        "ledger sign: money in is positive"
    );

    // The same fetch again imports nothing; one new transaction imports one.
    let (_, again) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(again["nye"], 0, "an overlapping fetch is not a second copy");
    shared
        .lock()
        .unwrap()
        .transactions
        .push(booked("A4", "75.00", "DBIT", "Gebyr"));
    let (_, more) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(more["nye"], 1);

    // Another company cannot fetch this connection.
    assert_eq!(
        call(
            &state,
            "POST",
            &format!("/companies/{other}/bank/feed/{pending}/sync"),
            &other_admin,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // A renewal replaces the connection instead of adding a second feed.
    let renewed = connect(&state, &base, &admin).await;
    let (code, _) = call(
        &state,
        "POST",
        &complete,
        &admin,
        json!({ "code": "ok", "state": renewed }),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    let (_, status) = call(
        &state,
        "GET",
        &format!("{base}/bank/feed"),
        &admin,
        Value::Null,
    )
    .await;
    let live: Vec<&Value> = status["koblinger"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|k| k["status"] == "aktiv")
        .collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0]["id"], renewed.to_string());
    let (_, after) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/{renewed}/sync"),
        &admin,
        Value::Null,
    )
    .await;
    assert_eq!(
        after["nye"], 0,
        "the renewed consent does not re-import the account's history"
    );

    // The bank says the consent is gone: the connection ends, with a message to connect again.
    shared.lock().unwrap().expired = true;
    let (code, why) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/{renewed}/sync"),
        &admin,
        Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        why["error"].as_str().unwrap().contains("koble til på nytt"),
        "{why}"
    );
    let (_, status) = call(
        &state,
        "GET",
        &format!("{base}/bank/feed"),
        &admin,
        Value::Null,
    )
    .await;
    assert!(
        status["koblinger"].as_array().unwrap().is_empty(),
        "{status}"
    );
}
