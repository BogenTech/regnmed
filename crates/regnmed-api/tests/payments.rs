//! Betalingsliste og remittering end to end: open leverandør-poster
//! become an utkast run, approval is a SEPARATE audited action that
//! renders and stores the pain.001 file (hash-checked download),
//! settlement posts the utbetalingsbilag and closes every reskontro-
//! rest in one transaction, and the run history is immutable at the
//! DB layer. Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let response = router(state.clone())
        .oneshot(builder.body(Body::from(body.unwrap_or_default())).unwrap())
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

async fn body_text(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, String) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn leverandorfaktura(dato: chrono::NaiveDate, belop: i64, party_no: &str) -> VoucherDraft {
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Leverandørfaktura {belop}"),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "4300".into(),
                amount: Ore(belop),
                vat_code: None,
                description: None,
                party_no: None,
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
            EntryDraft {
                account_number: "2400".into(),
                amount: Ore(-belop),
                vat_code: None,
                description: Some("Varekjøp".into()),
                party_no: Some(party_no.to_string()),
                avdeling: None,
                prosjekt: None,
                valuta: None,
            },
        ],
    }
}

async fn saldo(state: &AppState, company: Uuid, konto: &str) -> i64 {
    sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = $2",
    )
    .bind(company)
    .bind(konto)
    .fetch_one(&state.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn betalingsliste_pain001_and_settlement() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Betalings Ansvarlig"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Innkjøp AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("2400", "Leverandørgjeld"),
        ("4300", "Varekostnad"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    sqlx::query("update company set bank_account = '8601.11.17947' where id = $1")
        .bind(company)
        .execute(&state.pool)
        .await
        .unwrap();
    let (grossist_id, grossist_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Grossisten AS",
        None,
        None,
    )
    .await
    .unwrap();
    let (_, uten_konto_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Kontantløs AS",
        None,
        None,
    )
    .await
    .unwrap();

    // Kontonummeret valideres MOD11 før lagring.
    let bad = regnmed_db::update_party_contact(
        &state.pool,
        company,
        grossist_id,
        None,
        None,
        Some("86011117948"),
    )
    .await;
    assert!(bad.is_err(), "feil kontrollsiffer avvises");
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        grossist_id,
        None,
        None,
        Some("8601 11 17947"),
    )
    .await
    .unwrap();

    let date = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 1), 12_500_00, &grossist_no),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 5), 800_00, &grossist_no),
        "test",
    )
    .await
    .unwrap();
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &leverandorfaktura(date(7, 8), 500_00, &uten_konto_no),
        "test",
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Betalings Ansvarlig");
    let base = format!("/companies/{company}/payments");

    // The payable list: three open posts, one flagged without konto.
    let (status, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "body: {payable}");
    let items = payable["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    let entry_of = |belop: i64| {
        items.iter().find(|i| i["belop_ore"] == belop).unwrap()["entry_id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let uten_konto_entry = items
        .iter()
        .find(|i| i["party_name"] == "Kontantløs AS")
        .unwrap();
    assert!(uten_konto_entry["bank_account"].is_null());

    // A post whose supplier lacks a konto is refused loudly.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(
            serde_json::json!({
                "items": [{ "entry_id": uten_konto_entry["entry_id"] }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The list: full remainder on one post, KID on the other.
    let (status, made) = request(
        &state,
        "POST",
        &format!("{base}/runs"),
        &token,
        Some(
            serde_json::json!({
                "items": [
                    { "entry_id": entry_of(12_500_00), "kid": "000000018" },
                    { "entry_id": entry_of(800_00), "melding": "Faktura 99" },
                ],
                "execution_date": "2026-08-01",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    let run_id = made["run_id"].as_str().unwrap().to_string();
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert!(
        payable["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["i_kjoring"] == true)
            .count()
            == 2,
        "postene er flagget i kjøring"
    );

    // The file exists only after the SEPARATE approval action.
    let (status, _) = body_text(&state, &format!("{base}/runs/{run_id}/file"), &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "ingen fil før godkjenning");
    let (status, approved) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/approve"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert!(approved["file_sha256"].is_string());
    let (status, xml) = body_text(&state, &format!("{base}/runs/{run_id}/file"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(xml.contains("pain.001.001.03"));
    assert!(xml.contains("<CtrlSum>13300.00</CtrlSum>"));
    assert!(xml.contains("<Ref>000000018</Ref>"), "KID som SCOR");
    assert!(xml.contains("<Ustrd>Faktura 99</Ustrd>"));
    assert!(
        xml.contains("<Id>86011117947</Id>"),
        "normalisert kontonummer"
    );

    // Run history is evidence at the DB layer.
    let tamper = sqlx::query("update payment_run set execution_date = '2027-01-01' where id = $1")
        .bind(Uuid::parse_str(&run_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "run content is immutable");
    let tamper = sqlx::query("update payment_run_item set belop_ore = 1 where run_id = $1")
        .bind(Uuid::parse_str(&run_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "items are append-only");

    // Settlement: one voucher, every post closed, in one transaction.
    let (status, settled) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/settle"),
        &token,
        Some(serde_json::json!({ "dato": "2026-08-01" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {settled}");
    assert_eq!(saldo(&state, company, "1920").await, -13_300_00);
    assert_eq!(
        saldo(&state, company, "2400").await,
        -500_00,
        "bare den kontoløse posten står igjen"
    );
    let open_grossist: i64 = sqlx::query_scalar(
        "select count(*) from entry e
         where e.party_id = $1
           and e.amount_ore <> coalesce((select sum(m.amount_ore) from reskontro_match m
                                         where m.entry_a = e.id), 0)
                             + coalesce((select sum(-m.amount_ore) from reskontro_match m
                                         where m.entry_b = e.id), 0)",
    )
    .bind(grossist_id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(open_grossist, 0, "grossistens reskontro er i null");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/settle"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already settled");

    // Cancel is one-way and only from utkast.
    let (_, payable) = request(&state, "GET", &format!("{base}/payable"), &token, None).await;
    assert_eq!(
        payable["items"].as_array().unwrap().len(),
        1,
        "bare den kontoløse posten er åpen"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/runs/{run_id}/cancel"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "utbetalt kan ikke annulleres"
    );

    // The chain verifies over the lot: 3 inngående + utbetalingen.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 4);
}
