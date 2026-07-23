//! Bank CSV import — connectivity tier 1b (docs/bank.md).
//!
//! Every Norwegian nettbank can export transactions as CSV, but the
//! formats differ per bank (delimiter, column names, one signed beløp
//! column vs separate inn/ut columns, date and number formats). This
//! parser detects the layout from the header row instead of maintaining
//! per-bank profiles: header vocabularies drift less than bank product
//! names do, and a file we cannot understand fails loudly with the
//! headers we saw — never a silent half-import.
//!
//! The output is a [`Camt053Statement`], so storage, matching and
//! reconciliation are exactly the same engine as the camt.053 tier.
//! CSVs carry no statement id or balances: the statement ref is derived
//! from the file's content hash (re-import of the same file stays
//! idempotent), and balances are `None` — the reconciliation view
//! already treats them as optional.

use chrono::NaiveDate;

use crate::camt053::{Camt053Statement, Camt053Transaction};
use crate::hash::sha256;

#[derive(Debug, PartialEq, Eq)]
pub enum BankCsvError {
    Empty,
    /// No recognizable date/amount columns; carries the headers found.
    UnknownLayout(String),
    BadDate(String),
    BadAmount(String),
}

impl std::fmt::Display for BankCsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "filen er tom"),
            Self::UnknownLayout(headers) => write!(
                f,
                "fant ikke dato- og beløpskolonner i CSV-en (kolonner: {headers}) — \
                 eksporter transaksjonslisten fra nettbanken med kolonneoverskrifter"
            ),
            Self::BadDate(value) => write!(f, "ugyldig dato '{value}'"),
            Self::BadAmount(value) => write!(f, "ugyldig beløp '{value}'"),
        }
    }
}

impl std::error::Error for BankCsvError {}

/// Splits one CSV record, honoring double quotes (`"a;b"` is one field,
/// `""` inside quotes is an escaped quote).
fn split_record(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == delimiter {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

fn detect_delimiter(header: &str) -> char {
    for candidate in [';', '\t', ','] {
        if header.contains(candidate) {
            return candidate;
        }
    }
    ';'
}

fn norm(header: &str) -> String {
    header
        .to_lowercase()
        .replace('ø', "o")
        .replace('æ', "ae")
        .replace('å', "a")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

fn find_column(headers: &[String], candidates: &[&str], avoid: &[&str]) -> Option<usize> {
    // Exact name first, then containment — and never an avoided word
    // (e.g. "rentedato" must not win the date column).
    for stage in 0..2 {
        for (i, header) in headers.iter().enumerate() {
            if avoid.iter().any(|a| header.contains(a)) {
                continue;
            }
            let hit = candidates.iter().any(|c| {
                if stage == 0 {
                    header == c
                } else {
                    header.contains(c)
                }
            });
            if hit {
                return Some(i);
            }
        }
    }
    None
}

/// "1 234,56" / "1.234,56" / "-450,00" / "1234.56" → øre. If a comma is
/// present it is the decimal separator (Norwegian exports); otherwise a
/// dot is.
fn parse_amount(raw: &str) -> Result<i64, BankCsvError> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}')
        .collect();
    if cleaned.is_empty() {
        return Ok(0);
    }
    let bad = || BankCsvError::BadAmount(raw.to_string());
    let normalized = if cleaned.contains(',') {
        cleaned.replace('.', "").replace(',', ".")
    } else {
        cleaned
    };
    let (whole, frac) = match normalized.split_once('.') {
        Some((w, f)) => (w, f),
        None => (normalized.as_str(), ""),
    };
    if frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let negative = whole.starts_with('-');
    let whole_digits = whole.trim_start_matches(['-', '+']);
    if whole_digits.is_empty() || !whole_digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let whole: i64 = whole_digits.parse().map_err(|_| bad())?;
    let frac: i64 = format!("{frac:0<2}").parse().map_err(|_| bad())?;
    let ore = whole * 100 + frac;
    Ok(if negative { -ore } else { ore })
}

fn parse_date(raw: &str) -> Result<NaiveDate, BankCsvError> {
    let raw = raw.trim();
    for format in ["%d.%m.%Y", "%Y-%m-%d", "%d/%m/%Y", "%d.%m.%y"] {
        if let Ok(date) = NaiveDate::parse_from_str(raw, format) {
            return Ok(date);
        }
    }
    Err(BankCsvError::BadDate(raw.to_string()))
}

pub fn parse(text: &str) -> Result<Camt053Statement, BankCsvError> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or(BankCsvError::Empty)?;
    let delimiter = detect_delimiter(header_line);
    let headers: Vec<String> = split_record(header_line, delimiter)
        .iter()
        .map(|h| norm(h))
        .collect();

    let date_col = find_column(
        &headers,
        &[
            "bokforingsdato",
            "bokfort dato",
            "bokforingsdag",
            "utfort dato",
            "dato",
        ],
        &["rentedato", "valuteringsdato"],
    );
    let amount_col = find_column(&headers, &["belop"], &["valuta"]);
    let in_col = find_column(
        &headers,
        &["inn pa konto", "inn"],
        &["innskudd", "konto til"],
    );
    let out_col = find_column(
        &headers,
        &["ut fra konto", "ut"],
        &["utskudd", "konto fra", "utfort"],
    );
    let text_col = find_column(
        &headers,
        &[
            "beskrivelse",
            "forklaring",
            "tekst",
            "tittel",
            "melding",
            "type",
        ],
        &[],
    );
    let reference_col = find_column(&headers, &["kid", "referanse", "arkivref"], &[]);

    let has_amount = amount_col.is_some() || (in_col.is_some() && out_col.is_some());
    if date_col.is_none() || !has_amount {
        return Err(BankCsvError::UnknownLayout(headers.join(", ")));
    }
    let date_col = date_col.expect("checked above");

    let mut transactions = Vec::new();
    for line in lines {
        let fields = split_record(line, delimiter);
        let get = |i: Option<usize>| {
            i.and_then(|i| fields.get(i))
                .map(|s| s.trim())
                .unwrap_or("")
        };
        let date_raw = get(Some(date_col));
        if date_raw.is_empty() {
            // Reserved/not-yet-booked rows come without a booking date.
            continue;
        }
        let booking_date = parse_date(date_raw)?;
        let amount_ore = match amount_col {
            Some(i) => parse_amount(get(Some(i)))?,
            None => parse_amount(get(in_col))? - parse_amount(get(out_col))?.abs(),
        };
        if amount_ore == 0 {
            continue;
        }
        let reference = get(reference_col);
        transactions.push(Camt053Transaction {
            booking_date,
            amount_ore,
            description: get(text_col).to_string(),
            reference: (!reference.is_empty()).then(|| reference.to_string()),
        });
    }

    let digest = sha256(text.as_bytes());
    let statement_ref = format!(
        "csv-{}",
        digest[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    Ok(Camt053Statement {
        statement_ref,
        iban: None,
        from_date: transactions.iter().map(|t| t.booking_date).min(),
        to_date: transactions.iter().map(|t| t.booking_date).max(),
        opening_ore: None,
        closing_ore: None,
        transactions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_dnb_style_export() {
        let csv = "\u{feff}Dato;Forklaring;Rentedato;Ut fra konto;Inn på konto\n\
                   10.07.2026;Vipps*Kaffebar;10.07.2026;450,00;\n\
                   12.07.2026;\"Innbetaling; faktura 77\";12.07.2026;;12 500,00\n";
        let statement = parse(csv).unwrap();
        assert_eq!(statement.transactions.len(), 2);
        assert_eq!(statement.transactions[0].amount_ore, -450_00);
        assert_eq!(statement.transactions[0].description, "Vipps*Kaffebar");
        assert_eq!(statement.transactions[1].amount_ore, 12_500_00);
        assert_eq!(
            statement.transactions[1].description, "Innbetaling; faktura 77",
            "quoted delimiter survives"
        );
        assert_eq!(
            statement.transactions[1].booking_date,
            NaiveDate::from_ymd_opt(2026, 7, 12).unwrap()
        );
        assert!(statement.statement_ref.starts_with("csv-"));
    }

    #[test]
    fn parses_a_signed_single_column_export_with_kid() {
        let csv = "Bokføringsdato;Tekst;Beløp;KID\n\
                   2026-07-01;Husleie;-12500,00;\n\
                   2026-07-03;Innbetaling;1.234,56;004417\n";
        let statement = parse(csv).unwrap();
        assert_eq!(statement.transactions[0].amount_ore, -12_500_00);
        assert_eq!(statement.transactions[1].amount_ore, 1_234_56);
        assert_eq!(
            statement.transactions[1].reference.as_deref(),
            Some("004417")
        );
        assert_eq!(statement.from_date, NaiveDate::from_ymd_opt(2026, 7, 1));
    }

    #[test]
    fn rentedato_never_wins_the_date_column() {
        let csv = "Rentedato;Dato;Beløp\n01.01.2000;15.07.2026;100,00\n";
        let statement = parse(csv).unwrap();
        assert_eq!(
            statement.transactions[0].booking_date,
            NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
        );
    }

    #[test]
    fn unbooked_and_zero_rows_are_skipped() {
        let csv = "Dato;Tekst;Beløp\n;Reservert kortkjøp;-99,00\n10.07.2026;Gebyr;0,00\n\
                   10.07.2026;Ekte;-5,00\n";
        let statement = parse(csv).unwrap();
        assert_eq!(statement.transactions.len(), 1);
        assert_eq!(statement.transactions[0].description, "Ekte");
    }

    #[test]
    fn same_file_gets_the_same_ref_different_files_differ() {
        let a = "Dato;Tekst;Beløp\n10.07.2026;A;-5,00\n";
        let b = "Dato;Tekst;Beløp\n10.07.2026;B;-5,00\n";
        assert_eq!(
            parse(a).unwrap().statement_ref,
            parse(a).unwrap().statement_ref
        );
        assert_ne!(
            parse(a).unwrap().statement_ref,
            parse(b).unwrap().statement_ref
        );
    }

    #[test]
    fn unknown_layouts_fail_loudly_with_the_headers() {
        let err = parse("Kolonne1;Kolonne2\nx;y\n").unwrap_err();
        match err {
            BankCsvError::UnknownLayout(headers) => assert!(headers.contains("kolonne1")),
            other => panic!("wrong error: {other:?}"),
        }
        assert_eq!(parse("").unwrap_err(), BankCsvError::Empty);
    }

    #[test]
    fn bad_values_are_rejected_not_guessed() {
        assert!(parse("Dato;Beløp\n32.13.2026;5,00\n").is_err());
        assert!(parse("Dato;Beløp\n10.07.2026;fem kroner\n").is_err());
    }
}
