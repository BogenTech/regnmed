//! Document interpretation: suggestions from the document's own text
//! (docs/bilagstolkning.md, #34).
//!
//! The goal is to save keystrokes, not to post. Everything here is a
//! **suggestion**, and each one carries its own justification ("found
//! after «Å betale»") — the regnskapsfører should be able to see why the
//! machine believes something, and overrule it without hunting.
//!
//! The trick that makes the heuristic tolerably precise without models:
//! **the check digits we already trust**. A nine-digit number that passes
//! orgnr MOD11 is almost certainly an organisasjonsnummer; a number that
//! passes KID MOD10/MOD11 next to the word «KID» is a KID; eleven digits
//! that pass account-number MOD11 are a kontonummer. Random numbers do
//! not do that.
//!
//! No cloud service, no model in the core — OCR for scanned images
//! belongs in an optional sidecar (docs/frugality.md), and the API
//! behaves identically without it: the suggestions are simply absent.

use chrono::NaiveDate;

/// One suggested value with the reason we believe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding<T> {
    pub value: T,
    /// Human-readable provenance, shown in the UI next to the value.
    pub reason: String,
}

impl<T> Finding<T> {
    fn new(value: T, reason: impl Into<String>) -> Self {
        Self {
            value,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Hints {
    pub orgnumber: Option<Finding<String>>,
    pub invoice_number: Option<Finding<String>>,
    pub kid: Option<Finding<String>>,
    pub account_number: Option<Finding<String>>,
    pub date: Option<Finding<NaiveDate>>,
    pub due_date: Option<Finding<NaiveDate>>,
    /// Gross amount to pay.
    pub amount_ore: Option<Finding<i64>>,
    pub vat_ore: Option<Finding<i64>>,
    /// More than one plausible VAT line with different amounts: likely a
    /// VAT line per rate. `vat_ore` holds one of them, so the reader must
    /// be told the invoice needs splitting by rate.
    pub several_vat_lines: bool,
    /// The first line that reads like a name -- on a till receipt, the
    /// shop. Weak by nature; a caller prefers the registry name for the
    /// orgnr when there is one.
    pub seller: Option<Finding<String>>,
}

fn digits_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Every run of digits, spaces and dots — the shapes Norwegian numbers
/// are written in ("915 933 149", "8601.11.17947", "1 234,50").
fn number_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in line.chars() {
        if c.is_ascii_digit() || (!current.is_empty() && (c == ' ' || c == '.' || c == ',')) {
            current.push(c);
        } else {
            if current.chars().any(|c| c.is_ascii_digit()) {
                tokens.push(current.trim_end_matches([' ', '.', ',']).to_string());
            }
            current = String::new();
        }
    }
    if current.chars().any(|c| c.is_ascii_digit()) {
        tokens.push(current.trim_end_matches([' ', '.', ',']).to_string());
    }
    tokens
}

/// "1 234,50" / "1.234,50" / "1234.50" / "1234" → øre. Refuses anything
/// with more than two decimals — an invoice line is not a rate.
fn parse_amount(token: &str) -> Option<i64> {
    let cleaned: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    // The last separator decides; thousands separators come in pairs of
    // three digits behind them.
    let (whole, frac) = match cleaned.rfind([',', '.']) {
        Some(pos) => {
            let (w, f) = cleaned.split_at(pos);
            let f = &f[1..];
            if f.len() == 3 && cleaned.matches([',', '.']).count() >= 1 && !cleaned.contains(',') {
                // "1.234" — a thousands separator, not decimals.
                (cleaned.replace('.', ""), String::new())
            } else if f.len() > 2 {
                return None;
            } else {
                (w.replace(['.', ' '], "").replace(',', ""), f.to_string())
            }
        }
        None => (cleaned.clone(), String::new()),
    };
    let whole: i64 = whole.parse().ok()?;
    let frac: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<2}").parse().ok()?
    };
    Some(whole * 100 + frac)
}

fn parse_date(token: &str) -> Option<NaiveDate> {
    let t = token.trim();
    for format in ["%d.%m.%Y", "%Y-%m-%d", "%d.%m.%y", "%d/%m/%Y", "%d.%m.%Y."] {
        if let Ok(date) = NaiveDate::parse_from_str(t, format) {
            return Some(date);
        }
    }
    None
}

/// Dates written with dots are cut out of the surrounding text.
fn date_candidates(line: &str) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || "./-".contains(chars[i])) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if let Some(date) = parse_date(token.trim_end_matches(['.', '/', '-'])) {
                out.push(date);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn normalized(line: &str) -> String {
    line.to_lowercase()
        .replace('ø', "o")
        .replace('æ', "ae")
        .replace('å', "a")
}

/// Reads a document's text and proposes what it can defend.
pub fn read(text: &str) -> Hints {
    let mut hints = Hints::default();
    let lines: Vec<&str> = text.lines().collect();
    // Every line that looks like VAT, in reading order; the choice is
    // made at the end, against the total (see below).
    let mut vat_candidates: Vec<i64> = Vec::new();
    // A till receipt says «TOTALT» or «Bankkort», never «å betale»: these
    // are weaker than the invoice keywords and only fill an empty slot.
    let mut receipt_total: Option<Finding<i64>> = None;
    let mut first_date: Option<NaiveDate> = None;

    for (i, line) in lines.iter().enumerate() {
        let lower = normalized(line);
        // Labels sometimes sit above their value instead of beside it,
        // so an empty line falls through to the next one — but a line
        // that HAS numbers keeps to its own: reading across would let
        // the total below «mva» masquerade as the tax.
        let next_line = lines.get(i + 1).copied().unwrap_or("");
        let own_numbers = number_tokens(line);
        let tokens: Vec<String> = if own_numbers.is_empty() {
            number_tokens(next_line)
        } else {
            own_numbers.clone()
        };

        // --- Identifiers, decided by check digit ---
        for token in &tokens {
            let digits = digits_only(token);
            if hints.orgnumber.is_none()
                && digits.len() == 9
                && crate::orgnumber::is_valid(&digits)
                && (lower.contains("org")
                    || lower.contains("foretaksregister")
                    || lower.contains("mva"))
            {
                hints.orgnumber = Some(Finding::new(
                    digits.clone(),
                    format!(
                        "ni siffer med gyldig kontrollsiffer nær «{}»",
                        keyword(&lower)
                    ),
                ));
            }
            if hints.kid.is_none() && lower.contains("kid") && crate::kid::is_valid(&digits) {
                hints.kid = Some(Finding::new(digits.clone(), "gyldig KID nær «KID»"));
            }
            if hints.account_number.is_none()
                && digits.len() == 11
                && crate::pain001::is_valid_account_number(&digits)
                && (lower.contains("konto") || lower.contains("bank"))
            {
                hints.account_number = Some(Finding::new(
                    digits.clone(),
                    "elleve siffer med gyldig kontrollsiffer nær «konto»",
                ));
            }
        }

        // --- Invoice number ---
        if hints.invoice_number.is_none()
            && (lower.contains("fakturanr")
                || lower.contains("faktura nr")
                || lower.contains("fakturanummer"))
            && let Some(token) = tokens.iter().find(|t| {
                let d = digits_only(t);
                (1..=20).contains(&d.len()) && !d.is_empty()
            })
        {
            hints.invoice_number = Some(Finding::new(digits_only(token), "etter «fakturanr»"));
        }

        // --- Dates ---
        let dates = {
            let d = date_candidates(line);
            if d.is_empty() {
                date_candidates(next_line)
            } else {
                d
            }
        };
        if hints.due_date.is_none()
            && (lower.contains("forfall") || lower.contains("betalingsfrist"))
            && let Some(date) = dates.first()
        {
            hints.due_date = Some(Finding::new(*date, "etter «forfall»"));
        }
        if hints.date.is_none()
            && (lower.contains("fakturadato") || lower.contains("bilagsdato"))
            && let Some(date) = dates.first()
        {
            hints.date = Some(Finding::new(*date, "etter «fakturadato»"));
        }

        if first_date.is_none() && !lower.contains("forfall") && !lower.contains("betalingsfrist") {
            first_date = date_candidates(line).first().copied();
        }
        if hints.seller.is_none()
            && line.chars().filter(|c| c.is_alphabetic()).count() >= 3
            && !["faktura", "kvittering", "salgskvittering"].contains(&lower.trim())
        {
            hints.seller = Some(Finding::new(line.trim().to_string(), "øverste linje"));
        }

        // --- Amounts ---
        // «Å betale» is the one number on an invoice that means exactly
        // one thing, so it beats «sum» and «total».
        let amount_keyword: &[(&str, &str)] = &[
            ("a betale", "«å betale»"),
            ("belop a betale", "«beløp å betale»"),
            ("total inkl", "«total inkl. mva»"),
            ("totalt inkl", "«totalt inkl. mva»"),
            ("sum inkl", "«sum inkl. mva»"),
            ("totalsum", "«totalsum»"),
        ];
        for (key, vist) in amount_keyword {
            if lower.contains(key)
                && let Some(amount) = tokens.iter().filter_map(|t| parse_amount(t)).next_back()
                && amount > 0
            {
                let stronger = hints
                    .amount_ore
                    .as_ref()
                    .is_none_or(|f| !f.reason.contains("å betale"));
                if stronger {
                    hints.amount_ore = Some(Finding::new(amount, format!("etter {vist}")));
                }
                break;
            }
        }
        if receipt_total.is_none()
            && !lower.contains("mva")
            && let Some((_, vist)) = RECEIPT_TOTALS
                .iter()
                .find(|(key, _)| lower.trim_start().starts_with(key))
            // With decimals only: «Totalt antall varer 5» is not 5 kroner.
            && let Some(amount) = own_numbers
                .iter()
                .filter(|t| t.contains([',', '.']))
                .filter_map(|t| parse_amount(t))
                .next_back()
            && amount > 0
        {
            receipt_total = Some(Finding::new(amount, format!("etter {vist}")));
        }
        // «Org.nr 974 760 673 MVA» is the Foretaksregister marking, not
        // a tax line: any spelling of «org» or a valid orgnr among the
        // numbers rules the line out.
        let carries_orgnumber = tokens.iter().any(|t| {
            let d = digits_only(t);
            d.len() == 9 && crate::orgnumber::is_valid(&d)
        });
        if (lower.contains("mva") || lower.contains("merverdiavgift"))
            && !lower.contains("org")
            && !carries_orgnumber
            && !lower.contains("inkl")
            && let Some(amount) = tokens.iter().filter_map(|t| parse_amount(t)).next_back()
            && amount > 0
        {
            vat_candidates.push(amount);
        }
    }
    if hints.amount_ore.is_none() {
        hints.amount_ore = receipt_total;
    }
    if hints.date.is_none() {
        hints.date = first_date.map(|d| Finding::new(d, "første dato i dokumentet"));
    }
    // VAT is at most a fifth of a gross amount (25 % on the net is 20 % of
    // the gross), so a candidate above that was read off the wrong line --
    // typically a line-item table whose header has a «Mva» column. The
    // summary comes after the items, so the LAST plausible candidate wins.
    // Without a total there is nothing to check against: the last one.
    let plausible = |vat: i64| {
        hints
            .amount_ore
            .as_ref()
            .is_none_or(|total| vat * 5 <= total.value + 5)
    };
    let mut fitting: Vec<i64> = vat_candidates
        .into_iter()
        .filter(|&v| plausible(v))
        .collect();
    hints.vat_ore = fitting.last().map(|&vat| Finding::new(vat, "etter «mva»"));
    fitting.sort_unstable();
    fitting.dedup();
    hints.several_vat_lines = fitting.len() > 1;
    hints
}

/// The line-openers that carry a till receipt's total, best first.
const RECEIPT_TOTALS: &[(&str, &str)] = &[
    ("totalt", "«totalt»"),
    ("total", "«total»"),
    ("til betaling", "«til betaling»"),
    ("sum", "«sum»"),
    ("bankkort", "«bankkort»"),
    ("betalt", "«betalt»"),
];

/// The keyword we matched, for the provenance string.
fn keyword(lower: &str) -> &'static str {
    if lower.contains("orgnr") || lower.contains("organisasjonsnummer") {
        "orgnr"
    } else if lower.contains("foretaksregister") {
        "Foretaksregisteret"
    } else {
        "mva"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVOICE: &str = "\
Handelshuset AS
Orgnr 974760673 MVA
Storgata 1, 0155 Oslo

FAKTURA
Fakturanr: 90210
Fakturadato: 30.06.2026
Forfallsdato: 30.07.2026

Kontorstoler 10 stk            2 000,00
Frakt                            250,00
Nettosum                       2 250,00
MVA 25 %                         562,50
Å betale                       2 812,50

Kontonummer: 8601.11.17947
KID: 1234567897
";

    #[test]
    fn reads_an_ordinary_norwegian_invoice() {
        let f = read(INVOICE);
        assert_eq!(f.orgnumber.as_ref().unwrap().value, "974760673");
        assert_eq!(f.invoice_number.as_ref().unwrap().value, "90210");
        assert_eq!(
            f.date.as_ref().unwrap().value,
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap()
        );
        assert_eq!(
            f.due_date.as_ref().unwrap().value,
            NaiveDate::from_ymd_opt(2026, 7, 30).unwrap()
        );
        assert_eq!(f.amount_ore.as_ref().unwrap().value, 2_812_50);
        assert_eq!(f.vat_ore.as_ref().unwrap().value, 562_50);
        assert_eq!(f.account_number.as_ref().unwrap().value, "86011117947");
        assert_eq!(f.kid.as_ref().unwrap().value, "1234567897");
    }

    /// regnmed's own invoice PDF writes «Org.nr 790 292 642 MVA»: the
    /// orgnr, spaced and dotted, was once read as 790 292 642 kr of VAT.
    #[test]
    fn the_foretaksregister_marking_is_not_a_vat_amount() {
        let text = "Purredemo AS\nOrg.nr 790 292 642 MVA\nFaktura 2\nSats 25 %  Mva 250,00\nÅ betale 1 250,00\n";
        let f = read(text);
        assert_eq!(f.amount_ore.as_ref().unwrap().value, 1_250_00);
        assert_eq!(f.vat_ore.as_ref().unwrap().value, 250_00);
        // A VAT figure larger than a fifth of the total is dropped.
        let f = read("Mva 2 000,00\nÅ betale 1 250,00\n");
        assert!(f.vat_ore.is_none());
    }

    /// A line-item table with a «Mva» column header above the first item:
    /// the header falls through to the item's amount, which is no VAT.
    /// The summary's VAT line, later and within a fifth, wins.
    #[test]
    fn several_vat_lines_are_flagged() {
        let text = "Mva 15 % 120,00\nMva 25 % 50,00\nÅ betale 1 090,00\n";
        let f = read(text);
        assert!(f.several_vat_lines);
        assert!(!read(INVOICE).several_vat_lines);
    }

    #[test]
    fn a_vat_column_header_does_not_shadow_the_summary() {
        let text = "Beskrivelse   Antall   Pris   Mva   Beløp\n                    Konsulentbistand  1  1 000,00  25 %  1 000,00\n                    Netto 1 000,00\nMva 25 % 250,00\nÅ betale 1 250,00\n";
        let f = read(text);
        assert_eq!(f.vat_ore.as_ref().unwrap().value, 250_00);
    }

    #[test]
    fn every_finding_says_why() {
        let f = read(INVOICE);
        assert!(
            f.amount_ore.as_ref().unwrap().reason.contains("å betale"),
            "{:?}",
            f.amount_ore
        );
        assert!(f.kid.as_ref().unwrap().reason.contains("KID"));
        assert!(
            f.orgnumber
                .as_ref()
                .unwrap()
                .reason
                .contains("kontrollsiffer")
        );
    }

    #[test]
    fn numbers_without_a_valid_check_digit_are_not_suggested() {
        let text = "Orgnr 123456789\nKID: 1111111111\nKontonummer: 12345678901\nBeskrivelse";
        let f = read(text);
        assert!(
            f.orgnumber.is_none(),
            "an invalid orgnr must not be suggested"
        );
        assert!(f.kid.is_none());
        assert!(f.account_number.is_none());
    }

    #[test]
    fn a_value_on_the_line_below_its_label_is_found_too() {
        let text = "Fakturanr\n55512\nForfallsdato\n01.09.2026\nÅ betale\n1 000,00";
        let f = read(text);
        assert_eq!(f.invoice_number.unwrap().value, "55512");
        assert_eq!(
            f.due_date.unwrap().value,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        );
        assert_eq!(f.amount_ore.unwrap().value, 1_000_00);
    }

    #[test]
    fn a_betale_beats_the_other_totals() {
        let text = "Sum inkl mva 9 999,00\nÅ betale 1 234,50\nTotalsum 8 888,00";
        let f = read(text).amount_ore.unwrap();
        assert_eq!(f.value, 1_234_50);
        assert!(f.reason.contains("å betale"));
    }

    #[test]
    fn amount_formats() {
        assert_eq!(parse_amount("1 234,50"), Some(123_450));
        assert_eq!(parse_amount("1.234,50"), Some(123_450));
        assert_eq!(parse_amount("1234.50"), Some(123_450));
        assert_eq!(parse_amount("1234"), Some(123_400));
        assert_eq!(
            parse_amount("1.234"),
            Some(123_400),
            "thousands separator, not øre"
        );
        assert_eq!(
            parse_amount("12,345"),
            None,
            "tre desimaler er ikke et beløp"
        );
    }

    /// What regnmed-ocr returned for crates/regnmed-ocr/tests/receipt.jpg,
    /// noise and all: ASCII only, a merged row, «Ora.nr».
    const OCR_RECEIPT: &str = "\
REMA 1000 GRONLAND
Rema Gr?nland AS
Ora.nr: 915 933 149 MVA
Gr?nlandsleiret 25, 0190 Oslo
Kvittering 12.11.2026 14:32
Kasse 3 Bong 4471
Melk 9Port1,751  Bred  2.40  90
Kaffe filtermalt  89.90
Barepose  4.50
Pant  3.00
TOTALT  157,70
Bankkort  157,70
MVA-grunnlag Sats MVA
125.92  15%% 18.89
2,49  25%  0.60
3.00  0%  0.00
Sum MVA  19.49
Takk for handelen!";

    #[test]
    fn reads_a_till_receipt_as_the_ocr_returns_it() {
        let h = read(OCR_RECEIPT);
        let amount = h.amount_ore.unwrap();
        assert_eq!(
            (amount.value, amount.reason.as_str()),
            (15770, "etter «totalt»")
        );
        let date = h.date.unwrap();
        assert_eq!(date.value, NaiveDate::from_ymd_opt(2026, 11, 12).unwrap());
        assert_eq!(date.reason, "første dato i dokumentet");
        assert_eq!(h.orgnumber.unwrap().value, "915933149");
        assert_eq!(h.seller.unwrap().value, "REMA 1000 GRONLAND");
        assert_eq!(h.vat_ore.unwrap().value, 1949);
        assert!(h.several_vat_lines);
    }

    #[test]
    fn a_receipt_total_never_beats_an_invoice_keyword() {
        let h = read("Totalt 100,00\nÅ betale 125,00\nFakturadato: 01.02.2026\n03.02.2026");
        assert_eq!(h.amount_ore.unwrap().value, 12500);
        // A labelled date beats the first date in the text.
        assert_eq!(h.date.unwrap().reason, "etter «fakturadato»");
        // A count is not an amount.
        assert!(read("Totalt antall varer 5").amount_ore.is_none());
    }

    #[test]
    fn empty_text_yields_an_empty_suggestion() {
        let f = read("");
        assert!(f.orgnumber.is_none() && f.amount_ore.is_none() && f.date.is_none());
    }
}
