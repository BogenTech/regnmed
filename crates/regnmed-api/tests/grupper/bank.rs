//! End-to-end bank reconciliation over the web API: upload a camt.053
//! statement, auto-matching pairs it with posted vouchers, the leftover
//! is matched manually, and duplicate import is rejected. A revisor
//! ('les' via revisjon engagement) can read the reconciliation but not
//! mutate it. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, make_invoiceable, test_state, unique_orgnumber};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::NaiveDate;
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

const CAMT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
 <BkToCstmrStmt>
  <Stmt>
   <Id>ST-{REF}</Id>
   <Acct><Id><IBAN>NO9386011117947</IBAN></Id></Acct>
   <Bal><Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="NOK">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-01-01</Dt></Dt></Bal>
   <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="NOK">12350.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-01-31</Dt></Dt></Bal>
   <Ntry><Amt Ccy="NOK">12500.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts>
         <BookgDt><Dt>2026-01-20</Dt></BookgDt>
         <NtryDtls><TxDtls><RmtInf><Ustrd>Innbetaling faktura</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry>
   <Ntry><Amt Ccy="NOK">150.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>BOOK</Sts>
         <BookgDt><Dt>2026-01-27</Dt></BookgDt>
         <AddtlNtryInf>Gebyr</AddtlNtryInf></Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>"#;

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
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// Company with a bank account, a matching sale voucher (12 500 kr in on
/// 2026-01-20) and an accountant (bokforing) + a revisor (les).
async fn seed(state: &AppState, accountant_sub: &str, auditor_sub: &str) -> Uuid {
    let accountant =
        regnmed_db::ensure_person(&state.pool, accountant_sub, Some("Kari Bokfører"), None)
            .await
            .unwrap();
    let auditor = regnmed_db::ensure_person(&state.pool, auditor_sub, Some("Revy Sorsen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnumber(), "Bankklient AS")
        .await
        .unwrap();
    make_invoiceable(&state.pool, company).await;
    let firm = regnmed_db::ensure_firm(
        &state.pool,
        &unique_orgnumber(),
        "Tall & Orden AS",
        "regnskap",
    )
    .await
    .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, accountant, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "regnskap")
        .await
        .unwrap();
    let revisorfirma =
        regnmed_db::ensure_firm(&state.pool, &unique_orgnumber(), "Revisjon AS", "revisjon")
            .await
            .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, revisorfirma, auditor, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, revisorfirma, company, "revisjon")
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
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-10_000_00),
                vat_code: Some("3".into()),
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: "2700".into(),
                amount: Ore(-2_500_00),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &sale, "test")
        .await
        .unwrap();
    company
}

#[tokio::test]
async fn import_auto_match_manual_match_and_permissions() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let accountant_sub = format!("test|{}", Uuid::new_v4());
    let auditor_sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &accountant_sub, &auditor_sub).await;
    let accountant = idp.token(&accountant_sub, "Kari Bokfører");
    let auditor = idp.token(&auditor_sub, "Revy Sorsen");
    let camt = CAMT.replace("{REF}", &Uuid::new_v4().to_string());

    // A revisor cannot import (403 — company known via engagement).
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &auditor,
        Some(camt.clone()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The accountant imports: 2 transactions, the 12 500 kr inflow
    // auto-matches the posted sale on the same date.
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &accountant,
        Some(camt.clone()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["transactions"], 2);
    assert_eq!(body["auto_matched"], 1);

    // Re-import of the same statement id is rejected.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &accountant,
        Some(camt),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The revisor can read the reconciliation: the 150 kr gebyr is the
    // one unmatched bank transaction; the ledger has no entry for it yet.
    let (status, recon) = request(
        &state,
        "GET",
        &format!("/companies/{company}/bank/reconciliation?account=1920"),
        &auditor,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recon["matched_count"], 1);
    assert_eq!(recon["ledger_balance_ore"], 12_500_00);
    assert_eq!(recon["statement_closing_ore"], 12_350_00);
    assert_eq!(recon["unmatched_bank"].as_array().unwrap().len(), 1);
    assert_eq!(recon["unmatched_bank"][0]["amount_ore"], -150_00);

    // Post the gebyr voucher (dated two days after the bank booking),
    // then match it manually.
    let fee = VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: NaiveDate::from_ymd_opt(2026, 1, 29).unwrap(),
        description: "Bankgebyr".into(),
        reverses: None,
        entries: vec![
            EntryDraft {
                account_number: "1920".into(),
                amount: Ore(-150_00),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(150_00),
                vat_code: None,
                description: None,
                party_no: None,
                department: None,
                project: None,
                currency: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &fee, "test")
        .await
        .unwrap();

    let (_, recon) = request(
        &state,
        "GET",
        &format!("/companies/{company}/bank/reconciliation?account=1920"),
        &accountant,
        None,
        false,
    )
    .await;
    let bank_tx_id = recon["unmatched_bank"][0]["bank_transaction_id"]
        .as_str()
        .unwrap()
        .to_string();
    let entry_id = recon["unmatched_entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["amount_ore"] == -150_00)
        .unwrap()["entry_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/matches"),
        &accountant,
        Some(
            serde_json::json!({ "bank_transaction_id": bank_tx_id, "entry_id": entry_id })
                .to_string(),
        ),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Fully reconciled: no unmatched bank transactions left.
    let (_, recon) = request(
        &state,
        "GET",
        &format!("/companies/{company}/bank/reconciliation?account=1920"),
        &accountant,
        None,
        false,
    )
    .await;
    assert_eq!(recon["matched_count"], 2);
    assert!(recon["unmatched_bank"].as_array().unwrap().is_empty());

    // Unmatch works, and a stranger sees nothing at all.
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("/companies/{company}/bank/matches/{bank_tx_id}"),
        &accountant,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let stranger = idp.token(&format!("test|{}", Uuid::new_v4()), "Ukjent Person");
    let (status, _) = request(
        &state,
        "GET",
        &format!("/companies/{company}/bank/reconciliation?account=1920"),
        &stranger,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Tier 1b: a bank CSV export goes through the SAME import endpoint and
/// engine — auto-matching, idempotent re-import (content-hash ref) and
/// loud rejection of unknown layouts.
#[tokio::test]
async fn csv_export_imports_through_the_same_engine() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let accountant_sub = format!("test|{}", Uuid::new_v4());
    let auditor_sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &accountant_sub, &auditor_sub).await;
    let accountant = idp.token(&accountant_sub, "Kari Bokfører");

    // A DNB-style export: the 12 500 kr inflow matches the seeded sale.
    let csv = "Dato;Forklaring;Rentedato;Ut fra konto;Inn på konto\n\
               20.01.2026;Innbetaling faktura;20.01.2026;;12 500,00\n\
               27.01.2026;Gebyr;27.01.2026;150,00;\n";
    let (status, body) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &accountant,
        Some(csv.to_string()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["transactions"], 2);
    assert_eq!(body["auto_matched"], 1);

    // Same file again: the content-hash statement ref makes it a
    // duplicate, exactly like a camt.053 re-import.
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &accountant,
        Some(csv.to_string()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The reconciliation view carries the CSV lines (no closing balance
    // in a CSV — the view treats it as absent, not zero).
    let (status, recon) = request(
        &state,
        "GET",
        &format!("/companies/{company}/bank/reconciliation?account=1920"),
        &accountant,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recon["matched_count"], 1);
    assert_eq!(recon["unmatched_bank"][0]["amount_ore"], -150_00);
    assert!(recon["statement_closing_ore"].is_null());

    // A spreadsheet without recognizable columns fails loudly.
    let (status, error) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &accountant,
        Some("Kolonne A;Kolonne B\n1;2\n".to_string()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"].as_str().unwrap().contains("kolonne"),
        "{error}"
    );
}

/// The bank line with no posting behind it (the 150 kr fee): suggestions
/// never offer a different amount, a manual match of unequal amounts is
/// refused, and the fee is posted and matched from the line itself in one
/// step — subject to the same attestering gate as a manual voucher.
#[tokio::test]
async fn a_bank_line_is_posted_from_the_statement_and_amounts_must_agree() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let accountant_sub = format!("test|{}", Uuid::new_v4());
    let auditor_sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &accountant_sub, &auditor_sub).await;
    regnmed_db::ensure_account(&state.pool, company, "7770", "Bank- og kortgebyrer")
        .await
        .unwrap();
    let accountant = idp.token(&accountant_sub, "Kari Bokfører");
    let auditor = idp.token(&auditor_sub, "Revy Sorsen");
    let camt = CAMT.replace("{REF}", &Uuid::new_v4().to_string());
    let base = format!("/companies/{company}/bank");
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/statements?account=1920"),
        &accountant,
        Some(camt),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, accounts) = request(
        &state,
        "GET",
        &format!("{base}/accounts"),
        &auditor,
        None,
        false,
    )
    .await;
    let bank = accounts["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == "1920")
        .unwrap()
        .clone();
    assert_eq!(bank["unmatched_bank"], 1);
    assert_eq!(bank["last_statement_to"], "2026-01-31");

    let (_, recon) = request(
        &state,
        "GET",
        &format!("{base}/reconciliation?account=1920"),
        &auditor,
        None,
        false,
    )
    .await;
    let fee = recon["unmatched_bank"][0].clone();
    assert_eq!(fee["amount_ore"], -150_00);
    assert!(
        fee["suggestions"].as_array().unwrap().is_empty(),
        "no -150 entry exists: {fee}"
    );
    assert_eq!(recon["statements"].as_array().unwrap().len(), 1);
    assert_eq!(recon["statements"][0]["closing_ore"], 12_350_00);
    assert_eq!(
        recon["ledger_at_statement_ore"], 12_500_00,
        "the sale, not yet the fee"
    );
    let fee_id = fee["bank_transaction_id"].as_str().unwrap().to_string();

    // Undo the auto-match of the sale, find its entry by searching for
    // the amount as typed, and try to pair it with the FEE: refused.
    let inflow_id = recon["recent_matches"][0]["bank_transaction_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, _) = request(
        &state,
        "DELETE",
        &format!("{base}/matches/{inflow_id}"),
        &accountant,
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, found) = request(
        &state,
        "GET",
        &format!("{base}/entries?account=1920&q=12%20500"),
        &auditor,
        None,
        false,
    )
    .await;
    let sale_entry = found["entries"][0]["entry_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pair = |bank: &str| {
        serde_json::json!({ "bank_transaction_id": bank, "entry_id": sale_entry }).to_string()
    };
    let (status, body) = request(
        &state,
        "POST",
        &format!("{base}/matches"),
        &accountant,
        Some(pair(&fee_id)),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.to_string().contains("ulike"), "{body}");
    // The equal amount is fine, and is what the suggestion offers.
    let (_, recon) = request(
        &state,
        "GET",
        &format!("{base}/reconciliation?account=1920"),
        &auditor,
        None,
        false,
    )
    .await;
    let inflow = recon["unmatched_bank"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["amount_ore"] == 12_500_00)
        .unwrap()
        .clone();
    assert_eq!(inflow["suggestions"][0]["entry_id"], sale_entry.as_str());
    let (status, _) = request(
        &state,
        "POST",
        &format!("{base}/matches"),
        &accountant,
        Some(pair(&inflow_id)),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let post = format!("{base}/transactions/{fee_id}/post");
    let body = serde_json::json!({ "konto": "7770", "tekst": "Bankgebyr januar" }).to_string();
    let (status, _) = request(&state, "POST", &post, &auditor, Some(body.clone()), true).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a revisor reads, never posts"
    );

    // With attestering on (no limit), a voucher without a document is refused here too.
    regnmed_db::set_policy(&state.pool, company, true, None, None, None, "test")
        .await
        .unwrap();
    let (status, refused) =
        request(&state, "POST", &post, &accountant, Some(body.clone()), true).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refused.to_string().contains("attester"), "{refused}");
    regnmed_db::set_policy(&state.pool, company, false, None, None, None, "test")
        .await
        .unwrap();

    let (status, posted) =
        request(&state, "POST", &post, &accountant, Some(body.clone()), true).await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    let (status, _) = request(&state, "POST", &post, &accountant, Some(body), true).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a matched line is not posted twice"
    );

    let (_, recon) = request(
        &state,
        "GET",
        &format!("{base}/reconciliation?account=1920"),
        &auditor,
        None,
        false,
    )
    .await;
    assert!(recon["unmatched_bank"].as_array().unwrap().is_empty());
    assert_eq!(recon["ledger_at_statement_ore"], 12_350_00);
    assert_eq!(
        recon["statement_closing_ore"], 12_350_00,
        "reconciled to the øre"
    );
    let matched = recon["recent_matches"].as_array().unwrap();
    assert!(
        matched
            .iter()
            .any(|m| m["voucher"] == posted["voucher"] && m["method"] == "manual"),
        "{recon}"
    );
    let report = regnmed_db::verify_chain(&state.pool, company)
        .await
        .unwrap();
    assert_eq!(report.vouchers_checked, 2, "the sale and the fee");
}
