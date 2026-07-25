//! Produktregister og enkelt varelager end to end: register values are
//! COPIED onto document lines at issue (register edits never touch
//! issued documents), salg movements follow invoicing atomically
//! (kreditnota returns stock), the movement log is immutable at the DB
//! layer, and varetelling adjusts quantities AND posts the value change
//! as an ordinary voucher. Requires DATABASE_URL (skips otherwise).

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

fn inventory_row<'a>(body: &'a serde_json::Value, nummer: &str) -> &'a serde_json::Value {
    body["inventory"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["nummer"] == nummer)
        .unwrap()
}

#[tokio::test]
async fn products_stock_and_count() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Vare Handler"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Handel AS")
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
        ("1460", "Varelager"),
        ("4390", "Beholdningsendring"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kjøper AS", None, None)
            .await
            .unwrap();
    let token = idp.token(&sub, "Vare Handler");
    let base = format!("/companies/{company}");

    // Register: one lagerført vare, duplicate nummer rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/products"),
        &token,
        Some(
            serde_json::json!({
                "nummer": "V1", "navn": "Vare", "salgspris_ore": 500_00,
                "vat_code": "3", "lagerfort": true,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/products"),
        &token,
        Some(
            serde_json::json!({ "nummer": "V1", "navn": "Dublett", "salgspris_ore": 1 })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "duplicate nummer");

    // Varekjøp: 10 stk à 200 kr anskaffelseskost.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/inventory/movements"),
        &token,
        Some(
            serde_json::json!({
                "produkt": "V1", "dato": "2026-07-01", "kind": "kjop",
                "antall_milli": 10_000, "kostpris_ore": 200_00,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, inv) = request(&state, "GET", &format!("{base}/inventory"), &token, None).await;
    assert_eq!(inventory_row(&inv, "V1")["antall_milli"], 10_000);
    assert_eq!(inventory_row(&inv, "V1")["verdi_ore"], 2_000_00);
    assert_eq!(inventory_row(&inv, "V1")["gjennomsnitt_ore"], 200_00);

    // Justering requires a note (DB check constraint).
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/inventory/movements"),
        &token,
        Some(
            serde_json::json!({
                "produkt": "V1", "dato": "2026-07-01", "kind": "justering", "antall_milli": -1_000,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "justering without note");

    // Free-text line without price is rejected; product line needs
    // neither description nor price.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "invoice_date": "2026-07-02", "due_date": "2026-07-16",
                "lines": [{ "description": "Uten pris" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "invoice_date": "2026-07-02", "due_date": "2026-07-16",
                "lines": [{ "produkt": "V1", "quantity_milli": 2_000 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 1_250_00, "2 × 500 kr + 25 % mva");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();

    // The line is a COPY of the register values, and carries the ref.
    let line = sqlx::query_as::<_, (String, i64, Option<String>, Option<Uuid>)>(
        "select l.description, l.unit_price_ore, l.vat_code, l.product_id
         from invoice_line l where l.invoice_id = $1",
    )
    .bind(Uuid::parse_str(&invoice_id).unwrap())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(line.0, "Vare");
    assert_eq!(line.1, 500_00);
    assert_eq!(line.2.as_deref(), Some("3"));
    assert!(line.3.is_some());

    // Stock moved with the invoice, linked to it.
    let (_, inv) = request(&state, "GET", &format!("{base}/inventory"), &token, None).await;
    assert_eq!(inventory_row(&inv, "V1")["antall_milli"], 8_000);
    assert_eq!(inventory_row(&inv, "V1")["verdi_ore"], 1_600_00);
    let (_, movements) = request(
        &state,
        "GET",
        &format!("{base}/inventory/movements?produkt=V1"),
        &token,
        None,
    )
    .await;
    let salg = movements["movements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "salg")
        .unwrap();
    assert_eq!(salg["antall_milli"], -2_000);
    assert_eq!(salg["invoice_no"], 1);

    // Register edits never touch issued documents; deactivated products
    // cannot be put on new ones.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/products/V1"),
        &token,
        Some(serde_json::json!({ "salgspris_ore": 600_00, "aktiv": false }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "invoice_date": "2026-07-03", "due_date": "2026-07-17",
                "lines": [{ "produkt": "V1" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "deactivated product");
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{base}/products/V1"),
        &token,
        Some(serde_json::json!({ "aktiv": true }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Kreditnota returns the stock at gjennomsnittskost.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/invoices/{invoice_id}/credit-note"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, inv) = request(&state, "GET", &format!("{base}/inventory"), &token, None).await;
    assert_eq!(inventory_row(&inv, "V1")["antall_milli"], 10_000);
    assert_eq!(inventory_row(&inv, "V1")["verdi_ore"], 2_000_00);

    // Movement log and product identity are immutable at the DB layer.
    let tamper =
        sqlx::query("update inventory_movement set antall_milli = 1 where company_id = $1")
            .bind(company)
            .execute(&state.pool)
            .await;
    assert!(tamper.is_err(), "movements are append-only");
    let tamper = sqlx::query("delete from inventory_movement where company_id = $1")
        .bind(company)
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "movements are undeletable");
    let tamper = sqlx::query("update product set nummer = 'X9' where company_id = $1")
        .bind(company)
        .execute(&state.pool)
        .await;
    assert!(tamper.is_err(), "nummer is permanent");

    // Tilbud → ordre → faktura carries the product reference; the copy
    // is taken when the tilbud line is written (new price 600 kr).
    let (status, quote) = request(
        &state,
        "POST",
        &format!("{base}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "produkt": "V1", "quantity_milli": 3_000 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {quote}");
    let quote_id = quote["id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/quotes/{quote_id}/status"),
        &token,
        Some(serde_json::json!({ "status": "akseptert" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, order) = request(
        &state,
        "POST",
        &format!("{base}/quotes/{quote_id}/order"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = order["order_id"].as_str().unwrap().to_string();
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{base}/orders/{order_id}/invoice"),
        &token,
        Some("{}".to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["gross_ore"], 2_250_00, "3 × 600 kr + 25 % mva");
    let (_, inv) = request(&state, "GET", &format!("{base}/inventory"), &token, None).await;
    assert_eq!(inventory_row(&inv, "V1")["antall_milli"], 7_000);
    assert_eq!(inventory_row(&inv, "V1")["verdi_ore"], 1_400_00);

    // Varetelling: counted 6,5 → justering −0,5; the value difference
    // against the (empty) 1460 saldo is posted as an ordinary voucher.
    let (status, telling) = request(
        &state,
        "POST",
        &format!("{base}/inventory/count"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-20",
                "linjer": [{ "produkt": "V1", "talt_milli": 6_500 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {telling}");
    assert_eq!(telling["verdi_ore"], 1_300_00, "6,5 à 200 kr");
    assert_eq!(telling["bokfort_ore"], 0);
    assert!(telling["voucher"].is_string());
    let lager_saldo: i64 = sqlx::query_scalar(
        "select coalesce(sum(e.amount_ore), 0)::bigint
         from entry e join account a on a.id = e.account_id
         where a.company_id = $1 and a.number = '1460'",
    )
    .bind(company)
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(lager_saldo, 1_300_00, "lager på balansen = telleverdien");

    // Counting the same numbers again changes nothing and posts nothing.
    let (status, telling) = request(
        &state,
        "POST",
        &format!("{base}/inventory/count"),
        &token,
        Some(
            serde_json::json!({
                "dato": "2026-07-21",
                "linjer": [{ "produkt": "V1", "talt_milli": 6_500 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(telling["voucher"].is_null(), "no diff, no voucher");

    // The whole chain still verifies: faktura, kreditnota,
    // ordre-faktura, varetelling.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 4);
}
