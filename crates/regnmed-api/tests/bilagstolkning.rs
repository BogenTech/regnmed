//! Bilagstolkning (#34): innboksen foreslår, mennesket bokfører.
//!
//! Den sterkeste testen er en round-trip: vi utsteder en faktura med
//! vår egen PDF-generator, laster den opp i innboksen som om den kom
//! fra en leverandør, og krever at tolkningen finner igjen tallene vi
//! selv skrev — pluss kontoen leverandøren sist ble bokført på.
//! Requires DATABASE_URL (skips otherwise).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{TestIdp, test_state, unique_orgnr};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};
use regnmed_core::Ore;
use regnmed_core::voucher::{EntryDraft, VoucherDraft};

async fn get_json(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

async fn upload(
    state: &AppState,
    company: Uuid,
    bearer: &str,
    filename: &str,
    content_type: &str,
    body: Vec<u8>,
) -> String {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/companies/{company}/inbox?filename={filename}"))
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["document_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn forslag_fra_pdf_tekstlag_og_fra_historikken() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Tolke Toril"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Mottaker AS")
        .await
        .unwrap();
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1920", "Bank"),
        ("2400", "Leverandørgjeld"),
        ("2710", "Inngående mva"),
        ("6300", "Leie av lokaler"),
        ("4300", "Varekostnad"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    // Leverandøren finnes med orgnr — det er nøkkelen tolkningen bruker.
    let (_, leverandor_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Utleiebygg AS",
        Some("974760673"),
        None,
    )
    .await
    .unwrap();

    // Historikk: forrige måneds husleie ble bokført på 6300.
    regnmed_db::post_voucher(
        &state.pool,
        company,
        &VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            description: "Husleie juni".into(),
            reverses: None,
            entries: vec![
                EntryDraft {
                    account_number: "6300".into(),
                    amount: Ore(10_000_00),
                    vat_code: None,
                    description: None,
                    party_no: None,
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
                EntryDraft {
                    account_number: "2400".into(),
                    amount: Ore(-10_000_00),
                    vat_code: None,
                    description: None,
                    party_no: Some(leverandor_no.clone()),
                    avdeling: None,
                    prosjekt: None,
                    valuta: None,
                },
            ],
        },
        "test",
    )
    .await
    .unwrap();

    let token = idp.token(&sub, "Tolke Toril");
    let base = format!("/companies/{company}");

    // ---- En generert faktura-PDF fra "leverandøren" ----
    let mut pdf = regnmed_core::pdf::Pdf::new();
    let mut y = 780.0;
    for line in [
        "Utleiebygg AS",
        "Orgnr 974760673 MVA",
        "Storgata 1, 0155 Oslo",
        "FAKTURA",
        "Fakturanr: 90210",
        "Fakturadato: 01.07.2026",
        "Forfallsdato: 15.07.2026",
        "Husleie juli 2026            10 000,00",
        "MVA 25 %                      2 500,00",
        "Å betale                     12 500,00",
        "Kontonummer: 8601.11.17947",
        "KID: 1234567897",
    ] {
        pdf.text(50.0, y, 10.0, regnmed_core::pdf::Font::Regular, line);
        y -= 16.0;
    }
    let document_id = upload(
        &state,
        company,
        &token,
        "husleie-juli.pdf",
        "application/pdf",
        pdf.finish(),
    )
    .await;

    let (status, f) = get_json(
        &state,
        &format!("{base}/inbox/{document_id}/forslag"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{f}");
    assert_eq!(f["kilde"], "pdf-tekst");
    assert_eq!(f["orgnr"], "974760673");
    assert_eq!(f["fakturanr"], "90210");
    assert_eq!(f["dato"], "2026-07-01");
    assert_eq!(f["forfall"], "2026-07-15");
    assert_eq!(f["brutto_ore"], 12_500_00);
    assert_eq!(f["mva_ore"], 2_500_00);
    assert_eq!(f["netto_ore"], 10_000_00);
    assert_eq!(f["kid"], "1234567897");
    assert_eq!(f["kontonummer"], "86011117947");
    // Leverandøren gjenkjennes, og kontoen kommer fra selskapets EGEN
    // historikk — ikke fra en modell.
    assert_eq!(f["leverandor_no"], leverandor_no);
    assert_eq!(f["leverandor_navn"], "Utleiebygg AS");
    assert_eq!(f["konto"], "6300");
    assert!(
        f["konto_begrunnelse"]
            .as_str()
            .unwrap()
            .contains("sist bokført"),
        "{f}"
    );
    // Hvert felt kan forklare seg.
    let begrunnelser = f["begrunnelser"].as_array().unwrap();
    assert!(
        begrunnelser
            .iter()
            .any(|b| b["felt"] == "brutto" && b["hvorfor"].as_str().unwrap().contains("å betale"))
    );
    assert!(
        begrunnelser
            .iter()
            .any(|b| b["felt"] == "orgnr"
                && b["hvorfor"].as_str().unwrap().contains("kontrollsiffer"))
    );

    // ---- Ingenting bokføres av seg selv ----
    let (_, listing) = get_json(&state, &format!("{base}/inbox?status=ny"), &token).await;
    assert_eq!(
        listing["documents"].as_array().unwrap().len(),
        1,
        "forslaget rører ikke dokumentets status"
    );

    // ---- Et skannet bilde uten tekstlag sier fra ----
    let scan = upload(
        &state,
        company,
        &token,
        "skann.pdf",
        "application/pdf",
        b"%PDF-1.4\n1 0 obj\n<< /Subtype /Image /Length 4 >>\nstream\n\x00\x01\x02\x03\nendstream\nendobj\n".to_vec(),
    )
    .await;
    let (status, f) = get_json(&state, &format!("{base}/inbox/{scan}/forslag"), &token).await;
    assert_eq!(status, StatusCode::OK, "{f}");
    assert_eq!(f["kilde"], "ingen");
    assert!(f["brutto_ore"].is_null(), "ingen gjetning uten tekst");
    assert!(
        f["warnings"][0]
            .as_str()
            .unwrap()
            .contains("ingen lesbar tekst"),
        "{f}"
    );

    // ---- EHF går fortsatt den eksakte veien gjennom samme endepunkt ----
    let ehf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
 xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
 xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>777</cbc:ID>
  <cbc:IssueDate>2026-08-01</cbc:IssueDate>
  <cbc:DocumentCurrencyCode>NOK</cbc:DocumentCurrencyCode>
  <cac:AccountingSupplierParty><cac:Party>
    <cbc:EndpointID schemeID="0192">974760673</cbc:EndpointID>
    <cac:PartyLegalEntity><cbc:RegistrationName>Utleiebygg AS</cbc:RegistrationName></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingSupplierParty>
  <cac:TaxTotal><cbc:TaxAmount currencyID="NOK">2500.00</cbc:TaxAmount></cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:TaxExclusiveAmount currencyID="NOK">10000.00</cbc:TaxExclusiveAmount>
    <cbc:PayableAmount currencyID="NOK">12500.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
</Invoice>"#
    );
    let ehf_doc = upload(
        &state,
        company,
        &token,
        "ehf-777.xml",
        "application/xml",
        ehf.into_bytes(),
    )
    .await;
    let (_, f) = get_json(&state, &format!("{base}/inbox/{ehf_doc}/forslag"), &token).await;
    assert_eq!(f["kilde"], "ehf", "strukturert slår heuristikk");
    assert_eq!(f["fakturanr"], "777");
    assert_eq!(f["brutto_ore"], 12_500_00);
    assert_eq!(f["konto"], "6300", "historikken gjelder uansett kilde");
}
