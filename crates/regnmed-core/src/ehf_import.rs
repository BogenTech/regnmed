//! EHF inbound: a received faktura is read into a **posting suggestion**
//! (docs/ehf.md, #14).
//!
//! This is the structured end of document interpretation (#34): when the
//! document is EHF, the supplier, amounts and mva are not guesses — they
//! are in the file. The parser is tolerant in the camt.053 style (it
//! reads only what posting needs, skips the rest and accepts both Invoice
//! and CreditNote), because we do not control the sender.
//!
//! The suggestion is exactly that: a human posts (or rejects) it through
//! the innboks like any other bilag. Nothing derived here is stored — the
//! original is preserved, and the rest is recomputed every time it is
//! asked for.

use chrono::NaiveDate;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EhfImportError {
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("ikke et EHF-dokument: fant verken <Invoice> eller <CreditNote>")]
    NotEhf,
    #[error("ugyldig beløp '{0}'")]
    BadAmount(String),
    #[error("ugyldig dato '{0}'")]
    BadDate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MottattLinje {
    pub description: String,
    pub net_ore: i64,
    /// Basis points from ClassifiedTaxCategory when present.
    pub vat_rate_bp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MottattFaktura {
    /// True for CreditNote — the amounts flip when the bilag is proposed.
    pub er_kreditnota: bool,
    pub fakturanr: String,
    pub invoice_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub currency: String,
    pub seller_name: String,
    /// Norwegian orgnr without prefix when the sender uses ICD 0192.
    pub seller_orgnumber: Option<String>,
    pub buyer_orgnumber: Option<String>,
    pub kid: Option<String>,
    pub account_number: Option<String>,
    pub net_ore: i64,
    pub vat_ore: i64,
    /// PayableAmount — what the supplier actually claims.
    pub gross_ore: i64,
    pub linjer: Vec<MottattLinje>,
}

/// "1234.56" / "1234,56" / "1234" → øre. The sender decides the format;
/// we refuse to guess at more than two decimals.
fn amount(raw: &str) -> Result<i64, EhfImportError> {
    let cleaned: String = raw.trim().replace(',', ".");
    let bad = || EhfImportError::BadAmount(raw.to_string());
    let (whole, frac) = match cleaned.split_once('.') {
        Some((w, f)) => (w, f),
        None => (cleaned.as_str(), ""),
    };
    if frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let negative = whole.starts_with('-');
    let digits = whole.trim_start_matches(['-', '+']);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let whole: i64 = digits.parse().map_err(|_| bad())?;
    let frac: i64 = format!("{frac:0<2}").parse().map_err(|_| bad())?;
    let ore = whole * 100 + frac;
    Ok(if negative { -ore } else { ore })
}

fn date(raw: &str) -> Result<NaiveDate, EhfImportError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| EhfImportError::BadDate(raw.to_string()))
}

/// Percent "25" / "25.00" → basispunkter.
fn percent_bp(raw: &str) -> Option<i64> {
    let cleaned = raw.trim().replace(',', ".");
    let (whole, frac) = match cleaned.split_once('.') {
        Some((w, f)) => (w, f),
        None => (cleaned.as_str(), ""),
    };
    let whole: i64 = whole.parse().ok()?;
    let frac: i64 = format!("{:0<2}", &frac[..frac.len().min(2)])
        .parse()
        .unwrap_or(0);
    Some(whole * 100 + frac)
}

fn ends_with(path: &[String], tail: &[&str]) -> bool {
    path.len() >= tail.len() && path[path.len() - tail.len()..] == *tail
}

pub fn parse(xml: &str) -> Result<MottattFaktura, EhfImportError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut path: Vec<String> = Vec::new();
    let mut invoice = MottattFaktura {
        er_kreditnota: false,
        fakturanr: String::new(),
        invoice_date: None,
        due_date: None,
        currency: "NOK".into(),
        seller_name: String::new(),
        seller_orgnumber: None,
        buyer_orgnumber: None,
        kid: None,
        account_number: None,
        net_ore: 0,
        vat_ore: 0,
        gross_ore: 0,
        linjer: Vec::new(),
    };
    let mut root_seen = false;
    // Line being assembled.
    let (mut line_name, mut line_net, mut line_bp) = (String::new(), 0i64, None::<i64>);
    let mut in_line = false;
    // The seller's PartyLegalEntity/PartyName wins over other names.
    let mut supplier_name_from_legal = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| EhfImportError::Xml(e.to_string()))?;
        match event {
            Event::Start(start) => {
                let local = String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
                if !root_seen {
                    match local.as_str() {
                        "Invoice" => {
                            root_seen = true;
                        }
                        "CreditNote" => {
                            root_seen = true;
                            invoice.er_kreditnota = true;
                        }
                        _ => {}
                    }
                }
                if local == "InvoiceLine" || local == "CreditNoteLine" {
                    in_line = true;
                    line_name = String::new();
                    line_net = 0;
                    line_bp = None;
                }
                path.push(local);
            }
            Event::End(end) => {
                let local = String::from_utf8_lossy(end.local_name().as_ref()).into_owned();
                if local == "InvoiceLine" || local == "CreditNoteLine" {
                    invoice.linjer.push(MottattLinje {
                        description: std::mem::take(&mut line_name),
                        net_ore: line_net,
                        vat_rate_bp: line_bp,
                    });
                    in_line = false;
                }
                path.pop();
            }
            Event::Text(text) => {
                let value = text
                    .unescape()
                    .map_err(|e| EhfImportError::Xml(e.to_string()))?
                    .into_owned();
                let value = value.trim().to_string();
                if value.is_empty() || path.is_empty() {
                    continue;
                }
                let leaf = path[path.len() - 1].as_str();
                let supplier = path.iter().any(|p| p == "AccountingSupplierParty");
                let customer = path.iter().any(|p| p == "AccountingCustomerParty");

                match leaf {
                    "ID" if path.len() == 2 => invoice.fakturanr = value,
                    "IssueDate" if path.len() == 2 => invoice.invoice_date = Some(date(&value)?),
                    "DueDate" if path.len() == 2 => invoice.due_date = Some(date(&value)?),
                    "DocumentCurrencyCode" if path.len() == 2 => invoice.currency = value,
                    "PaymentID" => invoice.kid = Some(value),
                    "Name" if supplier && ends_with(&path, &["PartyName", "Name"]) => {
                        if !supplier_name_from_legal {
                            invoice.seller_name = value;
                        }
                    }
                    "RegistrationName" if supplier => {
                        invoice.seller_name = value;
                        supplier_name_from_legal = true;
                    }
                    "EndpointID" | "CompanyID" | "ID"
                        if (supplier || customer)
                            && (ends_with(&path, &["Party", "EndpointID"])
                                || ends_with(&path, &["PartyIdentification", "ID"])
                                || ends_with(&path, &["PartyLegalEntity", "CompanyID"])) =>
                    {
                        // Strip an ICD prefix ("0192:915933149") and any
                        // NO…MVA wrapper; keep the nine digits.
                        let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
                        let orgnumber = if digits.len() >= 9 {
                            Some(digits[digits.len() - 9..].to_string())
                        } else {
                            None
                        };
                        if supplier {
                            invoice.seller_orgnumber =
                                invoice.seller_orgnumber.take().or(orgnumber);
                        } else {
                            invoice.buyer_orgnumber = invoice.buyer_orgnumber.take().or(orgnumber);
                        }
                    }
                    "ID" if ends_with(&path, &["PayeeFinancialAccount", "ID"]) => {
                        invoice.account_number = Some(value);
                    }
                    "TaxExclusiveAmount" => invoice.net_ore = amount(&value)?,
                    "TaxInclusiveAmount" if invoice.gross_ore == 0 => {
                        invoice.gross_ore = amount(&value)?;
                    }
                    "PayableAmount" => invoice.gross_ore = amount(&value)?,
                    "TaxAmount" if ends_with(&path, &["TaxTotal", "TaxAmount"]) => {
                        invoice.vat_ore = amount(&value)?;
                    }
                    "LineExtensionAmount" if in_line => line_net = amount(&value)?,
                    "LineExtensionAmount" if invoice.net_ore == 0 => {
                        invoice.net_ore = amount(&value)?;
                    }
                    "Name" if in_line && ends_with(&path, &["Item", "Name"]) => line_name = value,
                    "Description" if in_line && line_name.is_empty() => line_name = value,
                    "Percent" if in_line => line_bp = percent_bp(&value),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !root_seen {
        return Err(EhfImportError::NotEhf);
    }
    if invoice.net_ore == 0 && !invoice.linjer.is_empty() {
        invoice.net_ore = invoice.linjer.iter().map(|l| l.net_ore).sum();
    }
    if invoice.gross_ore == 0 {
        invoice.gross_ore = invoice.net_ore + invoice.vat_ore;
    }
    Ok(invoice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ehf::{Dokumenttype, EhfDokument, EhfLinje, EhfPart, render};

    fn part(name: &str, orgnumber: &str) -> EhfPart {
        EhfPart {
            name: name.into(),
            orgnumber: orgnumber.into(),
            adresse: Some("Storgata 1".into()),
            postnr: Some("0155".into()),
            poststed: Some("Oslo".into()),
            land: "NO".into(),
            vat_registered: true,
            epost: None,
        }
    }

    fn dokument(dokumenttype: Dokumenttype) -> EhfDokument {
        EhfDokument {
            dokumenttype,
            fakturanr: "F-2026-7".into(),
            krediterer: None,
            invoice_date: NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(),
            due_date: NaiveDate::from_ymd_opt(2026, 7, 17),
            currency: "NOK".into(),
            kjopers_referanse: None,
            selger: part("Grossisten AS", "915933149"),
            kjoper: part("Vår Klient AS", "923609016"),
            delivery_date: NaiveDate::from_ymd_opt(2026, 7, 2),
            leveringssted: None,
            account_number: Some("86011117947".into()),
            kid: Some("1234567897".into()),
            linjer: vec![
                EhfLinje {
                    description: "Varekjøp".into(),
                    quantity_milli: 3_000,
                    unit: "EA".into(),
                    enhetspris_ore: 400_00,
                    net_ore: 1_200_00,
                    vat_rate_bp: Some(2500),
                    vat_ore: 300_00,
                },
                EhfLinje {
                    description: "Frakt".into(),
                    quantity_milli: 1_000,
                    unit: "EA".into(),
                    enhetspris_ore: 100_00,
                    net_ore: 100_00,
                    vat_rate_bp: Some(2500),
                    vat_ore: 25_00,
                },
            ],
        }
    }

    /// Round-trip against our own renderer, the same guarantee the SAF-T
    /// importer carries: what we write, we can read.
    #[test]
    fn reads_our_own_faktura_back() {
        let xml = render(&dokument(Dokumenttype::Invoice));
        let received = parse(&xml).unwrap();
        assert!(!received.er_kreditnota);
        assert_eq!(received.fakturanr, "F-2026-7");
        assert_eq!(
            received.invoice_date,
            Some(NaiveDate::from_ymd_opt(2026, 7, 3).unwrap())
        );
        assert_eq!(
            received.due_date,
            Some(NaiveDate::from_ymd_opt(2026, 7, 17).unwrap())
        );
        assert_eq!(received.seller_name, "Grossisten AS");
        assert_eq!(received.seller_orgnumber.as_deref(), Some("915933149"));
        assert_eq!(received.buyer_orgnumber.as_deref(), Some("923609016"));
        assert_eq!(received.kid.as_deref(), Some("1234567897"));
        assert_eq!(received.account_number.as_deref(), Some("86011117947"));
        assert_eq!(received.net_ore, 1_300_00);
        assert_eq!(received.vat_ore, 325_00);
        assert_eq!(received.gross_ore, 1_625_00);
        assert_eq!(received.linjer.len(), 2);
        assert_eq!(received.linjer[0].description, "Varekjøp");
        assert_eq!(received.linjer[0].net_ore, 1_200_00);
        assert_eq!(received.linjer[0].vat_rate_bp, Some(2500));
    }

    #[test]
    fn a_kreditnota_is_recognised() {
        let xml = render(&dokument(Dokumenttype::CreditNote));
        let received = parse(&xml).unwrap();
        assert!(received.er_kreditnota);
        assert_eq!(received.linjer.len(), 2);
        assert_eq!(received.gross_ore, 1_625_00);
    }

    /// A real EHF from another system: different prefixes, extra
    /// elements, no line-level tax category.
    #[test]
    fn tolerates_a_foreign_document_with_unknown_elements() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ubl:Invoice xmlns:ubl="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
             xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
             xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <cbc:UBLVersionID>2.1</cbc:UBLVersionID>
  <cbc:ID>90210</cbc:ID>
  <cbc:IssueDate>2026-06-30</cbc:IssueDate>
  <cbc:DueDate>2026-07-30</cbc:DueDate>
  <cbc:InvoiceTypeCode listID="UNCL1001">380</cbc:InvoiceTypeCode>
  <cbc:Note>Takk for handelen</cbc:Note>
  <cbc:DocumentCurrencyCode>NOK</cbc:DocumentCurrencyCode>
  <cac:AccountingSupplierParty>
    <cac:Party>
      <cbc:EndpointID schemeID="0192">974760673</cbc:EndpointID>
      <cac:PartyName><cbc:Name>Handelshuset</cbc:Name></cac:PartyName>
      <cac:PartyLegalEntity>
        <cbc:RegistrationName>Handelshuset AS</cbc:RegistrationName>
        <cbc:CompanyID schemeID="0192">974760673</cbc:CompanyID>
      </cac:PartyLegalEntity>
    </cac:Party>
  </cac:AccountingSupplierParty>
  <cac:AccountingCustomerParty>
    <cac:Party>
      <cbc:EndpointID schemeID="0192">923609016</cbc:EndpointID>
    </cac:Party>
  </cac:AccountingCustomerParty>
  <cac:PaymentMeans>
    <cbc:PaymentMeansCode>30</cbc:PaymentMeansCode>
    <cbc:PaymentID>0000000123456</cbc:PaymentID>
    <cac:PayeeFinancialAccount><cbc:ID>15062733139</cbc:ID></cac:PayeeFinancialAccount>
  </cac:PaymentMeans>
  <cac:TaxTotal>
    <cbc:TaxAmount currencyID="NOK">500.00</cbc:TaxAmount>
  </cac:TaxTotal>
  <cac:LegalMonetaryTotal>
    <cbc:LineExtensionAmount currencyID="NOK">2000.00</cbc:LineExtensionAmount>
    <cbc:TaxExclusiveAmount currencyID="NOK">2000.00</cbc:TaxExclusiveAmount>
    <cbc:TaxInclusiveAmount currencyID="NOK">2500.00</cbc:TaxInclusiveAmount>
    <cbc:PayableAmount currencyID="NOK">2500.00</cbc:PayableAmount>
  </cac:LegalMonetaryTotal>
  <cac:InvoiceLine>
    <cbc:ID>1</cbc:ID>
    <cbc:InvoicedQuantity unitCode="EA">10</cbc:InvoicedQuantity>
    <cbc:LineExtensionAmount currencyID="NOK">2000.00</cbc:LineExtensionAmount>
    <cac:Item><cbc:Name>Kontorstoler</cbc:Name></cac:Item>
  </cac:InvoiceLine>
</ubl:Invoice>"#;
        let received = parse(xml).unwrap();
        assert_eq!(received.fakturanr, "90210");
        assert_eq!(
            received.seller_name, "Handelshuset AS",
            "RegistrationName vinner over PartyName"
        );
        assert_eq!(received.seller_orgnumber.as_deref(), Some("974760673"));
        assert_eq!(received.buyer_orgnumber.as_deref(), Some("923609016"));
        assert_eq!(received.kid.as_deref(), Some("0000000123456"));
        assert_eq!(received.net_ore, 2_000_00);
        assert_eq!(received.vat_ore, 500_00);
        assert_eq!(received.gross_ore, 2_500_00);
        assert_eq!(received.linjer[0].description, "Kontorstoler");
        assert_eq!(received.linjer[0].vat_rate_bp, None, "ingen sats på linjen");
    }

    #[test]
    fn something_that_is_not_ehf_is_rejected() {
        let error = parse("<?xml version=\"1.0\"?><Document><Nope/></Document>").unwrap_err();
        assert!(matches!(error, EhfImportError::NotEhf));
    }

    #[test]
    fn amounts_without_decimals_and_with_commas_are_read() {
        assert_eq!(amount("2000").unwrap(), 200_000);
        assert_eq!(amount("2000.5").unwrap(), 200_050);
        assert_eq!(amount("-12,25").unwrap(), -1225);
        assert!(amount("12.345").is_err(), "tre desimaler er ikke øre");
    }
}
