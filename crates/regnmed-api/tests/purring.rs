//! Betalingsoppfølging over the web API: an overdue invoice surfaces
//! with its aldersintervall, a gebyrfri påminnelse writes no voucher, a
//! purring with gebyr + rente posts one (chain verifies), the legal
//! guardrails reject bad skritt, and the history is immutable.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

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

#[tokio::test]
async fn purring_loop_with_legal_guardrails() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Purring AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salgsinntekt"),
        ("2700", "Utgående mva"),
        ("3950", "Annen driftsrelatert inntekt"),
        ("8050", "Annen renteinntekt"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Sen Betaler AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .unwrap();

    // 10 000 kr + mva, long overdue (satsdekning fra 2025-01-01).
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-01-10",
                "due_date": "2026-01-24",
                "lines": [{
                    "description": "Konsulentbistand",
                    "unit_price_ore": 10_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();
    // A recently overdue invoice for the young end of aldersfordelingen.
    let (status, fresh) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": (today - chrono::Days::new(20)).to_string(),
                "due_date": (today - chrono::Days::new(5)).to_string(),
                "lines": [{ "description": "Småjobb", "unit_price_ore": 1_000_00 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {fresh}");

    // Forfallsovervåking: both surface, each in its bucket, sums match.
    let (status, overdue) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/overdue"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {overdue}");
    let rows = overdue["invoices"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let old_row = rows.iter().find(|r| r["invoice_no"] == 1).unwrap();
    let fresh_row = rows.iter().find(|r| r["invoice_no"] == 2).unwrap();
    assert_eq!(old_row["bucket"], "30+", "{old_row}");
    assert_eq!(fresh_row["bucket"], "1-14", "{fresh_row}");
    assert_eq!(overdue["buckets"]["30+"], 12_500_00);
    assert_eq!(overdue["buckets"]["1-14"], 1_000_00);

    let reminders_uri = format!("/companies/{company}/invoices/{invoice_id}/reminders");

    // Gebyrfri påminnelse: registered, but nothing posted.
    let frist = (today + chrono::Days::new(14)).to_string();
    let (status, paminnelse) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(serde_json::json!({ "steg": "paminnelse", "frist_date": frist }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {paminnelse}");
    assert_eq!(paminnelse["voucher"], serde_json::Value::Null);
    assert_eq!(paminnelse["gebyr_ore"], 0);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(
        report.vouchers_checked, 2,
        "no krav voucher for a påminnelse"
    );

    // Preview a purring with gebyr + rente: computed, rendered, not written.
    let (status, preview) = request(
        &state,
        "POST",
        &format!("{reminders_uri}?preview=true"),
        &token,
        Some(
            serde_json::json!({
                "steg": "purring",
                "frist_date": frist,
                "gebyr_ore": 3800,
                "med_rente": true,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {preview}");
    assert_eq!(preview["maks_gebyr_ore"], 3800, "purregebyr_maks 2026");
    assert!(preview["rente_ore"].as_i64().unwrap() > 0);
    let document = preview["document"].as_str().unwrap();
    assert!(document.starts_with("PURRING\n"));
    assert!(document.contains("forsinkelsesrenteloven"));
    let (_, history) = request(&state, "GET", &reminders_uri, &token, None).await;
    assert_eq!(
        history["reminders"].as_array().unwrap().len(),
        1,
        "preview wrote nothing"
    );

    // Gebyr over maksimalsatsen is rejected.
    let (status, over) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(
            serde_json::json!({ "steg": "purring", "frist_date": frist, "gebyr_ore": 3900 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {over}");

    // The real purring: gebyr + rente become one posted voucher.
    let (status, purring) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(
            serde_json::json!({
                "steg": "purring",
                "frist_date": frist,
                "gebyr_ore": 3800,
                "med_rente": true,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {purring}");
    assert!(
        purring["voucher"].as_str().is_some(),
        "krav posted: {purring}"
    );
    let rente_ore = purring["rente_ore"].as_i64().unwrap();
    assert!(rente_ore > 0);
    assert_eq!(
        purring["total_ore"].as_i64().unwrap(),
        12_500_00 + 3800 + rente_ore
    );
    assert_eq!(purring["kid"], issued["kid"], "KID follows the invoice");
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3, "chain verifies with the krav");

    // The krav is an open post on the same customer's reskontro; the
    // invoice's own remaining is untouched.
    let (_, overdue) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/overdue"),
        &token,
        None,
    )
    .await;
    let old_row = overdue["invoices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["invoice_no"] == 1)
        .unwrap()
        .clone();
    assert_eq!(old_row["remaining_ore"], 12_500_00);
    assert_eq!(old_row["last_steg"], "purring");

    // Purretrappen er enveis: påminnelse etter purring avvises.
    let (status, back) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(serde_json::json!({ "steg": "paminnelse", "frist_date": frist }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {back}");

    // Inkassovarsel with under 14 days frist is rejected …
    let (status, short) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(
            serde_json::json!({
                "steg": "inkassovarsel",
                "frist_date": (today + chrono::Days::new(13)).to_string(),
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {short}");
    // … and carries the lovtekst when the frist is lawful.
    let (status, varsel) = request(
        &state,
        "POST",
        &reminders_uri,
        &token,
        Some(
            serde_json::json!({
                "steg": "inkassovarsel",
                "frist_date": (today + chrono::Days::new(14)).to_string(),
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {varsel}");
    assert!(
        varsel["document"]
            .as_str()
            .unwrap()
            .contains("inkassoloven §9")
    );

    // History: three skritt, oldest first, stored document downloadable.
    let (_, history) = request(&state, "GET", &reminders_uri, &token, None).await;
    let reminders = history["reminders"].as_array().unwrap();
    assert_eq!(reminders.len(), 3);
    assert_eq!(reminders[0]["steg"], "paminnelse");
    assert_eq!(reminders[1]["steg"], "purring");
    assert_eq!(reminders[2]["steg"], "inkassovarsel");
    let reminder_id = reminders[1]["reminder_id"].as_str().unwrap();
    let (status, _) = request(
        &state,
        "GET",
        &format!("{reminders_uri}/{reminder_id}?format=tekst"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The history is evidence: UPDATE is rejected by the trigger.
    let tampering = sqlx::query("update invoice_reminder set gebyr_ore = 0 where id = $1")
        .bind(Uuid::parse_str(reminder_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(tampering.is_err(), "invoice_reminder must be append-only");
}
