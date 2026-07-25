//! Mva-terminordninger (#51): to-måneder is the default with no row,
//! a granted årstermin applies from its valid_from (dated,
//! append-only), the spesifikasjon and mva-melding follow the ordning
//! (skattleggingsperiodeAar in the XML), and periode numbers outside
//! the ordning are refused. Requires DATABASE_URL (skips otherwise).

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

fn salg(dato: chrono::NaiveDate, netto: i64) -> VoucherDraft {
    let entry = |konto: &str, amount: i64, vat: Option<&str>| EntryDraft {
        account_number: konto.into(),
        amount: Ore(amount),
        vat_code: vat.map(str::to_string),
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Salg {dato}"),
        reverses: None,
        entries: vec![
            entry("1920", netto + netto / 4, None),
            entry("3000", -netto, Some("3")),
            entry("2700", -netto / 4, None),
        ],
    }
}

#[tokio::test]
async fn terminordning_styrer_perioder_og_melding() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Termin Ansvarlig"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Liten Omsetning AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let date = |m, d| chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap();
    regnmed_db::post_voucher(&state.pool, company, &salg(date(3, 5), 40_000_00), "test")
        .await
        .unwrap();
    regnmed_db::post_voucher(&state.pool, company, &salg(date(10, 5), 10_000_00), "test")
        .await
        .unwrap();
    let token = idp.token(&sub, "Termin Ansvarlig");
    let base = format!("/companies/{company}");

    // Default: to-måneder, 6 perioder, frister incl. særregelen.
    let (status, info) = request(&state, "GET", &format!("{base}/mva/terminordning"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "body: {info}");
    assert_eq!(info["ordning"], "to-maneder");
    assert_eq!(info["antall_perioder"], 6);
    assert_eq!(info["perioder"][2]["frist"], "2026-08-31", "3. termin: 31. august");
    let (_, report) = request(
        &state,
        "GET",
        &format!("{base}/reports/mva?year=2026&termin=2"),
        &token,
        None,
    )
    .await;
    assert_eq!(report["utgaende_ore"], 10_000_00, "mars-salget i 2. termin");
    assert_eq!(report["frist"], "2026-06-10");
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/reports/mva?year=2026&termin=7"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Skatteetaten innvilger årstermin fra 2026: recorded with vedtak.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/mva/terminordning"),
        &token,
        Some(
            serde_json::json!({
                "ordning": "arlig", "valid_from": "2026-01-01",
                "note": "Vedtak SKE-2025-12345",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/mva/terminordning"),
        &token,
        Some(serde_json::json!({ "ordning": "kvartal", "valid_from": "2026-01-01" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "ukjent ordning avvises");

    let (_, info) = request(&state, "GET", &format!("{base}/mva/terminordning"), &token, None).await;
    assert_eq!(info["ordning"], "arlig");
    assert_eq!(info["antall_perioder"], 1);
    assert_eq!(info["perioder"][0]["frist"], "2027-03-10");
    assert_eq!(info["history"][0]["note"], "Vedtak SKE-2025-12345");

    // The spesifikasjon now covers the whole year in one periode.
    let (status, report) = request(
        &state,
        "GET",
        &format!("{base}/reports/mva?year=2026&termin=1"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {report}");
    assert_eq!(report["label"], "Årstermin 2026");
    assert_eq!(report["start"], "2026-01-01");
    assert_eq!(report["end"], "2026-12-31");
    assert_eq!(report["utgaende_ore"], 12_500_00, "begge salgene");
    let (status, _) = request(
        &state,
        "GET",
        &format!("{base}/reports/mva?year=2026&termin=2"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "årstermin har én periode");

    // The melding carries skattleggingsperiodeAar.
    let (status, xml) = body_text(
        &state,
        &format!("{base}/reports/mva-melding?year=2026&termin=1"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(xml.contains("<skattleggingsperiodeAar>aarlig</skattleggingsperiodeAar>"));
    assert!(!xml.contains("skattleggingsperiodeToMaaneder"));

    // The ordning history is append-only at the DB layer.
    let tamper = sqlx::query("update mva_terminordning set ordning = 'to-maneder' where company_id = $1")
        .bind(company)
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "ordning history is evidence");
}
