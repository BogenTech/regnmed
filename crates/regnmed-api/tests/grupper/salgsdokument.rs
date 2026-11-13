//! Tilbud → ordre → faktura end to end: gap-free series per kind, the
//! one-way status trapp, lossless conversion with chain links, the
//! ordinary atomic invoice path at the end, and the guards (edit after
//! akseptert, double ordre, double faktura). Requires DATABASE_URL
//! (skips otherwise).

use crate::common::{
    TestIdp, give_parties_address, make_invoiceable, test_state, unique_orgnumber,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, Vec<u8>) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
) -> (StatusCode, serde_json::Value) {
    let (status, bytes) = send(state, method, uri, bearer, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[tokio::test]
async fn quote_order_invoice_chain() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Tilbud & Co AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
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
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (_, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kjøper AS", None, None)
            .await
            .unwrap();
    give_parties_address(&state.pool, company).await;
    let token = idp.token(&sub, "Kari Bokfører");

    // Tilbud 1: utkast, editable.
    let (status, quote) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Prosjektering", "unit_price_ore": 50_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {quote}");
    assert_eq!(quote["doc_no"], 1, "own gap-free series");
    let quote_id = quote["id"].as_str().unwrap().to_string();

    // Edit while utkast: price negotiated down.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/quotes/{quote_id}"),
        &token,
        Some(
            serde_json::json!({
                "lines": [{ "description": "Prosjektering", "unit_price_ore": 45_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The editor reads the lines back as they now stand; another
    // company's id is a 404, like every other read.
    let (status, read) = request(
        &state,
        "GET",
        &format!("/companies/{company}/quotes/{quote_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["lines"].as_array().unwrap().len(), 1);
    assert_eq!(read["lines"][0]["unit_price_ore"], 45_000_00);
    assert_eq!(read["lines"][0]["quantity_milli"], 1000);
    assert_eq!(read["lines"][0]["description"], "Prosjektering");
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/quotes/{}", Uuid::new_v4()),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Tilbud PDF renders with the current state.
    let (status, pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/quotes/{quote_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    assert!(find(&pdf, b"TILBUD").is_some());
    assert!(find(&pdf, b"Tilbudsnr").is_some());
    assert!(
        find(&pdf, b"45 000,00").is_some(),
        "edited price on the PDF"
    );
    assert!(find(&pdf, b"KID").is_none(), "a tilbud is not payable");
    assert!(find(&pdf, b"BETALINGSINFORMASJON").is_none());

    // utkast → sendt → akseptert; avslått/backwards is rejected after.
    for s in ["sendt", "akseptert"] {
        let (status, body) = request(
            &state,
            "POST",
            &format!("/companies/{company}/quotes/{quote_id}/status"),
            &token,
            Some(serde_json::json!({ "status": s }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "→ {s}: {body}");
    }
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/status"),
        &token,
        Some(serde_json::json!({ "status": "avslatt" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "trappen er enveis");

    // Editing an accepted tilbud is rejected.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("/companies/{company}/quotes/{quote_id}"),
        &token,
        Some(
            serde_json::json!({
                "lines": [{ "description": "X", "unit_price_ore": 1, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Akseptert tilbud → ordre (lines copied); a second ordre from the
    // same tilbud is rejected.
    let (status, order) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {order}");
    assert_eq!(order["doc_no"], 1, "ordre series starts at 1");
    let order_id = order["order_id"].as_str().unwrap().to_string();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{quote_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one ordre per tilbud");

    // Ordrebekreftelse PDF.
    let (status, order_pdf) = send(
        &state,
        "GET",
        &format!("/companies/{company}/orders/{order_id}/pdf"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(find(&order_pdf, b"ORDREBEKREFTELSE").is_some());

    // Ordre → faktura through the ordinary path; chain + PDF verified;
    // links carried; second conversion rejected.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders/{order_id}/invoice"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["invoice_no"], 1);
    assert_eq!(issued["gross_ore"], 56_250_00, "45 000 + 25 % mva");
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);
    assert_eq!(
        regnmed_db::verify_attachments(&state.pool, company)
            .await
            .unwrap(),
        1,
        "the invoice got its stored PDF"
    );
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders/{order_id}/invoice"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one ordre → one faktura");

    // The listing carries the whole chain: T-1 → O-1 → faktura 1.
    let (_, orders) = request(
        &state,
        "GET",
        &format!("/companies/{company}/orders"),
        &token,
        None,
    )
    .await;
    let row = &orders["documents"][0];
    assert_eq!(row["status"], "fakturert");
    assert_eq!(row["tilbud_no"], 1);
    assert_eq!(row["invoice_no"], 1);

    // Avslått path: a rejected tilbud is history, not a hole — the next
    // tilbud takes number 3.
    let (_, rejected) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Noe annet", "unit_price_ore": 1_000_00 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(rejected["doc_no"], 2);
    let rejected_id = rejected["id"].as_str().unwrap();
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{rejected_id}/status"),
        &token,
        Some(serde_json::json!({ "status": "avslatt" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes/{rejected_id}/order"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "avslått blir aldri ordre");
    let (_, third) = request(
        &state,
        "POST",
        &format!("/companies/{company}/quotes"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Tredje", "unit_price_ore": 100 }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(third["doc_no"], 3, "rejected tilbud is history, not a hole");

    // A direct ordre (no tilbud) also works.
    let (status, direct) = request(
        &state,
        "POST",
        &format!("/companies/{company}/orders"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "lines": [{ "description": "Hasteordre", "unit_price_ore": 2_000_00, "vat_code": "3" }],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {direct}");
    assert_eq!(direct["doc_no"], 2, "ordre series continues");
}

/// "Deres referanse" (#221) through the API, end to end: where it comes
/// from on each path, and the guards the review of #226 asked for.
#[tokio::test]
async fn the_buyers_reference_travels_with_every_path() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Referanse AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
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
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let (customer_id, party_no) =
        regnmed_db::create_party(&state.pool, company, "kunde", "Kommunen", None, None)
            .await
            .unwrap();
    let (supplier_id, _) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Leverandør AS",
        None,
        None,
    )
    .await
    .unwrap();
    give_parties_address(&state.pool, company).await;
    let token = idp.token(&sub, "Kari Bokfører");
    let c = format!("/companies/{company}");
    let line = serde_json::json!([{ "description": "Tjeneste", "unit_price_ore": 1_000_00, "vat_code": "3" }]);
    let today = chrono::Utc::now().date_naive();
    let invoice = |body: serde_json::Value| {
        let (state, token, c) = (state.clone(), token.clone(), c.clone());
        async move {
            let (status, issued) = request(
                &state,
                "POST",
                &format!("{c}/invoices"),
                &token,
                Some(body.to_string()),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{issued}");
            issued["invoice_id"].as_str().unwrap().to_string()
        }
    };
    let reference_of = |invoice_id: String| {
        let (state, token, c) = (state.clone(), token.clone(), c.clone());
        async move {
            let (status, detail) = request(
                &state,
                "GET",
                &format!("{c}/invoices/{invoice_id}"),
                &token,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{detail}");
            detail["buyer_reference"].clone()
        }
    };
    let base = |reference: Option<&str>| {
        let mut body = serde_json::json!({
            "party_no": party_no, "invoice_date": today, "due_date": today, "lines": line,
        });
        if let Some(r) = reference {
            body["buyer_reference"] = r.into();
        }
        body
    };

    // Issued before the customer has a standing reference: none.
    let early = invoice(base(None)).await;
    assert_eq!(reference_of(early.clone()).await, serde_json::Value::Null);

    // The standing reference is set through the contact endpoint.
    let (status, body) = request(
        &state,
        "PUT",
        &format!("{c}/parties/{customer_id}/contact"),
        &token,
        Some(serde_json::json!({ "buyer_reference": "Innkjøp" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(reference_of(invoice(base(None)).await).await, "Innkjøp");
    assert_eq!(
        reference_of(invoice(base(Some("PO-1"))).await).await,
        "PO-1"
    );

    // A credit note takes the ORIGINAL's reference, even when that was
    // none -- not the customer's standing one it never carried.
    let (status, credit) = request(
        &state,
        "POST",
        &format!("{c}/invoices/{early}/credit-note"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{credit}");
    assert_eq!(
        reference_of(credit["invoice_id"].as_str().unwrap().to_string()).await,
        serde_json::Value::Null
    );

    // Too long is refused, not cut.
    let (status, _) = request(
        &state,
        "POST",
        &format!("{c}/invoices"),
        &token,
        Some(base(Some(&"x".repeat(71))).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A supplier has no "Deres referanse", and the refusal leaves the
    // whole contact update unapplied -- the account number included.
    let (status, _) = request(
        &state,
        "PUT",
        &format!("{c}/parties/{supplier_id}/contact"),
        &token,
        Some(
            serde_json::json!({ "bank_account": "60750595270", "buyer_reference": "x" })
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let account: Option<String> =
        sqlx::query_scalar("select bank_account from party where id = $1")
            .bind(supplier_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(account, None, "ingen halv oppdatering");

    // Tilbud → ordre → faktura carries the tilbud's own reference.
    let (status, quote) = request(
        &state,
        "POST",
        &format!("{c}/quotes"),
        &token,
        Some(serde_json::json!({ "party_no": party_no, "lines": line, "buyer_reference": "Kontrakt 7" }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{quote}");
    let quote_id = quote["id"].as_str().unwrap().to_string();
    for s in ["sendt", "akseptert"] {
        let (status, body) = request(
            &state,
            "POST",
            &format!("{c}/quotes/{quote_id}/status"),
            &token,
            Some(serde_json::json!({ "status": s }).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (_, order) = request(
        &state,
        "POST",
        &format!("{c}/quotes/{quote_id}/order"),
        &token,
        None,
    )
    .await;
    let order_id = order["order_id"].as_str().unwrap().to_string();
    let (status, issued) = request(
        &state,
        "POST",
        &format!("{c}/orders/{order_id}/invoice"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued}");
    assert_eq!(
        reference_of(issued["invoice_id"].as_str().unwrap().to_string()).await,
        "Kontrakt 7"
    );

    // A recurring template carries its own, per contract.
    let (status, created) = request(
        &state,
        "POST",
        &format!("{c}/invoice-templates"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no, "intervall": "manedlig", "neste_dato": today,
                "lines": line, "buyer_reference": "Avtale 3",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let template_id = created["template_id"].as_str().unwrap().to_string();
    let (status, generated) = request(
        &state,
        "POST",
        &format!("{c}/invoice-templates/{template_id}/generate"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{generated}");
    let generated_invoice: Uuid = sqlx::query_scalar(
        "select invoice_id from invoice_template_run
         where template_id = $1 and invoice_id is not null",
    )
    .bind(Uuid::parse_str(&template_id).unwrap())
    .fetch_one(&state.pool)
    .await
    .unwrap();
    assert_eq!(
        reference_of(generated_invoice.to_string()).await,
        "Avtale 3"
    );
}
