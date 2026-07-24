//! Timeføring end to end: integer minutes recorded/edited/deleted, the
//! month lock rejects changes (also at the trigger layer) while billing
//! locked hours stays possible, and the fakturagrunnlag becomes an
//! ordinary invoice with the prosjekt dimension — hours marked
//! fakturert one-way. Requires DATABASE_URL (skips otherwise).

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
async fn hours_lock_and_bill() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Konsulent"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Timer AS")
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
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "kunde",
        "Oppdragsgiver AS",
        None,
        None,
    )
    .await
    .unwrap();
    regnmed_db::create_dimension(&state.pool, company, "prosjekt", "P1", "Leveranse")
        .await
        .unwrap();
    let token = idp.token(&sub, "Kari Konsulent");
    let base = format!("/companies/{company}/timesheet");

    // Record 2,5 h billable on P1 (150 min) + 1 h internal.
    let (status, first) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06",
                "minutter": 150,
                "beskrivelse": "Implementasjon",
                "prosjekt": "P1",
                "fakturerbar": true,
                "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {first}");
    let entry_id = first["entry_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07",
                "minutter": 60,
                "beskrivelse": "Internmøte",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Billable without sats is rejected; unknown prosjekt is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07", "minutter": 30, "beskrivelse": "X", "fakturerbar": true,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &state,
        "POST",
        &base,
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-07", "minutter": 30, "beskrivelse": "X", "prosjekt": "P99",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Edit while open: 150 → 180 min.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{entry_id}"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06",
                "minutter": 180,
                "beskrivelse": "Implementasjon",
                "prosjekt": "P1",
                "fakturerbar": true,
                "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Weekly view + summary.
    let (_, week) = request(
        &state,
        "GET",
        &format!("{base}?from=2026-07-06&to=2026-07-12"),
        &token,
        None,
    )
    .await;
    assert_eq!(week["entries"].as_array().unwrap().len(), 2);
    let (_, summary) = request(
        &state,
        "GET",
        &format!("{base}/summary?from=2026-07-01&to=2026-07-31"),
        &token,
        None,
    )
    .await;
    let p1 = summary["prosjekter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["prosjekt"] == "P1")
        .unwrap();
    assert_eq!(p1["minutter"], 180);
    assert_eq!(p1["ufakturert_ore"], 3_600_00, "3 t à 1200 kr");

    // Lock July (admin): edits and inserts in July now fail — including
    // straight at the database (the trigger, not just the API).
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/lock"),
        &token,
        Some(serde_json::json!({ "locked_through": "2026-07-31" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/{entry_id}"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-06", "minutter": 60, "beskrivelse": "krymp",
                "prosjekt": "P1", "fakturerbar": true, "timesats_ore": 1_200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "locked month rejects edits"
    );
    let direct = sqlx::query("delete from time_entry where id = $1")
        .bind(Uuid::parse_str(&entry_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(direct.is_err(), "trigger guards the lock at the DB layer");

    // Billing LOCKED hours is still allowed: fakturagrunnlag → invoice
    // with the prosjekt dimension, entries marked fakturert.
    let (_, unbilled) = request(&state, "GET", &format!("{base}/unbilled"), &token, None).await;
    assert_eq!(unbilled["groups"].as_array().unwrap().len(), 1);
    assert_eq!(unbilled["groups"][0]["minutter"], 180);
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &token,
        Some(serde_json::json!({ "party_no": party_no }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 4_500_00, "3 t à 1200 + 25 % mva");
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);
    // The revenue entry carries the prosjekt dimension.
    let dim_code: Option<String> = sqlx::query_scalar(
        "select d.code from entry e
         join dimension d on d.id = e.prosjekt_id
         join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '3000'",
    )
    .bind(company)
    .fetch_optional(&state.pool)
    .await
    .unwrap();
    assert_eq!(dim_code.as_deref(), Some("P1"));

    // Fakturerte timer are immutable and never rebilled.
    let (_, again) = request(&state, "GET", &format!("{base}/unbilled"), &token, None).await;
    assert!(again["groups"].as_array().unwrap().is_empty());
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoice"),
        &token,
        Some(serde_json::json!({ "party_no": party_no }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "nothing left to bill");
    let tamper = sqlx::query("update time_entry set minutter = 1 where id = $1")
        .bind(Uuid::parse_str(&entry_id).unwrap())
        .execute(&state.pool)
        .await;
    assert!(
        tamper.is_err(),
        "billed hours are immutable at the DB layer"
    );

    // The week view links the hours to their invoice.
    let (_, week) = request(
        &state,
        "GET",
        &format!("{base}?from=2026-07-06&to=2026-07-12"),
        &token,
        None,
    )
    .await;
    let billed = week["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["entry_id"] == entry_id.as_str())
        .unwrap();
    assert_eq!(billed["invoice_no"], 1);
}
