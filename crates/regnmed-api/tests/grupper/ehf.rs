//! EHF outbound and inbound (#14): an issued faktura is rendered to
//! PEPPOL BIS Billing 3.0 from its own locked rows (and validated against
//! the UBL schema when xmllint is present), and a received EHF in the
//! bilagsinnboks yields a posting suggestion computed from the original
//! every time — never stored. Requires DATABASE_URL (skips otherwise).

use crate::common::{TestIdp, gi_partene_adresse, gjor_fakturaklar, test_state, unique_orgnr};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use regnmed_api::{AppState, router};

async fn get_text(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, String) {
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
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn get_json(state: &AppState, uri: &str, bearer: &str) -> (StatusCode, serde_json::Value) {
    let (status, body) = get_text(state, uri, bearer).await;
    (
        status,
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null),
    )
}

async fn post_bytes(
    state: &AppState,
    uri: &str,
    bearer: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let response = router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", format!("Bearer {bearer}"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
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

fn validate_ubl(xml: &str, schema: &str) {
    let dir = std::env::temp_dir().join(format!("regnmed-ehf-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.xml");
    std::fs::write(&path, xml).unwrap();
    let xsd = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ehf/");
    let Ok(output) = std::process::Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(format!("{xsd}{schema}"))
        .arg(&path)
        .output()
    else {
        eprintln!("xmllint ikke installert — hopper over skjemavalidering");
        return;
    };
    assert!(
        output.status.success(),
        "EHF-en fra databasen feilet skjemavalidering:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn faktura_out_as_ehf_and_received_ehf_into_the_innboks() {
    let idp = TestIdp::new();
    let Some(state) = test_state(&idp).await else {
        return;
    };
    let sub = format!("test|{}", Uuid::new_v4());
    let person = regnmed_db::ensure_person(&state.pool, &sub, Some("Eva EHF"), None)
        .await
        .unwrap();
    let company = regnmed_db::create_company(&state.pool, &unique_orgnr(), "Sender AS")
        .await
        .unwrap();
    gjor_fakturaklar(&state.pool, company).await;
    regnmed_db::ensure_company_member(&state.pool, company, person, "admin")
        .await
        .unwrap();
    regnmed_db::ensure_journal(&state.pool, company, "GL", "Hovedbok")
        .await
        .unwrap();
    for (number, name) in [
        ("1500", "Kundefordringer"),
        ("2700", "Utgående mva"),
        ("3000", "Salgsinntekt"),
        ("2400", "Leverandørgjeld"),
        ("4300", "Varekostnad"),
        ("2710", "Inngående mva"),
    ] {
        regnmed_db::ensure_account(&state.pool, company, number, name)
            .await
            .unwrap();
    }
    regnmed_db::set_account_reskontro(&state.pool, company, "1500", Some("kunde"))
        .await
        .unwrap();
    regnmed_db::set_account_reskontro(&state.pool, company, "2400", Some("leverandor"))
        .await
        .unwrap();
    // Company details: EHF needs an address and an account number.
    sqlx::query(
        "update company set address = 'Storgata 1, 0155 Oslo', bank_account = '86011117947',
                            orgform = 'AS', email = 'post@sender.no' where id = $1",
    )
    .bind(company)
    .execute(&state.pool)
    .await
    .unwrap();

    let (kunde_id, kunde_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "kunde",
        "Mottaker AS",
        Some("923609016"),
        None,
    )
    .await
    .unwrap();
    gi_partene_adresse(&state.pool, company).await;
    regnmed_db::update_party_contact(
        &state.pool,
        company,
        kunde_id,
        Some("Lilleveien 3, 5003 Bergen"),
        Some("faktura@mottaker.no"),
        None,
    )
    .await
    .unwrap();

    let issued = regnmed_db::create_invoice(
        &state.pool,
        company,
        &regnmed_db::InvoiceDraft {
            kontant_betalingsmiddel: None,
            party_no: kunde_no.clone(),
            invoice_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            due_date: chrono::NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            // Delivered before it was invoiced, so the rendered EHF
            // cannot pass by echoing the invoice date.
            delivery_date: chrono::NaiveDate::from_ymd_opt(2026, 6, 27).unwrap(),
            delivery_place: None,
            journal_code: "GL".into(),
            receivable_account: "1500".into(),
            vat_account: "2700".into(),
            valuta: None,
            valuta_kurs_micro: None,
            lines: vec![
                regnmed_db::InvoiceLineDraft {
                    description: "Konsulentbistand".into(),
                    account_number: "3000".into(),
                    quantity_milli: 2_000,
                    unit_price_ore: 1_250_00,
                    vat_code: Some("3".into()),
                    avdeling: None,
                    prosjekt: None,
                    product_id: None,
                },
                regnmed_db::InvoiceLineDraft {
                    description: "Utlegg uten mva".into(),
                    account_number: "3000".into(),
                    quantity_milli: 1_000,
                    unit_price_ore: 500_00,
                    vat_code: None,
                    avdeling: None,
                    prosjekt: None,
                    product_id: None,
                },
            ],
        },
        "Eva EHF",
        None,
    )
    .await
    .unwrap();

    let token = idp.token(&sub, "Eva EHF");
    let base = format!("/companies/{company}");

    // ---- Ut: EHF fra fakturaens egne rader ----
    let (status, xml) = get_text(
        &state,
        &format!("{base}/invoices/{}/ehf", issued.invoice_id),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    validate_ubl(&xml, "maindoc/UBL-Invoice-2.1.xsd");
    assert!(xml.contains("urn:fdc:peppol.eu:2017:poacc:billing:01:1.0"));
    assert!(
        xml.contains(&format!("<cbc:ID>{}</cbc:ID>", issued.invoice_no)),
        "fakturanummeret er hovedbokens eget"
    );
    assert!(
        xml.contains("<cbc:EndpointID schemeID=\"0192\">923609016</cbc:EndpointID>"),
        "mottakeren adresseres med orgnr som PEPPOL-deltaker"
    );
    assert!(
        xml.contains("<cbc:StreetName>Lilleveien 3</cbc:StreetName>")
            && xml.contains("<cbc:PostalZone>5003</cbc:PostalZone>")
            && xml.contains("<cbc:CityName>Bergen</cbc:CityName>"),
        "adressen deles i EHF-feltene: {xml}"
    );
    assert!(
        xml.contains(&format!("<cbc:PaymentID>{}</cbc:PaymentID>", issued.kid)),
        "KID-en følger med som betalingsreferanse"
    );
    // §5-1-1 nr. 4 / BT-72: the delivery date reaches the wire, and it
    // is the one that was recorded — not the invoice date.
    assert!(
        xml.contains("<cbc:ActualDeliveryDate>2026-06-27</cbc:ActualDeliveryDate>"),
        "leveringstidspunktet mangler i EHF: {xml}"
    );
    // 2 × 1 250 = 2 500 + 25 % mva, plus 500 without mva.
    assert!(
        xml.contains("<cbc:TaxInclusiveAmount currencyID=\"NOK\">3625.00</cbc:TaxInclusiveAmount>")
    );
    assert!(xml.contains("<cbc:Percent>25.00</cbc:Percent>"));

    // The kreditnota points back at the faktura.
    let credit = regnmed_db::credit_invoice(&state.pool, company, issued.invoice_id, "Eva EHF")
        .await
        .unwrap();
    let (status, credit_xml) = get_text(
        &state,
        &format!("{base}/invoices/{}/ehf", credit.invoice_id),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    validate_ubl(&credit_xml, "maindoc/UBL-CreditNote-2.1.xsd");
    assert!(credit_xml.contains("<cbc:CreditNoteTypeCode>381</cbc:CreditNoteTypeCode>"));
    assert!(
        credit_xml.contains(&format!("<cbc:ID>{}</cbc:ID>", issued.invoice_no)),
        "kreditnotaen navngir fakturaen den krediterer"
    );
    // The kreditnota repeats the ORIGINAL leveringstidspunkt: it
    // credits that delivery, and dating it "today" would assert a
    // delivery that never happened.
    assert!(
        credit_xml.contains("<cbc:ActualDeliveryDate>2026-06-27</cbc:ActualDeliveryDate>"),
        "kreditnotaen skal peke på den opprinnelige leveringen: {credit_xml}"
    );

    // ---- Inn: en mottatt EHF i innboksen ----
    let (_, leverandor_no) = regnmed_db::create_party(
        &state.pool,
        company,
        "leverandor",
        "Handelshuset AS",
        Some("974760673"),
        None,
    )
    .await
    .unwrap();
    let mottatt = r#"<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
         xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
         xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:ID>90210</cbc:ID>
  <cbc:IssueDate>2026-06-30</cbc:IssueDate>
  <cbc:DueDate>2026-07-30</cbc:DueDate>
  <cbc:DocumentCurrencyCode>NOK</cbc:DocumentCurrencyCode>
  <cac:AccountingSupplierParty><cac:Party>
    <cbc:EndpointID schemeID="0192">974760673</cbc:EndpointID>
    <cac:PartyLegalEntity><cbc:RegistrationName>Handelshuset AS</cbc:RegistrationName></cac:PartyLegalEntity>
  </cac:Party></cac:AccountingSupplierParty>
  <cac:AccountingCustomerParty><cac:Party>
    <cbc:EndpointID schemeID="0192">923609016</cbc:EndpointID>
  </cac:Party></cac:AccountingCustomerParty>
  <cac:PaymentMeans>
    <cbc:PaymentMeansCode>30</cbc:PaymentMeansCode>
    <cbc:PaymentID>0000000123456</cbc:PaymentID>
    <cac:PayeeFinancialAccount><cbc:ID>15062733139</cbc:ID></cac:PayeeFinancialAccount>
  </cac:PaymentMeans>
  <cac:TaxTotal><cbc:TaxAmount currencyID="NOK">500.00</cbc:TaxAmount></cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:TaxExclusiveAmount currencyID="NOK">2000.00</cbc:TaxExclusiveAmount>
    <cbc:PayableAmount currencyID="NOK">2500.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
  <cac:InvoiceLine>
    <cbc:ID>1</cbc:ID>
    <cbc:LineExtensionAmount currencyID="NOK">2000.00</cbc:LineExtensionAmount>
    <cac:Item><cbc:Name>Kontorstoler</cbc:Name>
      <cac:ClassifiedTaxCategory><cbc:ID>S</cbc:ID><cbc:Percent>25</cbc:Percent></cac:ClassifiedTaxCategory>
    </cac:Item>
  </cac:InvoiceLine>
</Invoice>"#;
    let (status, uploaded) = post_bytes(
        &state,
        &format!("{base}/inbox?filename=ehf-90210.xml"),
        &token,
        "application/xml",
        mottatt.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{uploaded}");
    let document_id = uploaded["document_id"].as_str().unwrap().to_string();

    let (status, forslag) =
        get_json(&state, &format!("{base}/inbox/{document_id}/ehf"), &token).await;
    assert_eq!(status, StatusCode::OK, "{forslag}");
    assert_eq!(forslag["fakturanr"], "90210");
    assert_eq!(forslag["selger_navn"], "Handelshuset AS");
    assert_eq!(forslag["selger_orgnr"], "974760673");
    assert_eq!(
        forslag["leverandor_no"], leverandor_no,
        "leverandøren gjenkjennes på orgnr"
    );
    assert_eq!(forslag["netto_ore"], 2_000_00);
    assert_eq!(forslag["mva_ore"], 500_00);
    assert_eq!(forslag["brutto_ore"], 2_500_00);
    assert_eq!(forslag["kid"], "0000000123456");
    assert_eq!(forslag["linjer"][0]["beskrivelse"], "Kontorstoler");
    assert_eq!(forslag["linjer"][0]["mva_sats_bp"], 2500);
    assert_eq!(forslag["warnings"].as_array().unwrap().len(), 0);

    // The suggestion is derived, not stored: the document still sits
    // undecided in the innboks, and the content is the original.
    let (_, listing) = get_json(&state, &format!("{base}/inbox?status=ny"), &token).await;
    assert_eq!(listing["documents"].as_array().unwrap().len(), 1);
    let (status, content) = get_text(
        &state,
        &format!("{base}/inbox/{document_id}/content"),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content, mottatt, "originalen er urørt");

    // A document that is not EHF says so instead of guessing.
    let (_, uploaded) = post_bytes(
        &state,
        &format!("{base}/inbox?filename=kvittering.txt"),
        &token,
        "text/plain",
        b"en helt vanlig kvittering".to_vec(),
    )
    .await;
    let other = uploaded["document_id"].as_str().unwrap().to_string();
    let (status, body) = get_json(&state, &format!("{base}/inbox/{other}/ehf"), &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap_or_default().contains("EHF"),
        "{body}"
    );
}
