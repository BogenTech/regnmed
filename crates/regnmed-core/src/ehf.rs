//! EHF / PEPPOL BIS Billing 3.0 — outgoing faktura and kreditnota
//! (docs/ehf.md, #14).
//!
//! EHF is UBL 2.1 with a PEPPOL profile: mandatory towards the public
//! sector, expected in B2B. The document is rendered **hand-rolled and
//! deterministically**, like SAF-T, the mva-melding and pain.001 — the
//! same reasoning: outgoing XML is a format we are answerable for, and a
//! generator we write ourselves has no hidden behavior. The schema
//! (UBL 2.1, vendored in docs/ehf/) is validated in tests and in CI.
//!
//! The element order is UBL's own sequence — the XSD accepts no other.
//! Amounts are integer øre right up to the closing two-decimal
//! formatting; no float is ever involved.
//!
//! **Honest limitation:** XSD validation proves the document is
//! well-formed UBL, not that it satisfies every PEPPOL BIS business rule
//! (those are Schematron, and are run by the access point on submission).
//! See docs/ehf.md.

use chrono::NaiveDate;

/// Norwegian orgnr as a PEPPOL participant id: ISO 6523 ICD 0192.
pub const SCHEME_ORGNR: &str = "0192";

const CUSTOMIZATION_ID: &str =
    "urn:cen.eu:en16931:2017#compliant#urn:fdc:peppol.eu:2017:poacc:billing:3.0";
const PROFILE_ID: &str = "urn:fdc:peppol.eu:2017:poacc:billing:01:1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dokumenttype {
    Faktura,
    Kreditnota,
}

impl Dokumenttype {
    fn root(self) -> &'static str {
        match self {
            Self::Faktura => "Invoice",
            Self::Kreditnota => "CreditNote",
        }
    }

    fn namespace(self) -> &'static str {
        match self {
            Self::Faktura => "urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
            Self::Kreditnota => "urn:oasis:names:specification:ubl:schema:xsd:CreditNote-2",
        }
    }

    /// UNCL1001: 380 commercial invoice, 381 credit note.
    fn type_code(self) -> &'static str {
        match self {
            Self::Faktura => "380",
            Self::Kreditnota => "381",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EhfPart {
    pub navn: String,
    pub orgnr: String,
    /// Street address; EHF requires an address with a country code.
    pub adresse: Option<String>,
    pub postnr: Option<String>,
    pub poststed: Option<String>,
    /// ISO 3166-1 alpha-2; "NO" when unknown.
    pub land: String,
    /// Registered in Merverdiavgiftsregisteret (yields PartyTaxScheme).
    pub mva_registrert: bool,
    pub epost: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EhfLinje {
    pub beskrivelse: String,
    /// Thousandths: 1 unit = 1000.
    pub antall_milli: i64,
    /// UN/ECE Rec 20 unit code; "EA" (each) by default.
    pub enhet: String,
    pub enhetspris_ore: i64,
    pub netto_ore: i64,
    /// Basis points; None = no mva code on the line.
    pub mva_sats_bp: Option<i64>,
    pub mva_ore: i64,
}

#[derive(Debug, Clone)]
pub struct EhfDokument {
    pub dokumenttype: Dokumenttype,
    pub fakturanr: String,
    /// The invoice number a kreditnota credits.
    pub krediterer: Option<String>,
    pub fakturadato: NaiveDate,
    pub forfallsdato: Option<NaiveDate>,
    /// ISO 4217; "NOK" domestically.
    pub valuta: String,
    /// Buyer's reference (EN 16931 BT-10) — many public recipients
    /// reject invoices without it, so we send what we have.
    pub kjopers_referanse: Option<String>,
    pub selger: EhfPart,
    pub kjoper: EhfPart,
    pub kontonummer: Option<String>,
    pub kid: Option<String>,
    pub linjer: Vec<EhfLinje>,
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Integer øre → "1234.56". XML decimals always use a dot, whatever the
/// locale of the reader.
fn amount(ore: i64) -> String {
    let negative = ore < 0;
    let abs = ore.abs();
    format!(
        "{}{}.{:02}",
        if negative { "-" } else { "" },
        abs / 100,
        abs % 100
    )
}

/// Basis points → "25.00".
fn percent(bp: i64) -> String {
    let negative = bp < 0;
    let abs = bp.abs();
    format!(
        "{}{}.{:02}",
        if negative { "-" } else { "" },
        abs / 100,
        abs % 100
    )
}

/// Thousandths → "2.000".
fn quantity(milli: i64) -> String {
    let negative = milli < 0;
    let abs = milli.abs();
    format!(
        "{}{}.{:03}",
        if negative { "-" } else { "" },
        abs / 1000,
        abs % 1000
    )
}

/// EN 16931 tax category (UNCL5305). Standard rate → S, zero rate → Z.
/// Exemption (E) and reverse charge (AE) require a reason and are not
/// derived automatically — see docs/ehf.md.
fn tax_category(mva_sats_bp: Option<i64>) -> (&'static str, i64) {
    match mva_sats_bp {
        Some(bp) if bp > 0 => ("S", bp),
        _ => ("Z", 0),
    }
}

struct Writer {
    out: String,
    depth: usize,
}

impl Writer {
    fn new() -> Self {
        Self {
            out: String::new(),
            depth: 0,
        }
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
    }

    fn open(&mut self, tag: &str) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push_str(">\n");
        self.depth += 1;
    }

    fn close(&mut self, tag: &str) {
        self.depth -= 1;
        self.indent();
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
    }

    fn leaf(&mut self, tag: &str, value: &str) {
        self.indent();
        self.out.push_str(&format!(
            "<{tag}>{}</{}>\n",
            esc(value),
            tag.split(' ').next().unwrap()
        ));
    }

    fn leaf_attr(&mut self, tag: &str, attrs: &str, value: &str) {
        self.indent();
        self.out
            .push_str(&format!("<{tag} {attrs}>{}</{tag}>\n", esc(value)));
    }
}

fn write_party(w: &mut Writer, wrapper: &str, part: &EhfPart) {
    w.open(wrapper);
    w.open("cac:Party");
    w.leaf_attr(
        "cbc:EndpointID",
        &format!("schemeID=\"{SCHEME_ORGNR}\""),
        &part.orgnr,
    );
    w.open("cac:PartyIdentification");
    w.leaf_attr(
        "cbc:ID",
        &format!("schemeID=\"{SCHEME_ORGNR}\""),
        &part.orgnr,
    );
    w.close("cac:PartyIdentification");
    w.open("cac:PartyName");
    w.leaf("cbc:Name", &part.navn);
    w.close("cac:PartyName");
    w.open("cac:PostalAddress");
    if let Some(adresse) = &part.adresse {
        w.leaf("cbc:StreetName", adresse);
    }
    if let Some(poststed) = &part.poststed {
        w.leaf("cbc:CityName", poststed);
    }
    if let Some(postnr) = &part.postnr {
        w.leaf("cbc:PostalZone", postnr);
    }
    w.open("cac:Country");
    w.leaf("cbc:IdentificationCode", &part.land);
    w.close("cac:Country");
    w.close("cac:PostalAddress");
    if part.mva_registrert {
        w.open("cac:PartyTaxScheme");
        // A Norwegian mva id is the orgnr plus MVA.
        w.leaf("cbc:CompanyID", &format!("NO{}MVA", part.orgnr));
        w.open("cac:TaxScheme");
        w.leaf("cbc:ID", "VAT");
        w.close("cac:TaxScheme");
        w.close("cac:PartyTaxScheme");
    }
    w.open("cac:PartyLegalEntity");
    w.leaf("cbc:RegistrationName", &part.navn);
    w.leaf_attr(
        "cbc:CompanyID",
        &format!("schemeID=\"{SCHEME_ORGNR}\""),
        &part.orgnr,
    );
    w.close("cac:PartyLegalEntity");
    if let Some(epost) = &part.epost {
        w.open("cac:Contact");
        w.leaf("cbc:ElectronicMail", epost);
        w.close("cac:Contact");
    }
    w.close("cac:Party");
    w.close(wrapper);
}

/// Renders the EHF document. Deterministic: the same invoice yields
/// byte-identical XML forever.
pub fn render(doc: &EhfDokument) -> String {
    let root = doc.dokumenttype.root();
    let currency = &doc.valuta;
    let money = |ore: i64| -> String { amount(ore) };
    let attr = format!("currencyID=\"{}\"", esc(currency));

    let netto_sum: i64 = doc.linjer.iter().map(|l| l.netto_ore).sum();
    let mva_sum: i64 = doc.linjer.iter().map(|l| l.mva_ore).sum();

    let mut w = Writer::new();
    w.out
        .push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    w.out.push_str(&format!(
        "<{root} xmlns=\"{}\" \
         xmlns:cac=\"urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2\" \
         xmlns:cbc=\"urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2\">\n",
        doc.dokumenttype.namespace()
    ));
    w.depth = 1;

    w.leaf("cbc:CustomizationID", CUSTOMIZATION_ID);
    w.leaf("cbc:ProfileID", PROFILE_ID);
    w.leaf("cbc:ID", &doc.fakturanr);
    w.leaf("cbc:IssueDate", &doc.fakturadato.to_string());
    if doc.dokumenttype == Dokumenttype::Faktura
        && let Some(forfall) = doc.forfallsdato
    {
        w.leaf("cbc:DueDate", &forfall.to_string());
    }
    match doc.dokumenttype {
        Dokumenttype::Faktura => w.leaf("cbc:InvoiceTypeCode", doc.dokumenttype.type_code()),
        Dokumenttype::Kreditnota => w.leaf("cbc:CreditNoteTypeCode", doc.dokumenttype.type_code()),
    }
    w.leaf("cbc:DocumentCurrencyCode", currency);
    if let Some(referanse) = &doc.kjopers_referanse {
        w.leaf("cbc:BuyerReference", referanse);
    }
    if let Some(kreditert) = &doc.krediterer {
        w.open("cac:BillingReference");
        w.open("cac:InvoiceDocumentReference");
        w.leaf("cbc:ID", kreditert);
        w.close("cac:InvoiceDocumentReference");
        w.close("cac:BillingReference");
    }

    write_party(&mut w, "cac:AccountingSupplierParty", &doc.selger);
    write_party(&mut w, "cac:AccountingCustomerParty", &doc.kjoper);

    if doc.dokumenttype == Dokumenttype::Faktura
        && let Some(konto) = &doc.kontonummer
    {
        w.open("cac:PaymentMeans");
        // UNCL4461 30: credit transfer.
        w.leaf("cbc:PaymentMeansCode", "30");
        if let Some(kid) = &doc.kid {
            w.leaf("cbc:PaymentID", kid);
        }
        w.open("cac:PayeeFinancialAccount");
        w.leaf("cbc:ID", konto);
        w.close("cac:PayeeFinancialAccount");
        w.close("cac:PaymentMeans");
    }

    // One TaxSubtotal per rate — the recipient reconciles per rate.
    let mut satser: Vec<(&'static str, i64)> = Vec::new();
    for linje in &doc.linjer {
        let key = tax_category(linje.mva_sats_bp);
        if !satser.contains(&key) {
            satser.push(key);
        }
    }
    w.open("cac:TaxTotal");
    w.leaf_attr("cbc:TaxAmount", &attr, &money(mva_sum));
    for (kategori, bp) in &satser {
        let grunnlag: i64 = doc
            .linjer
            .iter()
            .filter(|l| tax_category(l.mva_sats_bp) == (*kategori, *bp))
            .map(|l| l.netto_ore)
            .sum();
        let avgift: i64 = doc
            .linjer
            .iter()
            .filter(|l| tax_category(l.mva_sats_bp) == (*kategori, *bp))
            .map(|l| l.mva_ore)
            .sum();
        w.open("cac:TaxSubtotal");
        w.leaf_attr("cbc:TaxableAmount", &attr, &money(grunnlag));
        w.leaf_attr("cbc:TaxAmount", &attr, &money(avgift));
        w.open("cac:TaxCategory");
        w.leaf("cbc:ID", kategori);
        w.leaf("cbc:Percent", &percent(*bp));
        w.open("cac:TaxScheme");
        w.leaf("cbc:ID", "VAT");
        w.close("cac:TaxScheme");
        w.close("cac:TaxCategory");
        w.close("cac:TaxSubtotal");
    }
    w.close("cac:TaxTotal");

    w.open("cac:LegalMonetaryTotal");
    w.leaf_attr("cbc:LineExtensionAmount", &attr, &money(netto_sum));
    w.leaf_attr("cbc:TaxExclusiveAmount", &attr, &money(netto_sum));
    w.leaf_attr("cbc:TaxInclusiveAmount", &attr, &money(netto_sum + mva_sum));
    w.leaf_attr("cbc:PayableAmount", &attr, &money(netto_sum + mva_sum));
    w.close("cac:LegalMonetaryTotal");

    let line_tag = match doc.dokumenttype {
        Dokumenttype::Faktura => "cac:InvoiceLine",
        Dokumenttype::Kreditnota => "cac:CreditNoteLine",
    };
    let quantity_tag = match doc.dokumenttype {
        Dokumenttype::Faktura => "cbc:InvoicedQuantity",
        Dokumenttype::Kreditnota => "cbc:CreditedQuantity",
    };
    for (i, linje) in doc.linjer.iter().enumerate() {
        let (kategori, bp) = tax_category(linje.mva_sats_bp);
        w.open(line_tag);
        w.leaf("cbc:ID", &(i + 1).to_string());
        w.leaf_attr(
            quantity_tag,
            &format!("unitCode=\"{}\"", esc(&linje.enhet)),
            &quantity(linje.antall_milli),
        );
        w.leaf_attr("cbc:LineExtensionAmount", &attr, &money(linje.netto_ore));
        w.open("cac:Item");
        w.leaf("cbc:Name", &linje.beskrivelse);
        w.open("cac:ClassifiedTaxCategory");
        w.leaf("cbc:ID", kategori);
        w.leaf("cbc:Percent", &percent(bp));
        w.open("cac:TaxScheme");
        w.leaf("cbc:ID", "VAT");
        w.close("cac:TaxScheme");
        w.close("cac:ClassifiedTaxCategory");
        w.close("cac:Item");
        w.open("cac:Price");
        w.leaf_attr("cbc:PriceAmount", &attr, &money(linje.enhetspris_ore));
        w.close("cac:Price");
        w.close(line_tag);
    }

    w.out.push_str(&format!("</{root}>\n"));
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(navn: &str, orgnr: &str, mva: bool) -> EhfPart {
        EhfPart {
            navn: navn.into(),
            orgnr: orgnr.into(),
            adresse: Some("Storgata 1".into()),
            postnr: Some("0155".into()),
            poststed: Some("Oslo".into()),
            land: "NO".into(),
            mva_registrert: mva,
            epost: Some("post@example.no".into()),
        }
    }

    fn dokument(dokumenttype: Dokumenttype) -> EhfDokument {
        EhfDokument {
            dokumenttype,
            fakturanr: "1001".into(),
            krediterer: (dokumenttype == Dokumenttype::Kreditnota).then(|| "1000".to_string()),
            fakturadato: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            forfallsdato: NaiveDate::from_ymd_opt(2026, 7, 15),
            valuta: "NOK".into(),
            kjopers_referanse: Some("Bestilling 42".into()),
            selger: part("Selger AS", "915933149", true),
            kjoper: part("Kjøper & Sønn AS", "923609016", true),
            kontonummer: Some("86011117947".into()),
            kid: Some("1234567897".into()),
            linjer: vec![
                EhfLinje {
                    beskrivelse: "Konsulentbistand".into(),
                    antall_milli: 2_000,
                    enhet: "HUR".into(),
                    enhetspris_ore: 1_250_00,
                    netto_ore: 2_500_00,
                    mva_sats_bp: Some(2500),
                    mva_ore: 625_00,
                },
                EhfLinje {
                    beskrivelse: "Utlegg uten mva".into(),
                    antall_milli: 1_000,
                    enhet: "EA".into(),
                    enhetspris_ore: 500_00,
                    netto_ore: 500_00,
                    mva_sats_bp: None,
                    mva_ore: 0,
                },
            ],
        }
    }

    #[test]
    fn the_invoice_carries_the_peppol_profile_and_a_norwegian_participant_id() {
        let xml = render(&dokument(Dokumenttype::Faktura));
        assert!(xml.contains(CUSTOMIZATION_ID));
        assert!(xml.contains(PROFILE_ID));
        assert!(xml.contains("<cbc:EndpointID schemeID=\"0192\">915933149</cbc:EndpointID>"));
        assert!(xml.contains("<cbc:CompanyID>NO915933149MVA</cbc:CompanyID>"));
        assert!(xml.contains("<cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>"));
        assert!(xml.contains("<cbc:PaymentID>1234567897</cbc:PaymentID>"));
    }

    #[test]
    fn amounts_are_integer_ore_with_a_dot_and_a_currency_code() {
        let xml = render(&dokument(Dokumenttype::Faktura));
        assert!(xml.contains(
            "<cbc:TaxInclusiveAmount currencyID=\"NOK\">3625.00</cbc:TaxInclusiveAmount>"
        ));
        assert!(xml.contains("<cbc:PayableAmount currencyID=\"NOK\">3625.00</cbc:PayableAmount>"));
        assert!(xml.contains(
            "<cbc:LineExtensionAmount currencyID=\"NOK\">3000.00</cbc:LineExtensionAmount>"
        ));
        assert!(!xml.contains(','), "desimalskilletegn i XML er punktum");
    }

    #[test]
    fn one_tax_group_per_rate() {
        let xml = render(&dokument(Dokumenttype::Faktura));
        assert_eq!(xml.matches("<cac:TaxSubtotal>").count(), 2);
        assert!(xml.contains("<cbc:TaxableAmount currencyID=\"NOK\">2500.00</cbc:TaxableAmount>"));
        assert!(xml.contains("<cbc:Percent>25.00</cbc:Percent>"));
        // The line without an mva code becomes zero-rated, not omitted.
        assert!(xml.contains("<cbc:ID>Z</cbc:ID>"));
        assert!(xml.contains("<cbc:TaxableAmount currencyID=\"NOK\">500.00</cbc:TaxableAmount>"));
    }

    #[test]
    fn a_kreditnota_uses_its_own_element_names_and_points_at_the_faktura() {
        let xml = render(&dokument(Dokumenttype::Kreditnota));
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CreditNote "));
        assert!(xml.contains("<cbc:CreditNoteTypeCode>381</cbc:CreditNoteTypeCode>"));
        assert!(xml.contains("<cac:CreditNoteLine>"));
        assert!(
            xml.contains("<cbc:CreditedQuantity unitCode=\"HUR\">2.000</cbc:CreditedQuantity>")
        );
        assert!(
            xml.contains("<cac:InvoiceDocumentReference>\n      <cbc:ID>1000</cbc:ID>"),
            "kreditnotaen navngir fakturaen den krediterer"
        );
        assert!(
            !xml.contains("DueDate"),
            "kreditnota har ingen forfallsdato"
        );
        assert!(
            !xml.contains("PaymentMeans"),
            "og ingen betalingsinformasjon"
        );
    }

    #[test]
    fn characters_needing_escaping_never_leak_through() {
        let xml = render(&dokument(Dokumenttype::Faktura));
        assert!(xml.contains("Kjøper &amp; Sønn AS"));
        assert!(!xml.contains("Kjøper & Sønn"));
    }

    #[test]
    fn the_same_faktura_yields_identical_xml() {
        let a = render(&dokument(Dokumenttype::Faktura));
        let b = render(&dokument(Dokumenttype::Faktura));
        assert_eq!(a, b, "renderen er deterministisk");
    }

    /// The XSD is the authority; xmllint runs it in CI as well.
    #[test]
    fn validates_against_the_ubl_schema() {
        for (dokumenttype, schema) in [
            (Dokumenttype::Faktura, "maindoc/UBL-Invoice-2.1.xsd"),
            (Dokumenttype::Kreditnota, "maindoc/UBL-CreditNote-2.1.xsd"),
        ] {
            let xml = render(&dokument(dokumenttype));
            let dir = std::env::temp_dir().join(format!("regnmed-ehf-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{}.xml", dokumenttype.root()));
            std::fs::write(&path, &xml).unwrap();
            let xsd = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ehf/");
            let output = std::process::Command::new("xmllint")
                .arg("--noout")
                .arg("--schema")
                .arg(format!("{xsd}{schema}"))
                .arg(&path)
                .output();
            let Ok(output) = output else {
                eprintln!("xmllint ikke installert — hopper over skjemavalidering");
                return;
            };
            assert!(
                output.status.success(),
                "{} feilet skjemavalidering:\n{}",
                dokumenttype.root(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
