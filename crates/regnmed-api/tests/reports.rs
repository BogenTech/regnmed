//! End-to-end tests of the web report endpoints: an accountant with an
//! engagement fetches the mva-report, mva-melding and SAF-T export over
//! HTTP; a person with no path to the company gets 404 (not 403 — no
//! existence leak). Requires DATABASE_URL (skips otherwise).

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

async fn get(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap();
    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

/// A client company with one posted sale in termin 1 2026, and an
/// accountant reaching it through a firm engagement.
async fn seed(state: &AppState, sub: &str) -> Uuid {
    let person = regnmed_db::ensure_person(&state.pool, sub, Some("Kari Kontrolldame"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Rapportklient AS")
        .await
        .unwrap();
    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Tall & Orden AS", "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, person, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "regnskap")
        .await
        .unwrap();

    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bankinnskudd"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let sale = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 1, 20).unwrap(),
        description: "Salg".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(12_500_00),
                vat_code: None,
                description: None,
                party_no: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-10_000_00),
                vat_code: Some("3".into()),
                description: None,
                party_no: None,
            },
            EntryDraft {
                account_number: "2700".into(),
                amount: Ore(-2_500_00),
                vat_code: None,
                description: None,
                party_no: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &sale, "test")
        .await
        .unwrap();
    company
}

#[tokio::test]
async fn engagement_grants_web_reports() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &sub).await;
    let token = idp.token(&sub, "Kari Kontrolldame");

    // Mva-report as JSON.
    let (status, body) = get(
        &state,
        &format!("/companies/{company}/reports/mva?year=2026&termin=1"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["netto_ore"], 2_500_00);
    assert_eq!(report["lines"][0]["code"], "3");
    assert_eq!(report["lines"][0]["grunnlag_ore"], -10_000_00);

    // Mva-melding as XML download.
    let (status, body) = get(
        &state,
        &format!("/companies/{company}/reports/mva-melding?year=2026&termin=1"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let xml = String::from_utf8(body).unwrap();
    assert!(xml.contains("<mvaMeldingDto"));
    assert!(xml.contains("<fastsattMerverdiavgift>2500</fastsattMerverdiavgift>"));

    // SAF-T as XML download; contact defaults to the token's name.
    let (status, body) = get(
        &state,
        &format!("/companies/{company}/reports/saft?year=2026"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let xml = String::from_utf8(body).unwrap();
    assert!(xml.contains("<AuditFile"));
    assert!(xml.contains("<FirstName>Kari</FirstName>"));
    assert!(xml.contains("<LastName>Kontrolldame</LastName>"));
    assert!(xml.contains("<NumberOfEntries>1</NumberOfEntries>"));
}

#[tokio::test]
async fn no_engagement_means_404_and_bad_params_400() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let owner_sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &owner_sub).await;

    // A different, valid user with no path to the company: 404.
    let stranger = idp.token(&format!("test|{}", Uuid::new_v4()), "Ukjent Person");
    let (status, _) = get(
        &state,
        &format!("/companies/{company}/reports/mva?year=2026&termin=1"),
        &stranger,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "no access must read as not-found"
    );

    // Bad termin from an authorized user: 400.
    let owner = idp.token(&owner_sub, "Kari Kontrolldame");
    let (status, _) = get(
        &state,
        &format!("/companies/{company}/reports/mva?year=2026&termin=7"),
        &owner,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
