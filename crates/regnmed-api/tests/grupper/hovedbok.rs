//! Hovedbok over the web API (docs/hovedbok.md): the kontoplan lists
//! company accounts WITH computed balances plus the standard catalog;
//! accounts are added from the catalog (name resolved) or as custom
//! numbers (name required); the number is permanent but name/active are
//! editable; manual vouchers post through every existing guard; and an
//! active attestering policy closes the manual side door. Requires
//! DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn call(
    state: &AppState,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"));
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state.clone())
        .oneshot(builder.body(body).unwrap())
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

fn account<'a>(list: &'a serde_json::Value, number: &str) -> &'a serde_json::Value {
    list["kontoer"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["number"] == number)
        .unwrap_or_else(|| panic!("konto {number} in kontoplan"))
}

#[tokio::test]
async fn kontoplan_and_manual_vouchers_end_to_end() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Bo Bokfører"), None)
        .await
        .unwrap();
    let leser_sub = format!("test|{}", Uuid::new_v4());
    let leser = regnmed_db::ensure_person(&state.pool, &leser_sub, Some("Lise Leser"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Hovedbok AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "bokforing")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, leser, "les")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    let token = idp.token(&sub, "Bo Bokfører");
    let leser_token = idp.token(&leser_sub, "Lise Leser");
    let base = format!("/companies/{company}");

    // The catalog rides along even when the company has zero accounts —
    // that is what makes "every code one keystroke away" true on day 1.
    let (status, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(list["standard"].as_array().unwrap().len() > 200);
    assert!(
        list["standard"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["number"] == "1920")
    );

    // Add from the catalog: the standard name is resolved server-side.
    let (status, created) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1920" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert!(!created["name"].as_str().unwrap().is_empty());

    // A custom number outside the catalog needs its own name...
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1921" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // ...and with one it is a first-class account.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1921", "name": "Skattetrekkskonto (egen)" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Duplicates are refused, not silently merged.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &token,
        Some(json!({ "number": "1920" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Manual bilag through the new endpoint — with an account that does
    // not exist yet, refused loudly; then a balanced one that posts.
    let (status, body) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Feil konto",
            "lines": [
                { "account": "1920", "amount_ore": 5000_00 },
                { "account": "8888", "amount_ore": -5000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, posted) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Overføring til skattetrekk",
            "lines": [
                { "account": "1921", "amount_ore": 5000_00 },
                { "account": "1920", "amount_ore": -5000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert!(posted["voucher"].as_str().unwrap().starts_with("2026-"));

    // The kontoplan carries computed balances now.
    let (_, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(account(&list, "1921")["saldo_ore"], 5000_00);
    assert_eq!(account(&list, "1920")["saldo_ore"], -5000_00);
    assert_eq!(account(&list, "1921")["posteringer"], 1);

    // Rename is free (the name is master data); the deactivated account
    // refuses NEW postings while history stands.
    let (status, _) = call(
        &state,
        "PUT",
        &format!("{base}/accounts/1921"),
        &token,
        Some(json!({ "name": "Skattetrekk", "active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&state, "GET", &format!("{base}/accounts"), &token, None).await;
    assert_eq!(account(&list, "1921")["name"], "Skattetrekk");
    assert_eq!(account(&list, "1921")["active"], false);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-02",
            "description": "Mot deaktivert konto",
            "lines": [
                { "account": "1921", "amount_ore": 100 },
                { "account": "1920", "amount_ore": -100 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // les reads the kontoplan but changes nothing — VALID bodies, so the
    // guard is what answers (docs/auth.md: a 422 measures nothing).
    let (status, _) = call(
        &state,
        "GET",
        &format!("{base}/accounts"),
        &leser_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/accounts"),
        &leser_token,
        Some(json!({ "number": "6300" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &leser_token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Leser prøver seg",
            "lines": [
                { "account": "1920", "amount_ore": 100 },
                { "account": "1921", "amount_ore": -100 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn manual_posting_respects_the_attestering_policy() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Attestert Ane"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Policy AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [("1920", "Bank"), ("6300", "Leie lokale")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_policy(
        &state.pool,
        company,
        true,
        Some(10_000_00),
        None,
        None,
        "test",
    )
    .await
    .unwrap();
    let token = idp.token(&sub, "Attestert Ane");
    let base = format!("/companies/{company}");

    // At/over the grense the manual path is CLOSED — attestation lives
    // on inbox documents, and a side door would defeat #47.
    let (status, body) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Stor husleie",
            "lines": [
                { "account": "6300", "amount_ore": 10_000_00 },
                { "account": "1920", "amount_ore": -10_000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("attestering"),
        "{body}"
    );

    // Under the grense the manual path stays open.
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-03-01",
            "description": "Liten kostnad",
            "lines": [
                { "account": "6300", "amount_ore": 500_00 },
                { "account": "1920", "amount_ore": -500_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn voucher_listing_pages_and_filters_server_side() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Paige Pager"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Sidevis AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "bokforing")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("6800", "Kontorkostnad"),
        ("6300", "Leie"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let token = idp.token(&sub, "Paige Pager");
    let base = format!("/companies/{company}");

    // 24 kontor + 1 leie = 25 vouchers in 2026.
    for i in 1..=24 {
        let (status, _) = call(
            &state,
            "POST",
            &format!("{base}/vouchers"),
            &token,
            Some(json!({
                "journal_code": "GL",
                "date": format!("2026-04-{:02}", (i % 28) + 1),
                "description": format!("Kontorkostnad {i}"),
                "lines": [
                    { "account": "6800", "amount_ore": i * 100 },
                    { "account": "1920", "amount_ore": -(i * 100) },
                ],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL",
            "date": "2026-05-01",
            "description": "Husleie mai",
            "lines": [
                { "account": "6300", "amount_ore": 9_000_00 },
                { "account": "1920", "amount_ore": -9_000_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Page 1: newest 20 of 25, WITH lines; page 2: the remaining 5.
    let (status, side1) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&from=2026-01-01&to=2026-12-31&limit=20&offset=0"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(side1["total"], 25);
    assert_eq!(side1["vouchers"].as_array().unwrap().len(), 20);
    // Newest first: the husleie voucher (posted last) leads, lines attached.
    assert_eq!(side1["vouchers"][0]["description"], "Husleie mai");
    assert_eq!(side1["vouchers"][0]["lines"].as_array().unwrap().len(), 2);
    let (_, side2) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&from=2026-01-01&to=2026-12-31&limit=20&offset=20"),
        &token,
        None,
    )
    .await;
    assert_eq!(side2["vouchers"].as_array().unwrap().len(), 5);

    // The filter runs server-side: by description text...
    let (_, treff) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?lines=true&sok=husleie"),
        &token,
        None,
    )
    .await;
    assert_eq!(treff["total"], 1);
    assert_eq!(treff["vouchers"][0]["description"], "Husleie mai");
    // ...and by account number on the LINES, which headers alone can't see.
    let (_, treff) = call(
        &state,
        "GET",
        &format!("{base}/vouchers?sok=6300"),
        &token,
        None,
    )
    .await;
    assert_eq!(treff["total"], 1);

    // Without parameters the old contract stands: headers only, no lines key.
    let (status, alle) = call(&state, "GET", &format!("{base}/vouchers"), &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alle["vouchers"].as_array().unwrap().len(), 25);
    assert!(alle["vouchers"][0].get("lines").is_none());
}

/// A correction is a reversal (bokføringsloven § 9): every line mirrored,
/// linked to the original, a reskontro line closed against its mirror in
/// the same transaction — and only once, never of a reversal, and never
/// of a voucher another flow owns.
#[tokio::test]
async fn a_voucher_is_corrected_by_a_linked_reversal() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Rita Retting"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Retting AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "bokforing")
        .await
        .unwrap();
    for (code, name) in [("GL", "Hovedbok"), ("IMP", "Import")] {
        regnmed_db::ensure_journal(&state.pool, company, code, name)
            .await
            .unwrap();
    }
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("3000", "Salg"),
        ("1920", "Bank"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_subledger(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    let token = idp.token(&sub, "Rita Retting");
    let base = format!("/companies/{company}");
    let (status, customer) = call(
        &state,
        "POST",
        &format!("{base}/parties"),
        &token,
        Some(json!({ "kind": "kunde", "name": "Feilført Kunde AS" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{customer}");
    let party_no = customer["party_no"].as_str().unwrap().to_string();
    let party_id = customer["party_id"].as_str().unwrap().to_string();

    let (status, posted) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL", "date": "2026-03-10", "description": "Salg feil kunde",
            "lines": [
                { "account": "1500", "amount_ore": 1_250_00, "party_no": party_no },
                { "account": "3000", "amount_ore": -1_250_00 },
            ],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let original = posted["voucher_id"].as_str().unwrap().to_string();

    let (status, reversal) = call(
        &state,
        "POST",
        &format!("{base}/vouchers/{original}/reverse"),
        &token,
        Some(json!({ "dato": "2026-03-12" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reversal}");
    let reversal_id = reversal["voucher_id"].as_str().unwrap().to_string();

    // Linked both ways, every line mirrored.
    let (_, one) = call(
        &state,
        "GET",
        &format!("{base}/vouchers/{original}"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        one["reversed_by"]["voucher_id"],
        reversal_id.as_str(),
        "{one}"
    );
    assert!(one["owner"].is_null());
    let (_, two) = call(
        &state,
        "GET",
        &format!("{base}/vouchers/{reversal_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(two["reverses"]["voucher_id"], original.as_str(), "{two}");
    assert_eq!(two["date"], "2026-03-12");
    let amounts: Vec<i64> = two["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["amount_ore"].as_i64().unwrap())
        .collect();
    assert_eq!(amounts, vec![-1_250_00, 1_250_00]);
    assert_eq!(two["lines"][0]["party_no"], party_no.as_str());

    // The customer has nothing open: the mirror closed the original.
    let (_, items) = call(
        &state,
        "GET",
        &format!("{base}/parties/{party_id}/items?open=true"),
        &token,
        None,
    )
    .await;
    assert!(items["items"].as_array().unwrap().is_empty(), "{items}");

    // Once only, and never a reversal of a reversal.
    let (status, again) = call(
        &state,
        "POST",
        &format!("{base}/vouchers/{original}/reverse"),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        again.to_string().contains("allerede tilbakeført"),
        "{again}"
    );
    let (status, back) = call(
        &state,
        "POST",
        &format!("{base}/vouchers/{reversal_id}/reverse"),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(back.to_string().contains("selv en tilbakeføring"), "{back}");

    // A voucher another flow owns is corrected there, not here.
    let imported = regnmed_db::post_voucher(
        &state.pool,
        company,
        &regnmed_core::voucher::VoucherDraft {
            journal_code: "IMP".into(),
            voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            description: "Importert".into(),
            reverses: None,
            entries: ["1920", "3000"]
                .iter()
                .zip([100_00i64, -100_00])
                .map(|(a, ore)| regnmed_core::voucher::EntryDraft {
                    account_number: a.to_string(),
                    amount: regnmed_core::Ore(ore),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    department: None,
                    project: None,
                    currency: None,
                })
                .collect(),
        },
        "test",
    )
    .await
    .unwrap();
    let (_, owned) = call(
        &state,
        "GET",
        &format!("{base}/vouchers/{}", imported.id),
        &token,
        None,
    )
    .await;
    assert!(
        owned["owner"].as_str().unwrap().contains("importert"),
        "{owned}"
    );
    let (status, refused) = call(
        &state,
        "POST",
        &format!("{base}/vouchers/{}/reverse", imported.id),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused.to_string().contains("importert historikk"),
        "{refused}"
    );

    // An expense's voucher belongs to Utlegg: refused, and told so.
    for (number, name) in [("7100", "Bilgodtgjørelse"), ("2910", "Gjeld til ansatte")] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    let (expense, _, _) = regnmed_db::create_expense_run(
        &state.pool,
        company,
        person,
        chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(),
        "Kundebesøk",
        10,
        "test",
    )
    .await
    .unwrap();
    regnmed_db::approve_expense(
        &state.pool,
        company,
        expense,
        "7100",
        None,
        "2710",
        "2910",
        "test",
        person,
    )
    .await
    .unwrap();
    let expense_voucher: Uuid = sqlx::query_scalar("select voucher_id from expense where id = $1")
        .bind(expense)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let (status, refused) = call(
        &state,
        "POST",
        &format!("{base}/vouchers/{expense_voucher}/reverse"),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refused.to_string().contains("Utlegg"), "{refused}");

    // A voucher matched to a bank line: reversing it releases the bank
    // line, which is then reconciled again against the right voucher.
    let (_, paid) = call(
        &state,
        "POST",
        &format!("{base}/vouchers"),
        &token,
        Some(json!({
            "journal_code": "GL", "date": "2026-03-20", "description": "Innbetaling feil konto",
            "lines": [
                { "account": "1920", "amount_ore": 777_00 },
                { "account": "3000", "amount_ore": -777_00 },
            ],
        })),
    )
    .await;
    let statement =
        regnmed_core::bankcsv::parse("Dato;Forklaring;Beløp\n20.03.2026;Innbetaling;777,00\n")
            .unwrap();
    let summary = regnmed_db::import_statement(&state.pool, company, "1920", &statement, "test")
        .await
        .unwrap();
    assert_eq!(summary.auto_matched, 1);
    let (status, undone) = call(
        &state,
        "POST",
        &format!(
            "{base}/vouchers/{}/reverse",
            paid["voucher_id"].as_str().unwrap()
        ),
        &token,
        Some(json!({ "dato": "2026-03-21" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{undone}");
    assert_eq!(undone["bank_unmatched"], 1);
    let (_, recon) = call(
        &state,
        "GET",
        &format!("{base}/bank/reconciliation?account=1920"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        recon["unmatched_bank"].as_array().unwrap().len(),
        1,
        "{recon}"
    );

    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(
        report.vouchers_checked, 6,
        "original, reversal, import, expense, payment, its reversal"
    );
}
