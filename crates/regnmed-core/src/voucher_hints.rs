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
            own_numbers
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

        // --- Amounts ---
        // «Å betale» is the one number on an invoice that means exactly
        // one thing, so it beats «sum» and «total».
        let belop_stikkord: &[(&str, &str)] = &[
            ("a betale", "«å betale»"),
            ("belop a betale", "«beløp å betale»"),
            ("total inkl", "«total inkl. mva»"),
            ("totalt inkl", "«totalt inkl. mva»"),
            ("sum inkl", "«sum inkl. mva»"),
            ("totalsum", "«totalsum»"),
        ];
        for (nokkel, vist) in belop_stikkord {
            if lower.contains(nokkel)
                && let Some(belop) = tokens.iter().filter_map(|t| parse_amount(t)).next_back()
                && belop > 0
            {
                let stronger = hints
                    .amount_ore
                    .as_ref()
                    .is_none_or(|f| !f.reason.contains("å betale"));
                if stronger {
                    hints.amount_ore = Some(Finding::new(belop, format!("etter {vist}")));
                }
                break;
            }
        }
        if hints.vat_ore.is_none()
            && (lower.contains("mva") || lower.contains("merverdiavgift"))
            && !lower.contains("orgnr")
            && !lower.contains("inkl")
            && let Some(belop) = tokens.iter().filter_map(|t| parse_amount(t)).next_back()
            && belop > 0
        {
            hints.vat_ore = Some(Finding::new(belop, "etter «mva»"));
        }
    }
    hints
}

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

    const FAKTURA: &str = "\
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
    fn reads_an_ordinary_norwegian_faktura() {
        let f = read(FAKTURA);
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

    #[test]
    fn every_finding_says_why() {
        let f = read(FAKTURA);
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

    #[test]
    fn empty_text_yields_an_empty_suggestion() {
        let f = read("");
        assert!(f.orgnumber.is_none() && f.amount_ore.is_none() && f.date.is_none());
    }
}
