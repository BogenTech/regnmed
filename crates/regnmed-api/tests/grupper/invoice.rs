//! End-to-end faktura over the web API: issue an invoice (gap-free
//! number, valid KID, automatic ledger + reskontro posting), watch an
//! OCR payment identify it by KID, credit it, and verify the chain.
//! Requires DATABASE_URL (skips otherwise).

use crate::common::{
    TestIdp, give_parties_address, make_invoiceable, test_state, unique_orgnumber,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn request(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<String>,
    json: bool,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    if json {
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

fn record(parts: &[&str]) -> String {
    let line: String = parts.concat();
    assert_eq!(line.len(), 80, "fixture record must be 80 chars: {line}");
    line
}

/// An OCR file paying `amount` (17-digit øre) on `kid` (padded to 25).
fn ocr_file(transmission: &str, kid: &str, amount_17: &str) -> String {
    let kid_field = format!("{kid:>25}");
    [
        record(&[
            "NY000010",
            "00111222",
            transmission,
            "00008080",
            &"0".repeat(49),
        ]),
        record(&[
            "NY090020",
            "000988555",
            "0000001",
            "99991042764",
            &"0".repeat(45),
        ]),
        record(&[
            "NY091030", "0000001", "150226", "00", "20", "0", "00001", "0", amount_17, &kid_field,
            "000000",
        ]),
        record(&[
            "NY090088",
            "00000001",
            "00000003",
            amount_17,
            "150226",
            "150226",
            "150226",
            &"0".repeat(21),
        ]),
        record(&[
            "NY000089",
            "00000001",
            "00000005",
            amount_17,
            "150226",
            &"0".repeat(33),
        ]),
    ]
    .join("\n")
}

#[tokio::test]
async fn invoice_to_payment_to_credit_note_loop() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Faktura AS")
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
        ("1920", "Bankinnskudd"),
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
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde & Co AS", None, None)
            .await
            .unwrap();
    give_parties_address(&state.pool, company).await;
    let token = idp.token(&sub, "Kari Bokfører");

    // Issue: 2,5 timer à 4 000 kr + 25 % mva = 12 500 kr gross.
    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-02-01",
                "due_date": "2026-02-15",
                "lines": [{
                    "description": "Konsulentbistand",
                    "quantity_milli": 2500,
                    "unit_price_ore": 4_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {issued}");
    assert_eq!(issued["invoice_no"], 1, "gap-free numbering starts at 1");
    assert_eq!(issued["net_ore"], 10_000_00);
    assert_eq!(issued["vat_ore"], 2_500_00);
    assert_eq!(issued["gross_ore"], 12_500_00);
    let kid = issued["kid"].as_str().unwrap().to_string();
    assert!(regnmed_core::kid::is_valid_mod10(&kid));
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();

    // A bad invoice attempt (unknown vat code) must not burn a number.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-02-02",
                "due_date": "2026-02-16",
                "lines": [{ "description": "X", "unit_price_ore": 100, "vat_code": "99" }],
            })
            .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The ledger posted and the chain holds.
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 1);

    // The invoice is open for its full gross.
    let (_, list) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices?open=true"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(list["invoices"][0]["remaining_ore"], 12_500_00);
    assert_eq!(list["invoices"][0]["kid"], kid);

    // The portal's invoice page: header, derived status and the stored
    // lines. Due 2026-02-15, so it is overdue by now.
    let (status, detail) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{invoice_id}"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {detail}");
    assert_eq!(detail["status"], "overdue");
    assert_eq!(detail["net_ore"], 10_000_00);
    assert_eq!(detail["vat_ore"], 2_500_00);
    assert_eq!(detail["lines"][0]["description"], "Konsulentbistand");
    assert_eq!(detail["lines"][0]["quantity_milli"], 2500);
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{}", Uuid::new_v4()),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another id is no invoice of ours"
    );

    // The customer register (the portal's Kunder): the overdue amount uses
    // the invoice list's status, search and sort run in SQL, and one
    // customer can be read on its own.
    let get = |path: String| {
        let state = &state;
        let token = &token;
        async move { request(state, "GET", &path, token, None, false).await }
    };
    let (_, customers) = get(format!(
        "/companies/{company}/parties?kind=kunde&q=co&sort=overdue&desc=true&limit=10"
    ))
    .await;
    assert_eq!(customers["total"], 1, "body: {customers}");
    let customer = &customers["parties"][0];
    assert_eq!(customer["name"], "Kunde & Co AS");
    assert_eq!(customer["overdue_ore"], 12_500_00);
    assert_eq!(customer["saldo_ore"], 12_500_00);
    let customer_id = customer["party_id"].as_str().unwrap().to_string();
    let (status, one) = get(format!("/companies/{company}/parties/{customer_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["party_no"], party_no.as_str());
    assert_eq!(one["overdue_ore"], 12_500_00);
    // A page sorted by something else gets the overdue amount for its own
    // rows; an unpaged listing (the pickers) does not pay for it.
    let (_, by_name) = get(format!(
        "/companies/{company}/parties?kind=kunde&sort=name&limit=10"
    ))
    .await;
    assert_eq!(by_name["parties"][0]["overdue_ore"], 12_500_00);
    let (_, unpaged) = get(format!("/companies/{company}/parties?kind=kunde")).await;
    assert!(
        unpaged["parties"][0]["overdue_ore"].is_null(),
        "body: {unpaged}"
    );
    let (_, theirs) = get(format!(
        "/companies/{company}/invoices?party_no={party_no}&limit=10"
    ))
    .await;
    assert_eq!(theirs["total"], 1);
    let (_, nobodys) = get(format!(
        "/companies/{company}/invoices?party_no=99999&limit=10"
    ))
    .await;
    assert_eq!(nobodys["total"], 0);
    let (status, _) = get(format!("/companies/{company}/parties?sort=bogus&limit=10")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Ny kunde with contact info in one request; a bad account number is
    // refused BEFORE the customer exists, so no half-made customer remains.
    let create = |body: serde_json::Value| {
        let state = &state;
        let token = &token;
        async move {
            request(
                state,
                "POST",
                &format!("/companies/{company}/parties"),
                token,
                Some(body.to_string()),
                true,
            )
            .await
        }
    };
    let (status, _) = create(serde_json::json!({
        "kind": "kunde", "name": "Halvferdig AS", "bank_account": "1234.56.78901"
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, none) = get(format!(
        "/companies/{company}/parties?q=halvferdig&limit=10"
    ))
    .await;
    assert_eq!(none["total"], 0, "the refused customer was never created");
    let (status, made) = create(serde_json::json!({
        "kind": "kunde", "name": "Ny Kunde AS", "email": "post@nykunde.invalid",
        "address": "Gata 1, 0150 Oslo"
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "body: {made}");
    let (_, made) = get(format!(
        "/companies/{company}/parties/{}",
        made["party_id"].as_str().unwrap()
    ))
    .await;
    assert_eq!(made["email"], "post@nykunde.invalid");
    assert_eq!(made["address"], "Gata 1, 0150 Oslo");

    // An OCR payment on the invoice's KID identifies the invoice.
    let transmission = format!("{:07}", rand_7());
    let file = ocr_file(&transmission, &kid, "00000000001250000");
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/ocr/files?account=1920"),
        &token,
        Some(file),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, payments) = request(
        &state,
        "GET",
        &format!("/companies/{company}/ocr/payments"),
        &token,
        None,
        false,
    )
    .await;
    let payment = payments["payments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["kid"] == kid.as_str())
        .expect("payment listed");
    assert_eq!(payment["invoice_no"], 1, "KID resolved to the invoice");

    // Credit note: negates, posts, auto-matches — nothing open afterwards.
    let (status, credit) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/credit-note"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {credit}");
    assert_eq!(credit["invoice_no"], 2, "no gap despite the failed attempt");
    assert_eq!(credit["gross_ore"], -12_500_00);

    let (_, open) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices?open=true"),
        &token,
        None,
        false,
    )
    .await;
    assert!(
        open["invoices"].as_array().unwrap().is_empty(),
        "invoice and kreditnota settle each other: {open}"
    );

    // The list's filters: status and counts are derived in SQL, newest
    // first once a limit is given, and search matches number or name.
    let list = |query: &'static str| {
        let state = &state;
        let token = &token;
        async move {
            request(
                state,
                "GET",
                &format!("/companies/{company}/invoices?{query}"),
                token,
                None,
                false,
            )
            .await
        }
    };
    let (_, credit_view) = list("status=credit&limit=50").await;
    assert_eq!(credit_view["total"], 2);
    assert_eq!(
        credit_view["invoices"][0]["status"], "credit_note",
        "newest first"
    );
    assert_eq!(credit_view["invoices"][1]["status"], "credited");
    assert_eq!(credit_view["counts"]["credited"], 1);
    assert_eq!(credit_view["counts"]["credit_note"], 1);
    let (_, open_view) = list("status=open&limit=50").await;
    assert_eq!(open_view["total"], 0, "nothing open after the kreditnota");
    let (_, by_number) = list("q=1&limit=50").await;
    assert_eq!(by_number["total"], 1);
    assert_eq!(by_number["invoices"][0]["invoice_no"], 1);
    let (_, by_name) = list("q=kunde%20%26&limit=50").await;
    assert_eq!(by_name["total"], 2, "case-insensitive part of the name");
    let (_, paged) = list("limit=1&offset=1").await;
    assert_eq!(paged["total"], 2);
    assert_eq!(paged["invoices"][0]["invoice_no"], 1);
    let (_, keyset) = list("limit=50&before=2").await;
    assert_eq!(keyset["invoices"].as_array().unwrap().len(), 1);
    assert_eq!(keyset["invoices"][0]["invoice_no"], 1);
    assert_eq!(keyset["total"], 2, "total ignores the page position");
    let (_, nothing) = list("status=open&limit=50").await;
    assert_eq!(
        nothing["counts"]["credited"], 1,
        "an empty page still counts"
    );
    let (_, wildcard) = list("q=%25&limit=50").await;
    assert_eq!(
        wildcard["total"], 0,
        "% in a search is text, not a wildcard"
    );
    let (status, _) = list("status=bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Double-crediting is rejected; the chain still verifies (2 vouchers).
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices/{invoice_id}/credit-note"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 2);
}

/// A payment recorded by hand on the invoice page: posted and matched in
/// one transaction, part payments allowed, never more than is open — and
/// two people recording the same payment at once settle it only once.
#[tokio::test]
async fn a_payment_recorded_by_hand_settles_the_invoice() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Kari Bokfører"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Innbetaling AS")
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
        ("1900", "Kontanter"),
        ("1920", "Bankinnskudd"),
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
        regnmed_db::create_party(&state.pool, company, "kunde", "Kunde & Co AS", None, None)
            .await
            .unwrap();
    give_parties_address(&state.pool, company).await;
    let token = idp.token(&sub, "Kari Bokfører");

    let (status, issued) = request(
        &state,
        "POST",
        &format!("/companies/{company}/invoices"),
        &token,
        Some(
            serde_json::json!({
                "party_no": party_no,
                "invoice_date": "2026-02-01",
                "due_date": "2026-02-15",
                "lines": [{
                    "description": "Konsulentbistand",
                    "quantity_milli": 1000,
                    "unit_price_ore": 10_000_00,
                    "vat_code": "3",
                }],
            })
            .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued}");
    let invoice_id = issued["invoice_id"].as_str().unwrap().to_string();
    let pay = {
        let (state, token) = (state.clone(), token.clone());
        let uri = format!("/companies/{company}/invoices/{invoice_id}/payments");
        move |body: serde_json::Value| {
            let (state, token, uri) = (state.clone(), token.clone(), uri.clone());
            async move { request(&state, "POST", &uri, &token, Some(body.to_string()), true).await }
        }
    };

    // A part payment in cash.
    let (status, paid) = pay(serde_json::json!({
        "dato": "2026-03-01", "belop_ore": 5_000_00, "konto": "1900"
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{paid}");
    let (_, detail) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{invoice_id}"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(detail["remaining_ore"], 7_500_00);
    assert_eq!(detail["status"], "overdue");
    assert!(detail["voucher_id"].is_string());
    let settlements = detail["settlements"].as_array().unwrap();
    assert_eq!(settlements.len(), 1);
    assert_eq!(settlements[0]["amount_ore"], 5_000_00);
    assert_eq!(settlements[0]["voucher_id"], paid["voucher_id"]);
    assert_eq!(settlements[0]["date"], "2026-03-01");

    // Refusals, each with a message the user can act on.
    for (body, needle) in [
        (
            serde_json::json!({"dato": "2026-03-02", "belop_ore": 10_000_00}),
            "7500,00",
        ),
        (
            serde_json::json!({"dato": "2026-03-02", "belop_ore": 100, "konto": "3000"}),
            "19xx",
        ),
        (
            serde_json::json!({"dato": "2999-01-01", "belop_ore": 100}),
            "fram i tid",
        ),
        (
            serde_json::json!({"dato": "2026-03-02", "belop_ore": 0}),
            "større enn null",
        ),
    ] {
        let (status, error) = pay(body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(error.to_string().contains(needle), "{body}: {error}");
    }

    // Several people record the rest at once: exactly one wins, the
    // others are told nothing is open. (The guarantee is the party lock
    // in register_invoice_payment; this harness does not reliably make
    // the attempts overlap, so the test pins the outcome, not the race.)
    let rest = serde_json::json!({"dato": "2026-03-05", "belop_ore": 7_500_00});
    let attempts = futures_util::future::join_all((0..6).map(|_| {
        let pay = pay.clone();
        let rest = rest.clone();
        tokio::spawn(async move { pay(rest).await.0 })
    }))
    .await;
    let ok = attempts
        .iter()
        .filter(|s| *s.as_ref().unwrap() == StatusCode::OK)
        .count();
    assert_eq!(ok, 1, "{attempts:?}");
    let (_, detail) = request(
        &state,
        "GET",
        &format!("/companies/{company}/invoices/{invoice_id}"),
        &token,
        None,
        false,
    )
    .await;
    assert_eq!(detail["status"], "paid");
    assert_eq!(detail["remaining_ore"], 0);
    assert_eq!(detail["settlements"].as_array().unwrap().len(), 2);

    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 3);
}

fn rand_7() -> u32 {
    u32::from_be_bytes(Uuid::new_v4().as_bytes()[..4].try_into().unwrap()) % 10_000_000
}
