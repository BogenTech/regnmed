//! Repeterende faktura end to end: a template created over the API
//! (also from an existing invoice), generation through the ordinary
//! gap-free path with periodetekst, catch-up over missed periods,
//! idempotence, sluttdato/deaktivering respected, and the insert-only
//! run log. Requires DATABASE_URL (skips otherwise).

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
async fn recurring_invoices_generate_through_the_ordinary_path() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Husleie AS")
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
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Leietaker AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Kari Bokfører");
    let today: chrono::NaiveDate = sqlx::query_scalar("select current_date")
        .fetch_one(&state.pool)
        .await
        .unwrap();

    // A monthly template two periods behind — generation catches up.
    let start = today - chrono::Months::new(1);
    let (status, created) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoice-templates"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "intervall": "manedlig",
                "neste_dato": start.to_string(),
                "merk_utsendelse": true,
                "lines": [{
                    "description": "Husleie {måned} {år}",
                    "unit_price_ore": 12_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {created}");
    let template_id = created["template_id"].as_str().unwrap().to_string();

    // Bad intervall is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoice-templates"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "intervall": "ukentlig",
                "neste_dato": today.to_string(),
                "lines": [{ "description": "X", "unit_price_ore": 100 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Generate now: both due periods (start and start+1mnd if <= today).
    let (status, generated) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoice-templates/{template_id}/generate"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {generated}");
    let runs = generated["generated"].as_array().unwrap();
    assert!(
        (1..=2).contains(&runs.len()),
        "one or two periods due depending on the date: {generated}"
    );
    assert_eq!(runs[0]["generated_for"], start.to_string());

    // A second generate finds nothing due — idempotent.
    let (status, again) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoice-templates/{template_id}/generate"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(again["generated"].as_array().unwrap().is_empty(), "{again}");

    // The generated invoices are ordinary: gap-free numbers, KID, PDF
    // attachment, interpolated periodetekst, and the chain verifies.
    let (_, invoices) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices"),
        &token,
        None,
    )
    .await;
    let rows = invoices["invoices"].as_array().unwrap();
    assert_eq!(rows.len(), runs.len());
    assert_eq!(rows[0]["invoice_no"], 1);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, runs.len() as i64);
    let attachments = regnmed_db::verify_attachments(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(attachments, runs.len() as i64, "each got its PDF");
    let descriptions: Vec<String> = sqlx::query_scalar(
        "select l.description from invoice_line l
         join invoice i on i.id = l.invoice_id
         where i.company_id = $1 order by i.invoice_no",
    )
    .bind(company)
    .fetch_all(&state.pool)
    .await
    .unwrap();
    assert!(
        descriptions[0].starts_with("Husleie ")
            && !descriptions[0].contains("{måned}")
            && descriptions[0].contains(&start.format("%Y").to_string()),
        "interpolated: {descriptions:?}"
    );

    // The run log shows the generations, marked for utsendelse.
    let (_, log) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoice-templates/{template_id}/runs"),
        &token,
        None,
    )
    .await;
    let log_rows = log["runs"].as_array().unwrap();
    assert_eq!(log_rows.len(), runs.len());
    assert_eq!(log_rows[0]["til_utsendelse"], true);
    assert!(log_rows[0]["invoice_no"].as_i64().is_some());

    // The run log is evidence: UPDATE is rejected.
    let tampering = sqlx::query(
        "update invoice_template_run set til_utsendelse = false where template_id = $1",
    )
    .bind(Uuid::parse_str(&template_id).unwrap())
    .execute(&state.pool)
    .await;
    assert!(tampering.is_err(), "run log must be append-only");

    // Deactivate → nothing generates.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/invoice-templates/{template_id}"),
        &token,
        Some(serde_json::json!({ "active": false }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let outcomes = regnmed_db::generate_due(&state.pool, today + chrono::Months::new(6))
        .await
        .unwrap();
    assert!(
        outcomes
            .iter()
            .all(|o| o.template_id.to_string() != template_id),
        "deactivated template never generates"
    );

    // "Gjenta denne" — a template copied from invoice 1, with sluttdato
    // already passed: nothing is due.
    let (_, invoices) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices"),
        &token,
        None,
    )
    .await;
    let invoice_id = invoices["invoices"][0]["invoice_id"].as_str().unwrap();
    let (status, copied) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoice-templates"),
        &token,
        Some(
            serde_json::json!({
                "from_invoice_id": invoice_id,
                "intervall": "arlig",
                "neste_dato": today.to_string(),
                "slutt_dato": null,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {copied}");
    let copied_id = copied["template_id"].as_str().unwrap();
    // Its line came from the invoice (already-interpolated text).
    let (_, templates) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoice-templates"),
        &token,
        None,
    )
    .await;
    let copy = templates["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["template_id"] == copied_id)
        .unwrap();
    assert_eq!(copy["intervall"], "arlig");
    assert_eq!(copy["sum_netto_ore"], 12_000_00);
}
