//! EHF inn: en mottatt faktura leses til et **bokføringsforslag**
//! (docs/ehf.md, #14).
//!
//! Dette er den strukturerte enden av bilagstolkning (#34): når
//! dokumentet er EHF, er ikke leverandør, beløp og mva gjetning — de
//! står i filen. Parseren er tolerant i camt.053-stil (den leser bare
//! det bokføringen trenger, hopper over resten og godtar både Invoice
//! og CreditNote), fordi vi ikke kontrollerer avsenderen.
//!
//! Forslaget er nettopp et forslag: mennesket bokfører (eller avviser)
//! det gjennom innboksen som ethvert annet bilag. Ingenting utledet
//! herfra lagres — originalen er bevaret, resten regnes ut på nytt hver
//! gang det spørres.

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
    pub beskrivelse: String,
    pub netto_ore: i64,
    /// Basispunkter fra ClassifiedTaxCategory når den finnes.
    pub mva_sats_bp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MottattFaktura {
    /// True for CreditNote — beløpene snus når bilaget foreslås.
    pub er_kreditnota: bool,
    pub fakturanr: String,
    pub fakturadato: Option<NaiveDate>,
    pub forfallsdato: Option<NaiveDate>,
    pub valuta: String,
    pub selger_navn: String,
    /// Norsk orgnr uten prefiks når avsender bruker ICD 0192.
    pub selger_orgnr: Option<String>,
    pub kjoper_orgnr: Option<String>,
    pub kid: Option<String>,
    pub kontonummer: Option<String>,
    pub netto_ore: i64,
    pub mva_ore: i64,
    /// PayableAmount — det leverandøren faktisk krever.
    pub brutto_ore: i64,
    pub linjer: Vec<MottattLinje>,
}

/// "1234.56" / "1234,56" / "1234" → øre. Avsenderen bestemmer formatet;
/// vi nekter å gjette på mer enn to desimaler.
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
    let mut faktura = MottattFaktura {
        er_kreditnota: false,
        fakturanr: String::new(),
        fakturadato: None,
        forfallsdato: None,
        valuta: "NOK".into(),
        selger_navn: String::new(),
        selger_orgnr: None,
        kjoper_orgnr: None,
        kid: None,
        kontonummer: None,
        netto_ore: 0,
        mva_ore: 0,
        brutto_ore: 0,
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
                            faktura.er_kreditnota = true;
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
                    faktura.linjer.push(MottattLinje {
                        beskrivelse: std::mem::take(&mut line_name),
                        netto_ore: line_net,
                        mva_sats_bp: line_bp,
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
                    "ID" if path.len() == 2 => faktura.fakturanr = value,
                    "IssueDate" if path.len() == 2 => faktura.fakturadato = Some(date(&value)?),
                    "DueDate" if path.len() == 2 => faktura.forfallsdato = Some(date(&value)?),
                    "DocumentCurrencyCode" if path.len() == 2 => faktura.valuta = value,
                    "PaymentID" => faktura.kid = Some(value),
                    "Name" if supplier && ends_with(&path, &["PartyName", "Name"]) => {
                        if !supplier_name_from_legal {
                            faktura.selger_navn = value;
                        }
                    }
                    "RegistrationName" if supplier => {
                        faktura.selger_navn = value;
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
                        let orgnr = if digits.len() >= 9 {
                            Some(digits[digits.len() - 9..].to_string())
                        } else {
                            None
                        };
                        if supplier {
                            faktura.selger_orgnr = faktura.selger_orgnr.take().or(orgnr);
                        } else {
                            faktura.kjoper_orgnr = faktura.kjoper_orgnr.take().or(orgnr);
                        }
                    }
                    "ID" if ends_with(&path, &["PayeeFinancialAccount", "ID"]) => {
                        faktura.kontonummer = Some(value);
                    }
                    "TaxExclusiveAmount" => faktura.netto_ore = amount(&value)?,
                    "TaxInclusiveAmount" if faktura.brutto_ore == 0 => {
                        faktura.brutto_ore = amount(&value)?;
                    }
                    "PayableAmount" => faktura.brutto_ore = amount(&value)?,
                    "TaxAmount" if ends_with(&path, &["TaxTotal", "TaxAmount"]) => {
                        faktura.mva_ore = amount(&value)?;
                    }
                    "LineExtensionAmount" if in_line => line_net = amount(&value)?,
                    "LineExtensionAmount" if faktura.netto_ore == 0 => {
                        faktura.netto_ore = amount(&value)?;
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
    if faktura.netto_ore == 0 && !faktura.linjer.is_empty() {
        faktura.netto_ore = faktura.linjer.iter().map(|l| l.netto_ore).sum();
    }
    if faktura.brutto_ore == 0 {
        faktura.brutto_ore = faktura.netto_ore + faktura.mva_ore;
    }
    Ok(faktura)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ehf::{Dokumenttype, EhfDokument, EhfLinje, EhfPart, render};

    fn part(navn: &str, orgnr: &str) -> EhfPart {
        EhfPart {
            navn: navn.into(),
            orgnr: orgnr.into(),
            adresse: Some("Storgata 1".into()),
            postnr: Some("0155".into()),
            poststed: Some("Oslo".into()),
            land: "NO".into(),
            mva_registrert: true,
            epost: None,
        }
    }

    fn dokument(dokumenttype: Dokumenttype) -> EhfDokument {
        EhfDokument {
            dokumenttype,
            fakturanr: "F-2026-7".into(),
            krediterer: None,
            fakturadato: NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(),
            forfallsdato: NaiveDate::from_ymd_opt(2026, 7, 17),
            valuta: "NOK".into(),
            kjopers_referanse: None,
            selger: part("Grossisten AS", "915933149"),
            kjoper: part("Vår Klient AS", "923609016"),
            kontonummer: Some("86011117947".into()),
            kid: Some("1234567897".into()),
            linjer: vec![
                EhfLinje {
                    beskrivelse: "Varekjøp".into(),
                    antall_milli: 3_000,
                    enhet: "EA".into(),
                    enhetspris_ore: 400_00,
                    netto_ore: 1_200_00,
                    mva_sats_bp: Some(2500),
                    mva_ore: 300_00,
                },
                EhfLinje {
                    beskrivelse: "Frakt".into(),
                    antall_milli: 1_000,
                    enhet: "EA".into(),
                    enhetspris_ore: 100_00,
                    netto_ore: 100_00,
                    mva_sats_bp: Some(2500),
                    mva_ore: 25_00,
                },
            ],
        }
    }

    /// Round-trip against our own renderer, the same guarantee the SAF-T
    /// importer carries: what we write, we can read.
    #[test]
    fn leser_var_egen_faktura_tilbake() {
        let xml = render(&dokument(Dokumenttype::Faktura));
        let mottatt = parse(&xml).unwrap();
        assert!(!mottatt.er_kreditnota);
        assert_eq!(mottatt.fakturanr, "F-2026-7");
        assert_eq!(
            mottatt.fakturadato,
            Some(NaiveDate::from_ymd_opt(2026, 7, 3).unwrap())
        );
        assert_eq!(
            mottatt.forfallsdato,
            Some(NaiveDate::from_ymd_opt(2026, 7, 17).unwrap())
        );
        assert_eq!(mottatt.selger_navn, "Grossisten AS");
        assert_eq!(mottatt.selger_orgnr.as_deref(), Some("915933149"));
        assert_eq!(mottatt.kjoper_orgnr.as_deref(), Some("923609016"));
        assert_eq!(mottatt.kid.as_deref(), Some("1234567897"));
        assert_eq!(mottatt.kontonummer.as_deref(), Some("86011117947"));
        assert_eq!(mottatt.netto_ore, 1_300_00);
        assert_eq!(mottatt.mva_ore, 325_00);
        assert_eq!(mottatt.brutto_ore, 1_625_00);
        assert_eq!(mottatt.linjer.len(), 2);
        assert_eq!(mottatt.linjer[0].beskrivelse, "Varekjøp");
        assert_eq!(mottatt.linjer[0].netto_ore, 1_200_00);
        assert_eq!(mottatt.linjer[0].mva_sats_bp, Some(2500));
    }

    #[test]
    fn kreditnota_kjennes_igjen() {
        let xml = render(&dokument(Dokumenttype::Kreditnota));
        let mottatt = parse(&xml).unwrap();
        assert!(mottatt.er_kreditnota);
        assert_eq!(mottatt.linjer.len(), 2);
        assert_eq!(mottatt.brutto_ore, 1_625_00);
    }

    /// A real EHF from another system: different prefixes, extra
    /// elements, no line-level tax category.
    #[test]
    fn tolererer_fremmed_dokument_med_ukjente_elementer() {
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
        let mottatt = parse(xml).unwrap();
        assert_eq!(mottatt.fakturanr, "90210");
        assert_eq!(
            mottatt.selger_navn, "Handelshuset AS",
            "RegistrationName vinner over PartyName"
        );
        assert_eq!(mottatt.selger_orgnr.as_deref(), Some("974760673"));
        assert_eq!(mottatt.kjoper_orgnr.as_deref(), Some("923609016"));
        assert_eq!(mottatt.kid.as_deref(), Some("0000000123456"));
        assert_eq!(mottatt.netto_ore, 2_000_00);
        assert_eq!(mottatt.mva_ore, 500_00);
        assert_eq!(mottatt.brutto_ore, 2_500_00);
        assert_eq!(mottatt.linjer[0].beskrivelse, "Kontorstoler");
        assert_eq!(mottatt.linjer[0].mva_sats_bp, None, "ingen sats på linjen");
    }

    #[test]
    fn noe_som_ikke_er_ehf_avvises() {
        let error = parse("<?xml version=\"1.0\"?><Document><Nope/></Document>").unwrap_err();
        assert!(matches!(error, EhfImportError::NotEhf));
    }

    #[test]
    fn belop_uten_desimaler_og_med_komma_leses() {
        assert_eq!(amount("2000").unwrap(), 200_000);
        assert_eq!(amount("2000.5").unwrap(), 200_050);
        assert_eq!(amount("-12,25").unwrap(), -1225);
        assert!(amount("12.345").is_err(), "tre desimaler er ikke øre");
    }
}
