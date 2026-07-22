//! End-to-end bank reconciliation over the web API: upload a camt.053
//! statement, auto-matching pairs it with posted vouchers, the leftover
//! is matched manually, and duplicate import is rejected. A revisor
//! ('les' via revisjon engagement) can read the reconciliation but not
//! mutate it. Requires DATABASE_URL (skips otherwise).

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
async fn seed(state: &AppState, accountant_sub: &str, revisor_sub: &str) -> Uuid {
    let accountant =
        regnmed_db::ensure_person(&state.pool, accountant_sub, Some("Kari Bokfører"), None)
            .await
            .unwrap();
    let revisor = regnmed_db::ensure_person(&state.pool, revisor_sub, Some("Revy Sorsen"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Bankklient AS")
        .await
        .unwrap();
    let firm = regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Tall & Orden AS", "regnskap")
        .await
        .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, firm, accountant, "ansatt")
        .await
        .unwrap();
    regnmed_db::ensure_engagement(&state.pool, firm, company, "regnskap")
        .await
        .unwrap();
    let revisorfirma =
        regnmed_db::ensure_firm(&state.pool, &unique_orgnr(), "Revisjon AS", "revisjon")
            .await
            .unwrap();
    regnmed_db::ensure_firm_member(&state.pool, revisorfirma, revisor, "ansatt")
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
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(-10_000_00),
                vat_code: Some("3".into()),
                description: None,
            },
            EntryDraft {
                account_number: "2700".into(),
                amount: Ore(-2_500_00),
                vat_code: None,
                description: None,
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
    let revisor_sub = format!("test|{}", Uuid::new_v4());
    let company = seed(&state, &accountant_sub, &revisor_sub).await;
    let accountant = idp.token(&accountant_sub, "Kari Bokfører");
    let revisor = idp.token(&revisor_sub, "Revy Sorsen");
    let camt = CAMT.replace("{REF}", &Uuid::new_v4().to_string());

    // A revisor cannot import (403 — company known via engagement).
    let (status, _) = request(
        &state,
        "POST",
        &format!("/companies/{company}/bank/statements?account=1920"),
        &revisor,
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
        &revisor,
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
    let gebyr = VoucherDraft {
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
            },
            EntryDraft {
                account_number: "3000".into(),
                amount: Ore(150_00),
                vat_code: None,
                description: None,
            },
        ],
    };
    regnmed_db::post_voucher(&state.pool, company, &gebyr, "test")
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
