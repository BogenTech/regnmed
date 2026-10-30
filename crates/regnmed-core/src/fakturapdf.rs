//! Faktura/kreditnota as PDF (docs/faktura.md) and the purring document
//! as PDF (docs/purring.md) — pure layout over [`crate::pdf`].
//!
//! The content follows bokføringsforskriften §5-1-1: number and date, the
//! parties (the seller's orgnr with the "MVA" suffix when registered,
//! "Foretaksregisteret" for entities registered there), the nature and
//! extent of what was supplied, the consideration with its forfall, and
//! merverdiavgift specified in NOK per rate. Rendering is deterministic —
//! the PDF stored on the bilag at issue can be reproduced byte for byte.

use chrono::NaiveDate;

use crate::Ore;
use crate::pdf::{Font, PAGE_HEIGHT, Pdf};

const MARGIN: f32 = 50.0;
const RIGHT: f32 = 545.0;

#[derive(Debug, Clone)]
pub struct PdfLinje {
    pub description: String,
    /// Tusendeler: 1 stk = 1000.
    pub quantity_milli: i64,
    pub enhetspris_ore: i64,
    /// Basis points; None for lines without an mva code.
    pub vat_rate_bp: Option<i64>,
    pub net_ore: i64,
    pub vat_ore: i64,
}

/// The document kinds this layout renders. Faktura and Kreditnota are
/// regnskapsdokumenter (issued, stored, hash-verified); Tilbud and
/// Ordrebekreftelse are the commercial chain BEFORE the invoice (#31) —
/// no KID, no betalingsinformasjon, rendered on demand from editable
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    Invoice,
    CreditNote,
    Quote,
    Ordrebekreftelse,
    /// Kontantfaktura (bokføringsforskriften §5-3): the ytelse was paid
    /// on delivery. Carries NO KID, NO forfallsdato and no payment
    /// instructions — telling someone to pay what they already paid is
    /// not a detail, it is a wrong document.
    Kontantfaktura,
}

impl DocumentType {
    fn tittel(self) -> &'static str {
        match self {
            DocumentType::Invoice => "FAKTURA",
            DocumentType::CreditNote => "KREDITNOTA",
            DocumentType::Quote => "TILBUD",
            DocumentType::Ordrebekreftelse => "ORDREBEKREFTELSE",
            DocumentType::Kontantfaktura => "KONTANTFAKTURA",
        }
    }

    fn nummerord(self) -> &'static str {
        match self {
            DocumentType::Invoice => "Fakturanr",
            DocumentType::CreditNote => "Kreditnotanr",
            DocumentType::Quote => "Tilbudsnr",
            DocumentType::Ordrebekreftelse => "Ordrenr",
            DocumentType::Kontantfaktura => "Fakturanr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InvoicePdfInput {
    pub document_type: DocumentType,
    /// The invoice number of the faktura this one credits.
    pub credits_number: Option<i64>,

    pub seller_name: String,
    pub seller_orgnumber: String,
    pub seller_address: Option<String>,
    pub seller_vat_registered: bool,
    /// "Foretaksregisteret" is added for entities registered there (AS/ASA,
    /// foretaksregisterloven §10-2).
    pub selger_foretaksregistrert: bool,
    pub seller_account_number: Option<String>,

    pub buyer_name: String,
    pub buyer_number: String,
    pub buyer_orgnumber: Option<String>,
    pub buyer_address: Option<String>,

    pub fakturanr: i64,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    /// Leveringstidspunkt — bokføringsforskriften §5-1-1 nr. 4 requires
    /// it on every salgsdokument. `None` only for invoices issued
    /// before the field existed (#81): their stored PDF is what it is,
    /// and the document renders without the line rather than inventing
    /// a date it never recorded.
    pub delivery_date: Option<NaiveDate>,
    /// Leveringssted, same hjemmel — "der det er relevant".
    pub leveringssted: Option<String>,
    /// None when the seller has no KID/OCR agreement with the bank: a
    /// KID the payer's bank cannot accept is a false instruction, not a
    /// harmless extra line (#104).
    pub kid: Option<String>,
    /// How a kontantfaktura was settled ("Kort", "Vipps", "Kontant").
    /// Ignored on every other dokumenttype.
    pub betalingsmiddel: Option<String>,

    /// Dokumentvaluta (docs/valuta.md). None = NOK. When set, every
    /// beløp on the document is in this currency and the totals block
    /// states it together with the NOK motverdi.
    pub currency: Option<String>,
    /// NOK gross actually posted, shown as motverdi on valuta invoices.
    pub motverdi_nok_ore: Option<i64>,

    pub linjer: Vec<PdfLinje>,
}

fn kr(ore: i64) -> String {
    // Kroner with thin thousands grouping: 1234567,89 → "1 234 567,89".
    let raw = Ore(ore).to_string();
    let (heltall, rest) = raw.split_once(',').expect("Ore always has decimals");
    let (sign, digits) = heltall
        .strip_prefix('-')
        .map_or(("", heltall), |d| ("-", d));
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped},{rest}")
}

fn quantity(milli: i64) -> String {
    if milli % 1000 == 0 {
        format!("{}", milli / 1000)
    } else {
        format!(
            "{},{}",
            milli / 1000,
            format!("{:03}", (milli % 1000).abs()).trim_end_matches('0')
        )
    }
}

fn prosent(bp: i64) -> String {
    if bp % 100 == 0 {
        format!("{} %", bp / 100)
    } else {
        format!("{},{:02} %", bp / 100, bp % 100)
    }
}

/// Orgnr grouped 3-3-3 as customary: "999888777" → "999 888 777".
fn orgnumber_display(orgnumber: &str) -> String {
    if orgnumber.len() == 9 {
        format!(
            "{} {} {}",
            &orgnumber[..3],
            &orgnumber[3..6],
            &orgnumber[6..]
        )
    } else {
        orgnumber.to_string()
    }
}

pub fn render_invoice_pdf(input: &InvoicePdfInput) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let tittel = input.document_type.tittel();
    let kreditnota = input.document_type == DocumentType::CreditNote;

    // Seller block, top left.
    pdf.text(MARGIN, 60.0, 13.0, Font::Bold, &input.seller_name);
    let mut y = 76.0;
    if let Some(address) = &input.seller_address {
        pdf.text(MARGIN, y, 9.0, Font::Regular, address);
        y += 12.0;
    }
    let mut orglinje = format!("Org.nr {}", orgnumber_display(&input.seller_orgnumber));
    if input.seller_vat_registered {
        orglinje.push_str(" MVA");
    }
    pdf.text(MARGIN, y, 9.0, Font::Regular, &orglinje);
    if input.selger_foretaksregistrert {
        pdf.text(MARGIN, y + 12.0, 9.0, Font::Regular, "Foretaksregisteret");
    }

    // Title + document facts, top right.
    pdf.text_right(RIGHT, 60.0, 16.0, Font::Bold, tittel);
    let mut fakta: Vec<(String, String)> = vec![
        (
            input.document_type.nummerord().into(),
            input.fakturanr.to_string(),
        ),
        ("Dato".into(), input.invoice_date.to_string()),
    ];
    if input.document_type == DocumentType::Invoice {
        fakta.push(("Forfall".into(), input.due_date.to_string()));
    }
    // A kontantfaktura says what settled it, where a faktura says when
    // it falls due. §5-3 wants the salgsdokument to show that the
    // ytelse is paid.
    if input.document_type == DocumentType::Kontantfaktura {
        fakta.push((
            "Betalt".into(),
            input.betalingsmiddel.clone().unwrap_or_else(|| "Ja".into()),
        ));
    }
    // Leveringstidspunkt is mandatory on the salgsdokument, so it sits
    // with the other document facts rather than in a footnote. On a
    // kreditnota it points at the ORIGINAL delivery — the one being
    // credited — not at the day the correction was written.
    if let Some(levering) = input.delivery_date {
        fakta.push(("Levering".into(), levering.to_string()));
    }
    if let Some(number) = input.credits_number {
        fakta.push(("Krediterer faktura".into(), number.to_string()));
    }
    let mut fy = 82.0;
    for (label, value) in &fakta {
        pdf.text_right(RIGHT - 70.0, fy, 9.0, Font::Regular, label);
        pdf.text_right(RIGHT, fy, 9.0, Font::Bold, value);
        fy += 12.0;
    }
    // Leveringsstedet er fri tekst og hører ikke hjemme i faktablokken:
    // den gir verdien 70 pt før etiketten, nok til en dato og ikke til
    // en adresse, så en adresse ville lagt seg OVER etiketten. Det er
    // en hel linje i stedet, høyrejustert som resten.
    if let Some(sted) = &input.leveringssted {
        pdf.text_right(
            RIGHT,
            fy,
            9.0,
            Font::Regular,
            &format!("Leveringssted: {sted}"),
        );
    }

    // Buyer block.
    pdf.text(MARGIN, 130.0, 8.0, Font::Bold, "FAKTURAMOTTAKER");
    pdf.text(MARGIN, 144.0, 11.0, Font::Regular, &input.buyer_name);
    let mut ky = 158.0;
    if let Some(address) = &input.buyer_address {
        pdf.text(MARGIN, ky, 9.0, Font::Regular, address);
        ky += 12.0;
    }
    if let Some(orgnumber) = &input.buyer_orgnumber {
        pdf.text(
            MARGIN,
            ky,
            9.0,
            Font::Regular,
            &format!("Org.nr {}", orgnumber_display(orgnumber)),
        );
        ky += 12.0;
    }
    pdf.text(
        MARGIN,
        ky,
        9.0,
        Font::Regular,
        &format!("Kundenr {}", input.buyer_number),
    );

    // Line table. Columns: description | quantity | price | VAT | amount.
    let col_quantity = 330.0;
    let col_price = 405.0;
    let col_vat = 460.0;
    let table_top = 210.0;
    let mut ly = table_top;
    let header = |pdf: &mut Pdf, y: f32| {
        pdf.text(MARGIN, y, 8.0, Font::Bold, "Beskrivelse");
        pdf.text_right(col_quantity, y, 8.0, Font::Bold, "Antall");
        pdf.text_right(col_price, y, 8.0, Font::Bold, "Pris");
        pdf.text_right(col_vat, y, 8.0, Font::Bold, "Mva");
        pdf.text_right(RIGHT, y, 8.0, Font::Bold, "Beløp");
        pdf.rule(MARGIN, RIGHT, y + 5.0, 0.7);
    };
    header(&mut pdf, ly);
    ly += 20.0;
    for linje in &input.linjer {
        if ly > PAGE_HEIGHT - 160.0 {
            pdf.next_page();
            ly = 60.0;
            header(&mut pdf, ly);
            ly += 20.0;
        }
        pdf.text(MARGIN, ly, 9.0, Font::Regular, &linje.description);
        pdf.text_right(
            col_quantity,
            ly,
            9.0,
            Font::Regular,
            &quantity(linje.quantity_milli),
        );
        pdf.text_right(col_price, ly, 9.0, Font::Regular, &kr(linje.enhetspris_ore));
        if let Some(bp) = linje.vat_rate_bp {
            pdf.text_right(col_vat, ly, 9.0, Font::Regular, &prosent(bp));
        }
        pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(linje.net_ore));
        ly += 15.0;
    }
    pdf.rule(MARGIN, RIGHT, ly - 5.0, 0.7);
    ly += 8.0;

    // Totals + mva spesifisert i NOK per sats (§5-1-1 nr. 6).
    let net: i64 = input.linjer.iter().map(|l| l.net_ore).sum();
    let vat: i64 = input.linjer.iter().map(|l| l.vat_ore).sum();
    let gross = net + vat;
    pdf.text_right(RIGHT - 110.0, ly, 9.0, Font::Regular, "Netto");
    pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(net));
    ly += 14.0;
    let mut rates: Vec<i64> = input.linjer.iter().filter_map(|l| l.vat_rate_bp).collect();
    rates.sort_unstable();
    rates.dedup();
    for rate in rates {
        let grunnlag: i64 = input
            .linjer
            .iter()
            .filter(|l| l.vat_rate_bp == Some(rate))
            .map(|l| l.net_ore)
            .sum();
        let avgift: i64 = input
            .linjer
            .iter()
            .filter(|l| l.vat_rate_bp == Some(rate))
            .map(|l| l.vat_ore)
            .sum();
        pdf.text_right(
            RIGHT - 110.0,
            ly,
            9.0,
            Font::Regular,
            &format!("Mva {} av {}", prosent(rate), kr(grunnlag)),
        );
        pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(avgift));
        ly += 14.0;
    }
    pdf.rule(RIGHT - 220.0, RIGHT, ly - 4.0, 0.7);
    ly += 6.0;
    let betales = if kreditnota {
        "Å godskrive"
    } else {
        "Å betale"
    };
    pdf.text_right(RIGHT - 110.0, ly, 11.0, Font::Bold, betales);
    pdf.text_right(RIGHT, ly, 11.0, Font::Bold, &kr(gross));
    if let Some(currency) = &input.currency {
        ly += 14.0;
        let note = match input.motverdi_nok_ore {
            Some(nok) => format!("Alle beløp i {currency}. Motverdi NOK {}", kr(nok)),
            None => format!("Alle beløp i {currency}"),
        };
        pdf.text_right(RIGHT, ly, 8.0, Font::Regular, &note);
    }

    // Payment block at a fixed distance below the totals.
    // Betalingsinformasjon only on an actual faktura — tilbud/ordre are
    // not payable and kreditnotaer are settlements.
    if input.document_type == DocumentType::Invoice {
        ly += 30.0;
        pdf.rule(MARGIN, RIGHT, ly, 0.7);
        ly += 16.0;
        pdf.text(MARGIN, ly, 8.0, Font::Bold, "BETALINGSINFORMASJON");
        ly += 14.0;
        if let Some(account) = &input.seller_account_number {
            pdf.text(
                MARGIN,
                ly,
                9.0,
                Font::Regular,
                &format!("Kontonummer: {account}"),
            );
            ly += 13.0;
        }
        // With a KID the payer has a machine-readable reference. Without
        // one they need to be TOLD what to write, or the payment arrives
        // with an empty message and someone has to guess which claim it
        // settles (#104).
        match &input.kid {
            Some(kid) => {
                pdf.text(MARGIN, ly, 9.0, Font::Regular, &format!("KID: {kid}"));
            }
            None => {
                pdf.text(
                    MARGIN,
                    ly,
                    9.0,
                    Font::Regular,
                    &format!("Merk betalingen med fakturanummer {}", input.fakturanr),
                );
            }
        }
        ly += 13.0;
        pdf.text(
            MARGIN,
            ly,
            9.0,
            Font::Regular,
            &format!("Betales innen {} — {}", input.due_date, kr(gross)),
        );
    }
    pdf.finish()
}

/// A stored plain-text document (purring/inkassovarsel) as PDF: Courier
/// line by line, so the columns in the text document keep their
/// alignment.
pub fn render_tekst_pdf(document: &str) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let mut y = 60.0;
    for line in document.lines() {
        if y > PAGE_HEIGHT - 60.0 {
            pdf.next_page();
            y = 60.0;
        }
        if !line.is_empty() {
            pdf.text(MARGIN, y, 9.0, Font::Mono, line);
        }
        y += 12.5;
    }
    pdf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    pub(super) fn input() -> InvoicePdfInput {
        InvoicePdfInput {
            document_type: DocumentType::Invoice,
            credits_number: None,
            seller_name: "Demo & Sønn AS".into(),
            seller_orgnumber: "999888777".into(),
            seller_address: Some("Storgata 1, 0155 Oslo".into()),
            seller_vat_registered: true,
            selger_foretaksregistrert: true,
            seller_account_number: Some("1234.56.78903".into()),
            buyer_name: "Kunde & Co AS".into(),
            buyer_number: "10000".into(),
            buyer_orgnumber: Some("911111111".into()),
            buyer_address: None,
            fakturanr: 7,
            invoice_date: NaiveDate::from_ymd_opt(2026, 7, 24).unwrap(),
            due_date: NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
            delivery_date: NaiveDate::from_ymd_opt(2026, 7, 20),
            leveringssted: None,
            kid: Some("000000071".into()),
            betalingsmiddel: None,
            linjer: vec![
                PdfLinje {
                    description: "Konsulentbistand".into(),
                    quantity_milli: 2500,
                    enhetspris_ore: 4_000_00,
                    vat_rate_bp: Some(2500),
                    net_ore: 10_000_00,
                    vat_ore: 2_500_00,
                },
                PdfLinje {
                    description: "Bøker".into(),
                    quantity_milli: 1000,
                    enhetspris_ore: 500_00,
                    vat_rate_bp: None,
                    net_ore: 500_00,
                    vat_ore: 0,
                },
            ],
            currency: None,
            motverdi_nok_ore: None,
        }
    }

    #[test]
    fn invoice_carries_the_lovpalagte_contents() {
        let bytes = render_invoice_pdf(&input());
        for expected in [
            &b"FAKTURA"[..],
            b"Org.nr 999 888 777 MVA",
            b"Foretaksregisteret",
            b"Kunde & Co AS",
            b"Org.nr 911 111 111",
            b"Kundenr 10000",
            b"Konsulentbistand",
            b"2,5",
            b"25 %",
            b"10 000,00",
            b"Mva 25 % av 10 000,00",
            b"2 500,00",
            b"13 000,00",
            b"KID: 000000071",
            b"Kontonummer: 1234.56.78903",
            b"Betales innen 2026-08-07",
            // §5-1-1 nr. 4: leveringstidspunktet, som ikke er det
            // samme som fakturadatoen (her levert 20., fakturert 24.).
            b"Levering",
            b"2026-07-20",
        ] {
            assert!(
                find(&bytes, expected).is_some(),
                "missing {:?}",
                String::from_utf8_lossy(expected)
            );
        }
        assert_eq!(bytes, render_invoice_pdf(&input()), "deterministic");
    }

    /// Leveringsstedet er fri tekst av ukjent lengde. Faktablokken gir
    /// verdien 70 pt før etiketten — nok til en dato, ikke til en
    /// adresse — så stedet skrives som ÉN linje. Testen fester at det
    /// er én sammenhengende tekststreng: to separate strenger ville
    /// vært den overlappende varianten som ble funnet i nettleseren.
    #[test]
    fn the_delivery_place_is_one_line_not_a_label_value_pair() {
        let mut i = input();
        i.leveringssted = Some("Lagerveien 3, 0666 Oslo".into());
        let bytes = render_invoice_pdf(&i);
        assert!(find(&bytes, b"Leveringssted: Lagerveien 3, 0666 Oslo").is_some());
        // Etiketten skal ikke også stå alene et sted — da ville den
        // ligget i faktablokken, der adressen kolliderer med den.
        assert!(find(&bytes, b"(Leveringssted) Tj").is_none());
    }

    #[test]
    fn kreditnota_flips_title_and_drops_payment_block() {
        let mut i = input();
        i.document_type = DocumentType::CreditNote;
        i.credits_number = Some(7);
        for linje in &mut i.linjer {
            linje.quantity_milli = -linje.quantity_milli;
            linje.net_ore = -linje.net_ore;
            linje.vat_ore = -linje.vat_ore;
        }
        let bytes = render_invoice_pdf(&i);
        assert!(find(&bytes, b"KREDITNOTA").is_some());
        assert!(find(&bytes, b"Krediterer faktura").is_some());
        assert!(
            find(&bytes, b"-13 000,00").is_some(),
            "signs shown honestly"
        );
        assert!(find(&bytes, b"BETALINGSINFORMASJON").is_none());
        assert!(find(&bytes, b"KID:").is_none());
    }

    #[test]
    fn long_invoices_paginate_with_repeated_header() {
        let mut i = input();
        let linje = i.linjer[0].clone();
        i.linjer = (0..60).map(|_| linje.clone()).collect();
        let bytes = render_invoice_pdf(&i);
        assert!(find(&bytes, b"/Count 2").is_some(), "two pages");
    }

    #[test]
    fn tekst_pdf_wraps_the_stored_document() {
        let text = "PURRING\n=======\n\nKID:            000000071\n";
        let bytes = render_tekst_pdf(text);
        assert!(find(&bytes, b"PURRING").is_some());
        assert!(find(&bytes, b"/BaseFont /Courier").is_some());
        assert_eq!(bytes, render_tekst_pdf(text), "deterministic");
        let many: String = (0..130).map(|i| format!("linje {i}\n")).collect();
        assert!(
            find(&render_tekst_pdf(&many), b"/Count 3").is_some(),
            "paginates"
        );
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(kr(123_456_789), "1 234 567,89");
        assert_eq!(kr(-50), "-0,50");
        assert_eq!(kr(100_00), "100,00");
        assert_eq!(quantity(2500), "2,5");
        assert_eq!(quantity(1000), "1");
        assert_eq!(quantity(333), "0,333");
        assert_eq!(prosent(2500), "25 %");
        assert_eq!(prosent(1150), "11,50 %");
    }
}

#[cfg(test)]
mod kontant_tests {
    use super::*;

    fn input(document_type: DocumentType) -> InvoicePdfInput {
        let mut i = super::tests::input();
        i.document_type = document_type;
        i.betalingsmiddel = Some("Kort".into());
        i
    }

    /// §5-3: a kontantfaktura documents a ytelse already paid. Telling
    /// the buyer where to transfer money — and by when — is not a
    /// cosmetic slip; it is a demand for a second payment.
    #[test]
    fn a_kontantfaktura_carries_no_kid_and_no_due_date() {
        let pdf = render_invoice_pdf(&input(DocumentType::Kontantfaktura));
        let text = String::from_utf8_lossy(&pdf).to_string();
        assert!(text.contains("KONTANTFAKTURA"), "tittelen");
        assert!(text.contains("Betalt"), "at den er gjort opp");
        assert!(text.contains("Kort"), "og med hva");
        for forbudt in ["KID", "BETALINGSINFORMASJON", "Betales innen", "Forfall"] {
            assert!(
                !text.contains(forbudt),
                "«{forbudt}» hører ikke hjemme på en kontantfaktura"
            );
        }
        // An ordinary faktura still carries all of it — the guard is the
        // dokumenttype, not a global removal.
        let vanlig =
            String::from_utf8_lossy(&render_invoice_pdf(&input(DocumentType::Invoice))).to_string();
        assert!(vanlig.contains("KID"));
        assert!(vanlig.contains("Betales innen"));
    }
}
