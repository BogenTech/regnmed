//! Faktura/kreditnota som PDF (docs/faktura.md) og purredokument som
//! PDF (docs/purring.md) — pure layout over [`crate::pdf`].
//!
//! Innholdet følger bokføringsforskriften §5-1-1: nummer og dato,
//! partene (selgers orgnr med "MVA"-suffiks når registrert,
//! "Foretaksregisteret" for foretak registrert der), ytelsens art og
//! omfang, vederlag med forfall, og merverdiavgift spesifisert i NOK
//! per sats. Rendering er deterministisk — PDF-en som lagres på bilaget
//! ved utstedelse kan reproduseres byte for byte.

use chrono::NaiveDate;

use crate::Ore;
use crate::pdf::{Font, PAGE_HEIGHT, Pdf};

const MARGIN: f32 = 50.0;
const RIGHT: f32 = 545.0;

#[derive(Debug, Clone)]
pub struct PdfLinje {
    pub beskrivelse: String,
    /// Tusendeler: 1 stk = 1000.
    pub antall_milli: i64,
    pub enhetspris_ore: i64,
    /// Basispunkter; None for linjer uten mva-kode.
    pub mva_sats_bp: Option<i64>,
    pub netto_ore: i64,
    pub mva_ore: i64,
}

/// The document kinds this layout renders. Faktura and Kreditnota are
/// regnskapsdokumenter (issued, stored, hash-verified); Tilbud and
/// Ordrebekreftelse are the commercial chain BEFORE the invoice (#31) —
/// no KID, no betalingsinformasjon, rendered on demand from editable
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dokumenttype {
    Faktura,
    Kreditnota,
    Tilbud,
    Ordrebekreftelse,
}

impl Dokumenttype {
    fn tittel(self) -> &'static str {
        match self {
            Dokumenttype::Faktura => "FAKTURA",
            Dokumenttype::Kreditnota => "KREDITNOTA",
            Dokumenttype::Tilbud => "TILBUD",
            Dokumenttype::Ordrebekreftelse => "ORDREBEKREFTELSE",
        }
    }

    fn nummerord(self) -> &'static str {
        match self {
            Dokumenttype::Faktura => "Fakturanr",
            Dokumenttype::Kreditnota => "Kreditnotanr",
            Dokumenttype::Tilbud => "Tilbudsnr",
            Dokumenttype::Ordrebekreftelse => "Ordrenr",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FakturaPdfInput {
    pub dokumenttype: Dokumenttype,
    /// Fakturanummeret til fakturaen denne krediterer.
    pub krediterer_nr: Option<i64>,

    pub selger_navn: String,
    pub selger_orgnr: String,
    pub selger_adresse: Option<String>,
    pub selger_mva_registrert: bool,
    /// "Foretaksregisteret" påføres for foretak registrert der (AS/ASA,
    /// foretaksregisterloven §10-2).
    pub selger_foretaksregistrert: bool,
    pub selger_kontonummer: Option<String>,

    pub kjoper_navn: String,
    pub kjoper_nr: String,
    pub kjoper_orgnr: Option<String>,
    pub kjoper_adresse: Option<String>,

    pub fakturanr: i64,
    pub fakturadato: NaiveDate,
    pub forfallsdato: NaiveDate,
    pub kid: String,

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

fn antall(milli: i64) -> String {
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
fn orgnr_display(orgnr: &str) -> String {
    if orgnr.len() == 9 {
        format!("{} {} {}", &orgnr[..3], &orgnr[3..6], &orgnr[6..])
    } else {
        orgnr.to_string()
    }
}

pub fn render_faktura_pdf(input: &FakturaPdfInput) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let tittel = input.dokumenttype.tittel();
    let kreditnota = input.dokumenttype == Dokumenttype::Kreditnota;

    // Seller block, top left.
    pdf.text(MARGIN, 60.0, 13.0, Font::Bold, &input.selger_navn);
    let mut y = 76.0;
    if let Some(adresse) = &input.selger_adresse {
        pdf.text(MARGIN, y, 9.0, Font::Regular, adresse);
        y += 12.0;
    }
    let mut orglinje = format!("Org.nr {}", orgnr_display(&input.selger_orgnr));
    if input.selger_mva_registrert {
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
            input.dokumenttype.nummerord().into(),
            input.fakturanr.to_string(),
        ),
        ("Dato".into(), input.fakturadato.to_string()),
    ];
    if input.dokumenttype == Dokumenttype::Faktura {
        fakta.push(("Forfall".into(), input.forfallsdato.to_string()));
    }
    if let Some(nr) = input.krediterer_nr {
        fakta.push(("Krediterer faktura".into(), nr.to_string()));
    }
    let mut fy = 82.0;
    for (label, value) in &fakta {
        pdf.text_right(RIGHT - 70.0, fy, 9.0, Font::Regular, label);
        pdf.text_right(RIGHT, fy, 9.0, Font::Bold, value);
        fy += 12.0;
    }

    // Buyer block.
    pdf.text(MARGIN, 130.0, 8.0, Font::Bold, "FAKTURAMOTTAKER");
    pdf.text(MARGIN, 144.0, 11.0, Font::Regular, &input.kjoper_navn);
    let mut ky = 158.0;
    if let Some(adresse) = &input.kjoper_adresse {
        pdf.text(MARGIN, ky, 9.0, Font::Regular, adresse);
        ky += 12.0;
    }
    if let Some(orgnr) = &input.kjoper_orgnr {
        pdf.text(
            MARGIN,
            ky,
            9.0,
            Font::Regular,
            &format!("Org.nr {}", orgnr_display(orgnr)),
        );
        ky += 12.0;
    }
    pdf.text(
        MARGIN,
        ky,
        9.0,
        Font::Regular,
        &format!("Kundenr {}", input.kjoper_nr),
    );

    // Line table. Columns: description | antall | pris | mva | beløp.
    let col_antall = 330.0;
    let col_pris = 405.0;
    let col_mva = 460.0;
    let table_top = 210.0;
    let mut ly = table_top;
    let header = |pdf: &mut Pdf, y: f32| {
        pdf.text(MARGIN, y, 8.0, Font::Bold, "Beskrivelse");
        pdf.text_right(col_antall, y, 8.0, Font::Bold, "Antall");
        pdf.text_right(col_pris, y, 8.0, Font::Bold, "Pris");
        pdf.text_right(col_mva, y, 8.0, Font::Bold, "Mva");
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
        pdf.text(MARGIN, ly, 9.0, Font::Regular, &linje.beskrivelse);
        pdf.text_right(
            col_antall,
            ly,
            9.0,
            Font::Regular,
            &antall(linje.antall_milli),
        );
        pdf.text_right(col_pris, ly, 9.0, Font::Regular, &kr(linje.enhetspris_ore));
        if let Some(bp) = linje.mva_sats_bp {
            pdf.text_right(col_mva, ly, 9.0, Font::Regular, &prosent(bp));
        }
        pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(linje.netto_ore));
        ly += 15.0;
    }
    pdf.rule(MARGIN, RIGHT, ly - 5.0, 0.7);
    ly += 8.0;

    // Totals + mva spesifisert i NOK per sats (§5-1-1 nr. 6).
    let netto: i64 = input.linjer.iter().map(|l| l.netto_ore).sum();
    let mva: i64 = input.linjer.iter().map(|l| l.mva_ore).sum();
    let brutto = netto + mva;
    pdf.text_right(RIGHT - 110.0, ly, 9.0, Font::Regular, "Netto");
    pdf.text_right(RIGHT, ly, 9.0, Font::Regular, &kr(netto));
    ly += 14.0;
    let mut satser: Vec<i64> = input.linjer.iter().filter_map(|l| l.mva_sats_bp).collect();
    satser.sort_unstable();
    satser.dedup();
    for sats in satser {
        let grunnlag: i64 = input
            .linjer
            .iter()
            .filter(|l| l.mva_sats_bp == Some(sats))
            .map(|l| l.netto_ore)
            .sum();
        let avgift: i64 = input
            .linjer
            .iter()
            .filter(|l| l.mva_sats_bp == Some(sats))
            .map(|l| l.mva_ore)
            .sum();
        pdf.text_right(
            RIGHT - 110.0,
            ly,
            9.0,
            Font::Regular,
            &format!("Mva {} av {}", prosent(sats), kr(grunnlag)),
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
    pdf.text_right(RIGHT, ly, 11.0, Font::Bold, &kr(brutto));

    // Payment block at a fixed distance below the totals.
    // Betalingsinformasjon only on an actual faktura — tilbud/ordre are
    // not payable and kreditnotaer are settlements.
    if input.dokumenttype == Dokumenttype::Faktura {
        ly += 30.0;
        pdf.rule(MARGIN, RIGHT, ly, 0.7);
        ly += 16.0;
        pdf.text(MARGIN, ly, 8.0, Font::Bold, "BETALINGSINFORMASJON");
        ly += 14.0;
        if let Some(konto) = &input.selger_kontonummer {
            pdf.text(
                MARGIN,
                ly,
                9.0,
                Font::Regular,
                &format!("Kontonummer: {konto}"),
            );
            ly += 13.0;
        }
        pdf.text(
            MARGIN,
            ly,
            9.0,
            Font::Regular,
            &format!("KID: {}", input.kid),
        );
        ly += 13.0;
        pdf.text(
            MARGIN,
            ly,
            9.0,
            Font::Regular,
            &format!("Betales innen {} — {}", input.forfallsdato, kr(brutto)),
        );
    }
    pdf.finish()
}

/// Et lagret klartekstdokument (purring/inkassovarsel) som PDF: Courier
/// linje for linje, så kolonnene i tekstdokumentet beholder justeringen.
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

    fn input() -> FakturaPdfInput {
        FakturaPdfInput {
            dokumenttype: Dokumenttype::Faktura,
            krediterer_nr: None,
            selger_navn: "Demo & Sønn AS".into(),
            selger_orgnr: "999888777".into(),
            selger_adresse: Some("Storgata 1, 0155 Oslo".into()),
            selger_mva_registrert: true,
            selger_foretaksregistrert: true,
            selger_kontonummer: Some("1234.56.78903".into()),
            kjoper_navn: "Kunde & Co AS".into(),
            kjoper_nr: "10000".into(),
            kjoper_orgnr: Some("911111111".into()),
            kjoper_adresse: None,
            fakturanr: 7,
            fakturadato: NaiveDate::from_ymd_opt(2026, 7, 24).unwrap(),
            forfallsdato: NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
            kid: "000000071".into(),
            linjer: vec![
                PdfLinje {
                    beskrivelse: "Konsulentbistand".into(),
                    antall_milli: 2500,
                    enhetspris_ore: 4_000_00,
                    mva_sats_bp: Some(2500),
                    netto_ore: 10_000_00,
                    mva_ore: 2_500_00,
                },
                PdfLinje {
                    beskrivelse: "Bøker".into(),
                    antall_milli: 1000,
                    enhetspris_ore: 500_00,
                    mva_sats_bp: None,
                    netto_ore: 500_00,
                    mva_ore: 0,
                },
            ],
        }
    }

    #[test]
    fn faktura_carries_the_lovpalagte_contents() {
        let bytes = render_faktura_pdf(&input());
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
        ] {
            assert!(
                find(&bytes, expected).is_some(),
                "missing {:?}",
                String::from_utf8_lossy(expected)
            );
        }
        assert_eq!(bytes, render_faktura_pdf(&input()), "deterministic");
    }

    #[test]
    fn kreditnota_flips_title_and_drops_payment_block() {
        let mut i = input();
        i.dokumenttype = Dokumenttype::Kreditnota;
        i.krediterer_nr = Some(7);
        for linje in &mut i.linjer {
            linje.antall_milli = -linje.antall_milli;
            linje.netto_ore = -linje.netto_ore;
            linje.mva_ore = -linje.mva_ore;
        }
        let bytes = render_faktura_pdf(&i);
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
        let bytes = render_faktura_pdf(&i);
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
        assert_eq!(antall(2500), "2,5");
        assert_eq!(antall(1000), "1");
        assert_eq!(antall(333), "0,333");
        assert_eq!(prosent(2500), "25 %");
        assert_eq!(prosent(1150), "11,50 %");
    }
}
