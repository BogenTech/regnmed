//! Payments to self-employed people (#363, docs/selvstendig.md): the
//! report is read from the supplier ledger as invoiced excluding VAT net
//! of credit notes, only suppliers the customer marked are in it, the
//! file carries the fødselsnummer while the preview does not, and the
//! delivery is stored before it is sent and never filed twice.
//!
//! Requires DATABASE_URL — skips politely otherwise.

use crate::common::{TestIdp, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    token: &str,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
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

fn valid_orgnumber() -> String {
    std::iter::repeat_with(unique_orgnumber)
        .find(|n| regnmed_core::orgnumber::is_valid(n))
        .unwrap()
}

fn line(account: &str, ore: i64, party_no: Option<&str>) -> EntryDraft {
    EntryDraft {
        account_number: account.into(),
        amount: Ore(ore),
        vat_code: None,
        description: None,
        party_no: party_no.map(str::to_string),
        department: None,
        project: None,
        currency: None,
    }
}

async fn post(state: &AppState, company: Uuid, date: (i32, u32, u32), entries: Vec<EntryDraft>) {
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(date.0, date.1, date.2).unwrap(),
        description: "Bilag".into(),
        reverses: None,
        entries,
    };
    regnmed_db::post_voucher(&state.pool, company, &draft, "test")
        .await
        .unwrap();
}

/// (idempotencyKey, body) per POST, and how many to refuse first.
type Calls = Arc<Mutex<Vec<(String, String)>>>;

async fn mock_skatteetaten(refuse_first: usize) -> (String, Calls) {
    use axum::extract::{Path, State};
    use axum::http::HeaderMap;
    use axum::routing::{get, post};
    let calls: Calls = Arc::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/token",
            post(|| async { axum::Json(json!({"access_token": "t", "expires_in": 120})) }),
        )
        .route(
            "/v1/{year}",
            post(
                move |State(c): State<Calls>, Path(_): Path<String>, h: HeaderMap, body: String| async move {
                    let key = h["idempotencyKey"].to_str().unwrap().to_string();
                    let n = {
                        let mut c = c.lock().unwrap();
                        c.push((key, body));
                        c.len()
                    };
                    if n <= refuse_first {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            axum::Json(json!({"kode": "GLD_017", "melding": "Uspesifisert systemfeil"})),
                        );
                    }
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "dialogId": format!("d-{n}"),
                            "forsendelseId": format!("f-{n}"),
                            "oppgavegiversLeveranseReferanse": "r",
                            "antallOppgaver": 2
                        })),
                    )
                },
            ),
        )
        .route(
            "/dp/enduser/dialogs/{id}",
            get(|Path(id): Path<String>| async move {
                let f = id.replace("d-", "f-");
                axum::Json(json!({"transmissions": [
                    {"id": f, "type": "Submission"},
                    {"id": "a-1", "type": "Acceptance", "relatedTransmissionId": f,
                     "content": {"title": {"value": [{"languageCode": "nb", "value": "Godkjent"}]}}}
                ]}))
            }),
        )
        .with_state(calls.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, calls)
}

#[tokio::test]
async fn the_report_comes_from_the_supplier_ledger_and_is_filed_once() {
    let idp = TestIdp::new();
    let Some(mut state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Kontor"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &valid_orgnumber(), "Oppdrag AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    let token = idp.token(&sub, "Kari Kontor");
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("2400", "Leverandørgjeld"),
        ("2090", "Kredittkort"),
        ("2710", "Inngående mva"),
        ("4000", "Innkjøp"),
        ("8060", "Valutagevinst (agio)"),
        ("2050", "Annen egenkapital"),
        ("2410", "Leverandørgjeld, migrert"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    let supplier = |name: &'static str, orgnumber: Option<String>| {
        let pool = state.pool.clone();
        async move {
            regnmed_db::create_party(
                &pool,
                company,
                "leverandor",
                name,
                orgnumber.as_deref(),
                None,
            )
            .await
            .unwrap()
        }
    };
    let (snekker, snekker_no) = supplier("Ola Snekker", None).await;
    let maler_org = valid_orgnumber();
    let (maler, maler_no) = supplier("Maler Hansen ENK", Some(maler_org.clone())).await;
    let (byggvare, byggvare_no) = supplier("Byggvare AS", Some(valid_orgnumber())).await;
    let (_, rorlegger_no) = supplier("Rørlegger Rask", None).await;

    // Snekker: 10 000 + 25 % VAT, paid, then a credit note for 2 000 + VAT.
    post(
        &state,
        company,
        (2026, 3, 1),
        vec![
            line("4000", 1_000_000, None),
            line("2710", 250_000, None),
            line("2400", -1_250_000, Some(&snekker_no)),
        ],
    )
    .await;
    post(
        &state,
        company,
        (2026, 3, 20),
        vec![
            line("2400", 1_250_000, Some(&snekker_no)),
            line("1920", -1_250_000, None),
        ],
    )
    .await;
    post(
        &state,
        company,
        (2026, 4, 2),
        vec![
            line("2400", 250_000, Some(&snekker_no)),
            line("4000", -200_000, None),
            line("2710", -50_000, None),
        ],
    )
    .await;
    // Paid by company credit card and an agio gain on the supplier: both
    // debit the supplier without crediting a cost, so neither is a credit
    // note (seen to fail when only 19xx counted as payment).
    post(
        &state,
        company,
        (2026, 4, 3),
        vec![
            line("2400", 100_000, Some(&snekker_no)),
            line("2090", -100_000, None),
        ],
    )
    .await;
    post(
        &state,
        company,
        (2026, 4, 4),
        vec![
            line("2400", 1_000, Some(&snekker_no)),
            line("8060", -1_000, None),
        ],
    )
    .await;
    // Maler, outside the VAT register: 4 000,50 this year, more last year.
    post(
        &state,
        company,
        (2026, 6, 1),
        vec![
            line("4000", 400_050, None),
            line("2400", -400_050, Some(&maler_no)),
        ],
    )
    .await;
    post(
        &state,
        company,
        (2025, 12, 31),
        vec![
            line("4000", 900_000, None),
            line("2400", -900_000, Some(&maler_no)),
        ],
    )
    .await;
    // An AS, judged not self-employed; and a supplier nobody has judged.
    post(
        &state,
        company,
        (2026, 7, 1),
        vec![
            line("4000", 3_000_000, None),
            line("2400", -3_000_000, Some(&byggvare_no)),
        ],
    )
    .await;
    post(
        &state,
        company,
        (2026, 8, 1),
        vec![
            line("4000", 100_000, None),
            line("2400", -100_000, Some(&rorlegger_no)),
        ],
    )
    .await;

    let base = format!("/companies/{company}");
    // Migrated in March: last December's open invoice from the carpenter
    // comes in through the open-items import against 2050. History, not
    // an invoice of 2026, so it stays out of the report.
    let rows = regnmed_core::migration_csv::parse_open_items(
        "Navn;Fakturanr;Fakturadato;Restbeløp\nOla Snekker;L-9;15.12.2025;40 000,00\n",
    )
    .unwrap();
    regnmed_db::migration::import_open_items(
        &state.pool,
        company,
        regnmed_core::migration_csv::PartyKind::Supplier,
        "2410",
        "2050",
        NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
        &rows,
        "test",
    )
    .await
    .unwrap();

    let plain = state.clone();
    let assess = |party: Uuid, body: serde_json::Value| {
        let (state, token, base) = (plain.clone(), token.clone(), base.clone());
        async move {
            call(
                &state,
                &token,
                "POST",
                &format!("{base}/parties/{party}/self-employed"),
                Some(body),
            )
            .await
        }
    };
    // A fødselsnummer is refused for a supplier with an orgnr, and a bad
    // one is refused outright.
    let (status, body) = assess(
        maler,
        json!({"selvstendig": true, "fodselsnummer": "26829398612"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = assess(
        snekker,
        json!({"selvstendig": true, "fodselsnummer": "26829398613"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        assess(maler, json!({"selvstendig": true})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        assess(byggvare, json!({"selvstendig": false})).await.0,
        StatusCode::OK
    );
    // Marked yes with no identifier: the report says so and is not delivered.
    assert_eq!(
        assess(snekker, json!({"selvstendig": true})).await.0,
        StatusCode::OK
    );
    let report = format!("{base}/reports/selvstendig?year=2026");
    let (_, body) = call(&state, &token, "GET", &report, None).await;
    assert_eq!(body["leverbar"], false, "{body}");
    assert!(
        body["hindringer"][0]
            .as_str()
            .unwrap()
            .contains("Ola Snekker")
    );
    let (status, _) = call(&state, &token, "GET", &format!("{report}&format=fil"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = assess(
        snekker,
        json!({"selvstendig": true, "fodselsnummer": "26829398612"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["historikk"].as_array().unwrap().len(), 2);
    assert!(
        !body.to_string().contains("26829398612"),
        "the number is never shown"
    );
    // Saving yes again without retyping the number keeps it.
    let (status, body) = assess(snekker, json!({"selvstendig": true})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["gjeldende"]["har_fodselsnummer"], true, "{body}");

    let (status, body) = call(&state, &token, "GET", &report, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["leverbar"], true, "{body}");
    let got: Vec<(String, i64)> = body["mottakere"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["navn"].as_str().unwrap().into(),
                m["rapporteres_kr"].as_i64().unwrap(),
            )
        })
        .collect();
    // 10 000 − 2 000 excl. VAT; payment not counted. 4 000,50 rounds up.
    assert_eq!(
        got,
        vec![
            ("Maler Hansen ENK".into(), 4_001),
            ("Ola Snekker".into(), 8_000)
        ]
    );
    assert_eq!(body["sum_kr"], 12_001);
    assert_eq!(body["ikke_vurdert"][0]["navn"], "Rørlegger Rask");
    assert!(!body.to_string().contains("26829398612"));
    assert_eq!(body["mottakere"][1]["fodselsdato"], "1993-02-26");

    // The file to file by hand carries the number.
    let (_, file) = call(&state, &token, "GET", &format!("{report}&format=fil"), None).await;
    let doc: serde_json::Value = serde_json::from_str(file["dokument"].as_str().unwrap()).unwrap();
    assert_eq!(
        doc["leveranse"]["oppgave"][1]["oppgaveeier"]["foedselsnummer"],
        "26829398612"
    );
    assert_eq!(
        doc["leveranse"]["oppgave"][0]["oppgaveeier"]["organisasjonsnummer"],
        maler_org
    );
    assert_eq!(
        doc["leveranse"]["oppgaveoppsummering"]["sumBetaltBeloep"],
        12_001
    );

    // Submission: no rail, then the terms, then a refused attempt that is
    // retried with the same key and body.
    let submissions = format!("{base}/reports/selvstendig/submissions?year=2026");
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("ikke satt opp"),
        "{body}"
    );

    let (mock, calls) = mock_skatteetaten(1).await;
    state.contractor_payments = Some(Arc::new(regnmed_db::contractor_payments::Rail {
        tokens: regnmed_gov::maskinporten::TokenProvider::new(
            regnmed_gov::maskinporten::MaskinportenConfig {
                token_endpoint: format!("{mock}/token"),
                audience: "https://test.maskinporten.no/".into(),
                client_id: "regnmed-test".into(),
                encoding_key: idp.encoding_key.clone(),
                kid: None,
                scopes: regnmed_gov::contractor_payments::SCOPES.into(),
            },
        ),
        client: regnmed_gov::contractor_payments::ContractorPaymentsClient::new(
            format!("{mock}/v1"),
            format!("{mock}/dp"),
        ),
    }));
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("kundevilkårene"),
        "{body}"
    );
    assert!(calls.lock().unwrap().is_empty());
    let (status, _) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/terms/accept"),
        Some(json!({ "versjon": *regnmed_api::terms::VERSION })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("GLD_017"),
        "{body}"
    );
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "levert");
    {
        let c = calls.lock().unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], c[1], "a retry is the same key and the same body");
        let sent: serde_json::Value = serde_json::from_str(&c[0].1).unwrap();
        assert_eq!(
            sent["leveranse"]["oppgavegiversLeveranseReferanse"],
            body["id"]
        );
    }

    // The same again is refused: nothing to correct, nothing sent.
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("allerede levert"),
        "{body}"
    );
    assert_eq!(calls.lock().unwrap().len(), 2);

    // A correction: the painter turns out to have an office. Their record
    // is withdrawn with 0, the carpenter's stands.
    assert_eq!(
        assess(maler, json!({"selvstendig": false})).await.0,
        StatusCode::OK
    );
    let (_, body) = call(&state, &token, "GET", &report, None).await;
    assert_eq!(body["trekkes_tilbake"], json!(["Maler Hansen ENK"]));
    let (status, body) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (key, sent) = calls.lock().unwrap()[2].clone();
    assert_ne!(
        key,
        calls.lock().unwrap()[0].0,
        "a new delivery has a new key"
    );
    let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
    let records = sent["leveranse"]["oppgave"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1]["oppgaveeier"]["organisasjonsnummer"], maler_org);
    assert_eq!(records[1]["betaltBeloep"], 0);
    let corrected = body.clone();

    // Delivered as it stands: the withdrawn record stays at 0 in the set,
    // so nothing new is offered and nothing is sent again.
    let (_, page) = call(&state, &token, "GET", &report, None).await;
    assert_eq!(page["trekkes_tilbake"], json!([]), "{page}");
    let (status, again) = call(&state, &token, "POST", &submissions, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{again}");
    assert!(again["error"].as_str().unwrap().contains("allerede levert"));
    assert_eq!(calls.lock().unwrap().len(), 3);
    let body = corrected;

    // Skatteetaten's answer, from the receipt's Dialogporten dialog.
    let id = body["id"].as_str().unwrap();
    let (status, _) = call(
        &state,
        &token,
        "POST",
        &format!("{base}/reports/selvstendig/submissions/{id}/outcome"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = call(&state, &token, "GET", &submissions, None).await;
    let list = body["innsendinger"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["status"], "godkjent", "{body}");
    assert_eq!(list[1]["status"], "levert");

    // The terms' register lists every delivery.
    let (_, body) = call(
        &state,
        &token,
        "GET",
        &format!("{base}/authority-calls"),
        None,
    )
    .await;
    let n = body
        .as_array()
        .or(body["kall"].as_array())
        .unwrap()
        .iter()
        .filter(|c| c.to_string().contains("betalinger-selvstendig"))
        .count();
    assert_eq!(n, 2, "{body}");
}
