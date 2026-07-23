//! The revisor's verification report over the web API: a revisor whose
//! only path to the company is a 'revisjon' engagement (read-only)
//! generates the report; every kontroll passes on a healthy ledger; a
//! planted anchor mismatch turns the verdict; the text download renders;
//! outsiders get 404. Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use common::{TestIdp, test_state, unique_orgnr};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn get_raw(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, String, String) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn revisor_generates_the_verification_report() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };

    // The revisor reaches the company ONLY through her firm's revisjon
    // engagement — the marketplace path, not a direct membership.
    let revisor_sub = format!("test|{}", Uuid::new_v4());
    let revisor = regnmed_db::ensure_person(&state.pool, &revisor_sub, Some("Randi Revisor"), None)
        .await
        .unwrap();
    let firm =
        regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Revisjon & Co AS", "revisjon")
            .await
            .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, revisor, "ansatt")
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Kontrollert AS")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "revisjon")
        .await
        .unwrap();
    let token = idp.token(&revisor_sub, "Randi Revisor");

    // A small ledger with reskontro, a period lock and an anchor.
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("1920", "Bank"),
        ("3000", "Salg"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde AS", None, None)
            .await
            .unwrap();
    let draft = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 5, 10).unwrap(),
        description: "Faktura".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1500".into(),
                amount: Ore(12_500_00),
                vat_code: None,
                description: None,
                party_no: Some(party_no),
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-12_500_00),
                vat_code: None,
                description: None,
                party_no: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &draft, "test")
        .await
        .unwrap();
    regnmed_db::set_period_lock(
        &state.pool,
        company,
        NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        "test",
        false,
    )
    .await
    .unwrap();
    regnmed_db::create_anchor_snapshot(&state.pool)
        .await
        .unwrap()
        .expect("ledger has vouchers");

    // Healthy ledger: every kontroll OK, anchors listed.
    let uri = format!("/companies/{company}/reports/revisjon");
    let (status, _, body) = get_raw(&state, &uri, &token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(report["alle_ok"], true, "{report}");
    let kontroller = report["kontroller"].as_array().unwrap();
    assert_eq!(kontroller.len(), 7);
    for kontroll in kontroller {
        assert_eq!(kontroll["ok"], true, "{kontroll}");
    }
    assert!(
        report["kontroller"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["navn"] == "Reskontro mot hovedbok"
                && k["detalj"].as_str().unwrap().contains("1 reskontrokonto")),
        "{report}"
    );
    assert!(!report["ankere"].as_array().unwrap().is_empty());
    assert_eq!(report["kjede_sekvens"], 1);

    // The text rendering downloads with the verdict stated.
    let (status, content_type, text) =
        get_raw(&state, &format!("{uri}?format=tekst"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/plain"), "{content_type}");
    assert!(text.contains("VERIFIKASJONSRAPPORT"));
    assert!(text.contains("ALLE KONTROLLER OK"));
    assert!(text.contains("Kontrollert AS"));

    // A planted anchor claiming a different head turns the verdict —
    // the report reports, it never hides.
    let fake = Uuid::now_v7();
    sqlx::query("insert into anchor_snapshot (id, root_hash, leaf_count) values ($1, $2, 1)")
        .bind(fake)
        .bind([0xAA_u8; 32].as_slice())
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query(
        "insert into anchor_leaf (snapshot_id, company_id, last_seq, last_hash)
         values ($1, $2, 1, $3)",
    )
    .bind(fake)
    .bind(company)
    .bind([0xAA_u8; 32].as_slice())
    .execute(&state.pool)
    .await
    .unwrap();
    let (_, _, body) = get_raw(&state, &uri, &token).await;
    let report: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(report["alle_ok"], false);
    let forankring = report["kontroller"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["navn"] == "Ekstern forankring")
        .unwrap();
    assert_eq!(forankring["ok"], false);

    // No path to the company → 404, never a hint that it exists.
    let stranger_sub = format!("test|{}", Uuid::new_v4());
    regnmed_db::ensure_person(&state.pool, &stranger_sub, Some("Fremmed"), None)
        .await
        .unwrap();
    let stranger_token = idp.token(&stranger_sub, "Fremmed");
    let (status, _, _) = get_raw(&state, &uri, &stranger_token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
