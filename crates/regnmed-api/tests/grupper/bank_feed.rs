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
    rate_limited: bool,
    /// The `date_from` of the last fetch.
    last_from: Option<String>,
    /// Sessions closed at the provider (DELETE /sessions/{id}).
    closed: Vec<String>,
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
        let mut bank = s.lock().unwrap();
        bank.last_from = q.get("date_from").cloned();
        if bank.rate_limited {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(
                    json!({ "message": "ASPSP Rate limit exceeded", "code": 429, "error": "ASPSP_RATE_LIMIT_EXCEEDED" }),
                ),
            );
        }
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
            axum::routing::delete(
                |State(s): State<Shared>, Path(id): Path<String>| async move {
                    s.lock().unwrap().closed.push(id);
                    Json(json!({ "message": "OK" }))
                },
            ),
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
    // Two providers: the real client against the mock, and a second one, to
    // hold the seam open — regnmed is not tied to one provider.
    state.bank_feed = regnmed_gov::bankfeed::Providers::new(vec![
        Arc::new(
            regnmed_gov::enablebanking::Client::new(
                APP_ID,
                pem.as_bytes(),
                Some(&mock_bank(shared.clone()).await),
            )
            .unwrap(),
        ),
        Arc::new(OtherProvider),
    ]);

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
    let wanted = json!({ "account_number": "1920", "bank": "Testbank", "kontonummer": BBAN });
    let (code, why) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        &admin,
        wanted.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{why}");
    assert!(
        why["error"].as_str().unwrap().contains("tillegget"),
        "{why}"
    );

    // The add-on is granted by systemadmin, never by support, and shows in the console.
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
    let (_, before) = call(
        &state,
        "GET",
        &format!("/platform/companies/{company}"),
        &sysadmin,
        Value::Null,
    )
    .await;
    assert!(
        before["tillegg_mulige"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["addon"] == "bank"),
        "{before}"
    );
    assert_eq!(
        call(&state, "POST", &uri, &sysadmin, addon).await.0,
        StatusCode::CREATED
    );
    let (_, after) = call(
        &state,
        "GET",
        &format!("/platform/companies/{company}"),
        &sysadmin,
        Value::Null,
    )
    .await;
    assert_eq!(after["tillegg"][0]["addon"], "bank", "{after}");
    assert_eq!(after["tillegg"][0]["note"], "pilot");

    // Without a public address there is nowhere for the bank to send the user back.
    let (code, why) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        &admin,
        wanted.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        why["error"].as_str().unwrap().contains("PORTAL_BASE_URL"),
        "{why}"
    );
    state.portal_base = Some("https://portal.test".into());

    // A read-only member sees the status and the banks, and may not connect.
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
            wanted.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // Both providers' banks are on offer, each with the provider it comes through.
    let (_, banks) = call(
        &state,
        "GET",
        &format!("{base}/bank/feed/banks"),
        &reader,
        Value::Null,
    )
    .await;
    let offered: Vec<(&str, &str)> = banks["banker"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["name"].as_str().unwrap(), b["provider"].as_str().unwrap()))
        .collect();
    assert_eq!(
        offered,
        vec![("Testbank", "enablebanking"), ("Annenbank", "annen")]
    );
    let (code, via) = call(
        &state,
        "POST",
        &format!("{base}/bank/feed/connect"),
        &admin,
        json!({ "account_number": "1920", "bank": "Annenbank", "kontonummer": BBAN }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{via}");
    assert!(
        via["url"]
            .as_str()
            .unwrap()
            .starts_with("https://annen.test/"),
        "{via}"
    );
    let stored: String = sqlx::query_scalar(
        "select provider from bank_connection where company_id = $1 and bank_name = 'Annenbank'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(stored, "annen", "a connection remembers its provider");
    assert_eq!(
        call(&state, "POST", &format!("{base}/bank/feed/connect"), &admin,
            json!({ "account_number": "1920", "bank": "Testbank", "provider": "annen", "kontonummer": BBAN })).await.0,
        StatusCode::BAD_REQUEST,
        "a bank is only reachable through a provider that offers it"
    );

    // Connect: the user is sent to the bank, and the calls are signed as our application.
    let pending = connect(&state, &base, &admin).await;
    assert_eq!(shared.lock().unwrap().last_kid.as_deref(), Some(APP_ID));

    // The bank's redirect only hands the parameters to the portal, and never
    // passes on text from the open address.
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
    let cancelled = redirect(format!(
        "/bank/callback?error=access_denied&error_description=Ring+12345678&state={pending}"
    ))
    .await;
    assert!(cancelled.contains("bankfeil=Tilkoblingen"), "{cancelled}");
    assert!(!cancelled.contains("12345678"), "{cancelled}");

    // (3) The account must be the one stated: a consent for another account is
    // refused, and the session is not left open at the provider.
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
    assert_eq!(
        shared.lock().unwrap().closed.len(),
        1,
        "the unused session is closed"
    );
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
        "lines": [{ "account": "1920", "amount_ore": 1_250_000 }, { "account": "1950", "amount_ore": -1_250_000 }]
    })).await;
    assert_eq!(code, StatusCode::OK, "{why}");
    // A file uploaded earlier already holds yesterday's 640 kr payment.
    let yesterday = today - chrono::Days::new(1);
    regnmed_db::bank::import_statement(
        &state.pool,
        company,
        "1920",
        &regnmed_core::camt053::Camt053Statement {
            statement_ref: format!("fil-{}", Uuid::new_v4()),
            iban: None,
            from_date: Some(yesterday),
            to_date: Some(yesterday),
            opening_ore: None,
            closing_ore: None,
            transactions: vec![regnmed_core::camt053::Camt053Transaction {
                booking_date: yesterday,
                amount_ore: -64_000,
                description: "Nordlys Kontor AS".into(),
                reference: None,
            }],
        },
        "test",
    )
    .await
    .unwrap();

    // Fetch. Of seven lines from the bank, four become new statement lines:
    // the pending one and the zero-amount one are skipped, the 640 kr payment is
    // already there from the file, and a reference the bank reuses ("0") does
    // not merge two payments into one.
    shared.lock().unwrap().transactions = vec![
        booked("A1", "12500.00", "CRDT", "Fjellvik Bygg AS"),
        {
            let mut t = booked("A2", "640.00", "DBIT", "Nordlys Kontor AS");
            t["booking_date"] = json!(yesterday.to_string());
            t
        },
        json!({ "entry_reference": "P1", "status": "PDNG", "credit_debit_indicator": "DBIT",
                "transaction_amount": { "currency": "NOK", "amount": "99.00" },
                "booking_date": today.to_string() }),
        booked("A3", "1850.00", "DBIT", "Husleie"),
        booked("Z0", "0.00", "DBIT", "Informasjon"),
        booked("0", "10.00", "DBIT", "Gebyr"),
        booked("0", "10.00", "DBIT", "Gebyr"),
    ];
    let sync = format!("{base}/bank/feed/{pending}/sync");
    let (code, first) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{first}");
    assert_eq!(first["nye"], 4, "{first}");
    assert_eq!(first["auto_matchet"], 1, "matched by the ordinary engine");
    assert_eq!(
        shared.lock().unwrap().last_from.as_deref(),
        Some(yesterday.to_string().as_str()),
        "the first fetch starts ON the newest day already on the account, not after it"
    );
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
        vec![-185_000, -64_000, -1_000, -1_000, 1_250_000],
        "ledger sign, the file's line once, both of the reused-reference payments"
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

    // The fetch also runs by itself: completing the connection gave the company a
    // schedule (docs/jobs.md, «Tidsplaner»), at a night-time default.
    let schedules = format!("{base}/schedules");
    let (code, plans) = call(&state, "GET", &schedules, &reader, Value::Null).await;
    assert_eq!(code, StatusCode::OK, "{plans}");
    let plan = &plans["tidsplaner"][0];
    assert_eq!(plan["kind"], "bank_sync");
    assert_eq!(plan["aktiv"], true);
    assert_eq!(plan["tider"].as_array().unwrap().len(), 1);
    assert_eq!(plan["maks_per_dag"], 4);
    assert!(plan["neste"].is_string(), "{plan}");
    assert!(plan["sist"].is_null(), "nothing has run by itself yet");

    // Changing it takes the right to do the same by hand, and stays within the kind's rules.
    let put = format!("{schedules}/bank_sync");
    let twice = json!({ "tider": ["12:00", "03:15"], "aktiv": true });
    assert_eq!(
        call(&state, "PUT", &put, &reader, twice.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &state,
            "PUT",
            &put,
            &admin,
            json!({ "tider": ["01:00", "07:00", "13:00", "19:00", "23:00"], "aktiv": true })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "banks limit background fetches: at most four a day"
    );
    assert_eq!(
        call(
            &state,
            "PUT",
            &put,
            &admin,
            json!({ "tider": ["25:00"], "aktiv": true })
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &state,
            "PUT",
            &format!("{schedules}/audit_report"),
            &admin,
            twice.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&state, "PUT", &put, &other_admin, twice.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (code, changed) = call(&state, "PUT", &put, &admin, twice).await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    assert_eq!(changed["tider"], json!(["03:15", "12:00"]));

    // The schedule comes due: the worker's own code turns it into a job and runs
    // it, and the new bank line arrives without anyone pressing «Hent nå».
    shared
        .lock()
        .unwrap()
        .transactions
        .push(booked("A5", "20.00", "DBIT", "Gebyr"));
    sqlx::query(
        "update schedule set next_run_at = now() - interval '1 minute' where company_id = $1",
    )
    .bind(company)
    .execute(&state.pool)
    .await
    .unwrap();
    regnmed_db::schedule::enqueue_due(&state.pool)
        .await
        .unwrap();
    let job_id: Uuid =
        sqlx::query_scalar("select id from job where company_id = $1 and kind = 'bank_sync'")
            .bind(company)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    let claimed = regnmed_db::job::claim(&state.pool, job_id, "test-worker")
        .await
        .unwrap()
        .unwrap();
    let runtime = regnmed_db::job::Runtime {
        bank: state.bank_feed.clone(),
    };
    regnmed_db::job::process(&state.pool, &claimed, "test-worker", &runtime)
        .await
        .unwrap();
    let (_, plans) = call(&state, "GET", &schedules, &admin, Value::Null).await;
    let last = &plans["tidsplaner"][0]["sist"];
    assert_eq!(last["status"], "done", "{last}");
    assert_eq!(last["resultat"]["koblinger"], 1, "{last}");
    assert_eq!(last["resultat"]["nye"], 1, "{last}");
    assert_eq!(last["resultat"]["feil"], json!([]));
    // Its outcome is read with the right to see the bank; the console lists the schedule.
    assert_eq!(
        call(
            &state,
            "GET",
            &format!("{base}/jobs/{job_id}"),
            &reader,
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let (code, everything) =
        call(&state, "GET", "/platform/schedules", &sysadmin, Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        everything["tidsplaner"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["company_id"] == company.to_string() && t["sist_status"] == "done"),
        "{everything}"
    );
    assert_eq!(
        call(&state, "GET", "/platform/schedules", &admin, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // The bank rate limits: said in Norwegian, noted on the connection, and the
    // connection stays. The next fetch clears the note.
    shared.lock().unwrap().rate_limited = true;
    let (code, why) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        why["error"]
            .as_str()
            .unwrap()
            .contains("Prøv igjen om noen timer"),
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
    let row = status["koblinger"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == pending.to_string())
        .unwrap();
    assert_eq!(row["status"], "aktiv");
    assert!(
        row["siste_feil"]
            .as_str()
            .unwrap()
            .starts_with("Banken tillater ikke"),
        "{row}"
    );
    shared.lock().unwrap().rate_limited = false;
    assert_eq!(
        call(&state, "POST", &sync, &admin, Value::Null).await.0,
        StatusCode::OK
    );

    // Another company cannot fetch this connection, and a reader cannot fetch at all.
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
    assert_eq!(
        call(&state, "POST", &sync, &reader, Value::Null).await.0,
        StatusCode::FORBIDDEN
    );

    // Without the add-on the fetch stops (the row is backdated so that ending it bites today).
    sqlx::query("update company_addon set valid_from = current_date - 5 where company_id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
    let ended = format!("/platform/companies/{company}/addons/bank/end");
    assert_eq!(
        call(&state, "POST", &ended, &grant("support").await, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&state, "POST", &ended, &sysadmin, Value::Null).await.0,
        StatusCode::NO_CONTENT
    );
    let (code, why) = call(&state, "POST", &sync, &admin, Value::Null).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(
        why["error"].as_str().unwrap().contains("tillegget"),
        "{why}"
    );
    regnmed_db::bank_feed::grant_addon(&state.pool, company, "bank", "pilot igjen", "test")
        .await
        .unwrap();

    // A renewal replaces the connection instead of adding a second feed, and
    // the old consent is closed at the provider.
    let old_session: String =
        sqlx::query_scalar("select session_id from bank_connection where id = $1")
            .bind(pending)
            .fetch_one(&state.pool)
            .await
            .unwrap();
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
    assert!(
        shared.lock().unwrap().closed.contains(&old_session),
        "the replaced consent is closed"
    );
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
    assert_eq!(live[0]["leverandor"], "enablebanking");
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
        why["error"].as_str().unwrap().contains("Koble til på nytt"),
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
        status["koblinger"]
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k["status"] != "aktiv"),
        "{status}"
    );
    shared.lock().unwrap().expired = false;

    // «Koble fra»: not by a reader; by an admin it ends the connection here and at the provider.
    let third = connect(&state, &base, &admin).await;
    assert_eq!(
        call(
            &state,
            "POST",
            &complete,
            &admin,
            json!({ "code": "ok", "state": third })
        )
        .await
        .0,
        StatusCode::OK
    );
    let third_session: String =
        sqlx::query_scalar("select session_id from bank_connection where id = $1")
            .bind(third)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    let end = format!("{base}/bank/feed/{third}/end");
    assert_eq!(
        call(&state, "POST", &end, &reader, Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&state, "POST", &end, &admin, Value::Null).await.0,
        StatusCode::NO_CONTENT
    );
    assert!(shared.lock().unwrap().closed.contains(&third_session));
    assert_eq!(
        call(
            &state,
            "POST",
            &format!("{base}/bank/feed/{third}/sync"),
            &admin,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

/// A second provider: one bank, and a consent URL of its own. Enough to prove
/// that the API routes a bank to the provider that offers it.
struct OtherProvider;

impl regnmed_gov::bankfeed::BankFeed for OtherProvider {
    fn id(&self) -> &'static str {
        "annen"
    }
    fn banks<'a>(
        &'a self,
        _country: &'a str,
    ) -> regnmed_gov::bankfeed::Fut<'a, Vec<regnmed_gov::bankfeed::Bank>> {
        Box::pin(async {
            Ok(vec![regnmed_gov::bankfeed::Bank {
                name: "Annenbank".into(),
                country: "NO".into(),
                maximum_consent_validity: 0,
            }])
        })
    }
    fn start_auth<'a>(
        &'a self,
        r: &'a regnmed_gov::bankfeed::AuthRequest<'a>,
    ) -> regnmed_gov::bankfeed::Fut<'a, String> {
        Box::pin(async move { Ok(format!("https://annen.test/auth?state={}", r.state)) })
    }
    fn create_session<'a>(
        &'a self,
        _code: &'a str,
    ) -> regnmed_gov::bankfeed::Fut<'a, regnmed_gov::bankfeed::Session> {
        unreachable!("not exercised")
    }
    fn transactions<'a>(
        &'a self,
        _account_uid: &'a str,
        _from: chrono::NaiveDate,
    ) -> regnmed_gov::bankfeed::Fut<'a, Vec<regnmed_core::bankfeed::FeedTx>> {
        unreachable!("not exercised")
    }
    fn end_session<'a>(&'a self, _session_id: &'a str) -> regnmed_gov::bankfeed::Fut<'a, ()> {
        unreachable!("not exercised")
    }
}
