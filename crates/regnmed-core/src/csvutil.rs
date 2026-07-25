//! Shared CSV primitives for the layout-detecting importers.
//!
//! Two importers read files that humans exported from someone else's
//! system — bank statements ([`crate::bankcsv`], docs/bank.md) and
//! migration lists ([`crate::migreringcsv`], docs/migration.md). Neither
//! keeps per-vendor profiles: the layout is detected from the header
//! row, because header vocabularies drift far less than product names
//! do, and a file we cannot read fails loudly listing the headers we
//! saw. These are the pieces both need — one implementation, so a fix
//! to quoting or to Norwegian number formats reaches both.

use chrono::NaiveDate;

/// Splits one CSV record, honoring double quotes (`"a;b"` is one field,
/// `""` inside quotes is an escaped quote).
pub(crate) fn split_record(line: &str, delimiter: char) -> Vec<String> {
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

pub(crate) fn detect_delimiter(header: &str) -> char {
    for candidate in [';', '\t', ','] {
        if header.contains(candidate) {
            return candidate;
        }
    }
    ';'
}

/// Lowercased, æøå folded, punctuation dropped — so "Beløp NOK",
/// "belop_nok" and "BELOPNOK" all compare equal enough.
pub(crate) fn norm(header: &str) -> String {
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

/// Header-first search: walks the headers looking for any candidate,
/// exact match before containment, skipping anything with an avoided
/// word ("rentedato" must not win the date column).
pub(crate) fn find_column(
    headers: &[String],
    candidates: &[&str],
    avoid: &[&str],
) -> Option<usize> {
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

/// Candidate-first search: the candidate list is a PRIORITY order, so a
/// file carrying both "Beløp" and "Restbeløp" yields the remainder when
/// "restbelop" is listed first. Use this where two plausible columns
/// mean different things.
pub(crate) fn find_column_ranked(
    headers: &[String],
    candidates: &[&str],
    avoid: &[&str],
) -> Option<usize> {
    let allowed = |header: &String| !avoid.iter().any(|a| header.contains(a));
    for candidate in candidates {
        for (i, header) in headers.iter().enumerate() {
            if allowed(header) && header == candidate {
                return Some(i);
            }
        }
    }
    for candidate in candidates {
        for (i, header) in headers.iter().enumerate() {
            if allowed(header) && header.contains(candidate) {
                return Some(i);
            }
        }
    }
    None
}

/// "1 234,56" / "1.234,56" / "-450,00" / "1234.56" → øre. If a comma is
/// present it is the decimal separator (Norwegian exports); otherwise a
/// dot is. Empty is zero — a blank cell is not an error.
pub(crate) fn parse_amount(raw: &str) -> Result<i64, String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}')
        .collect();
    if cleaned.is_empty() {
        return Ok(0);
    }
    let bad = || raw.to_string();
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

pub(crate) fn parse_date(raw: &str) -> Result<NaiveDate, String> {
    let raw = raw.trim();
    for format in ["%d.%m.%Y", "%Y-%m-%d", "%d/%m/%Y", "%d.%m.%y"] {
        if let Ok(date) = NaiveDate::parse_from_str(raw, format) {
            return Ok(date);
        }
    }
    Err(raw.to_string())
}

/// Header row + delimiter of a CSV, BOM stripped.
pub(crate) struct CsvHeader {
    pub delimiter: char,
    pub headers: Vec<String>,
}

pub(crate) fn read_header(text: &str) -> Option<(CsvHeader, std::str::Lines<'_>)> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    let header_line = loop {
        let line = lines.next()?;
        if !line.trim().is_empty() {
            break line;
        }
    };
    let delimiter = detect_delimiter(header_line);
    let headers = split_record(header_line, delimiter)
        .iter()
        .map(|h| norm(h))
        .collect();
    Some((CsvHeader { delimiter, headers }, lines))
}
