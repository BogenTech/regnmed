//! OCR giro innbetalinger: parses the fixed-width 80-character record
//! files from Mastercard Payment Services (tidligere Nets/BBS) carrying
//! KID-tagged incoming payments.
//!
//! Record layouts follow the official "Systemspesifikasjon OCR giro"
//! (service code 09), cross-checked against the open-source netsgiro
//! reference implementation. Each line is exactly 80 characters:
//!
//! - `NY000010` transmission start · `NY090020` assignment start
//! - `NY09xx30` amount item 1 (xx = transaction type 10–21): date,
//!   sign, amount (øre), KID
//! - `NY09xx31` amount item 2: form number, bank reference, debit account
//! - `NY090088` assignment end (control: count + sum) · `NY000089`
//!   transmission end
//!
//! Control records are verified: the assignment's transaction count must
//! match, and the sum too (skipped only when reversals — sign '-' — are
//! present, since the spec's totals are gross). KIDs are checked with
//! MOD10/MOD11 and flagged, never rejected: the bank accepted the
//! payment, so we must record it.

use chrono::NaiveDate;
use thiserror::Error;

use crate::kid;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("line {0}: expected an 80-character record starting with NY, got {1} chars")]
    BadRecord(usize, usize),
    #[error("line {0}: unparseable field {1}")]
    BadField(usize, &'static str),
    #[error("no assignment (NY090020) in file")]
    NoAssignment,
    #[error("assignment {0}: control record says {1} transactions, file has {2}")]
    CountMismatch(String, i64, usize),
    #[error("assignment {0}: control sum {1} øre, transactions sum to {2} øre")]
    SumMismatch(String, i64, i64),
}

#[derive(Debug)]
pub struct OcrFile {
    pub transmission_number: String,
    pub data_transmitter: String,
    pub assignments: Vec<OcrAssignment>,
}

#[derive(Debug)]
pub struct OcrAssignment {
    pub agreement_id: String,
    pub assignment_number: String,
    /// The 11-digit oppdragskonto the payments settle into.
    pub bank_account: String,
    pub payments: Vec<OcrPayment>,
}

#[derive(Debug)]
pub struct OcrPayment {
    pub transaction_number: String,
    /// Oppgjørsdato (nets-dato).
    pub date: NaiveDate,
    /// Positive for innbetalinger; negative when the sign field marks a
    /// reversal.
    pub amount_ore: i64,
    pub kid: String,
    /// MOD10/MOD11 verdict — flagged, not rejected.
    pub kid_valid: bool,
    /// The spec's transaction type (10–21: giro, AvtaleGiro, TeleGiro…).
    pub transaction_type: String,
    pub bank_reference: Option<String>,
    pub debit_account: Option<String>,
}

fn slice<'a>(line: &'a str, from: usize, to: usize) -> &'a str {
    &line[from - 1..to]
}

fn parse_nets_date(line_no: usize, text: &str) -> Result<NaiveDate, OcrError> {
    let bad = || OcrError::BadField(line_no, "date");
    let day: u32 = text[0..2].parse().map_err(|_| bad())?;
    let month: u32 = text[2..4].parse().map_err(|_| bad())?;
    let year: i32 = text[4..6].parse().map_err(|_| bad())?;
    NaiveDate::from_ymd_opt(2000 + year, month, day).ok_or_else(bad)
}

pub fn parse(content: &str) -> Result<OcrFile, OcrError> {
    let mut file = OcrFile {
        transmission_number: String::new(),
        data_transmitter: String::new(),
        assignments: Vec::new(),
    };
    let mut open_assignment: Option<OcrAssignment> = None;

    for (index, raw) in content.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if line.len() != 80 || !line.is_ascii() || !line.starts_with("NY") {
            return Err(OcrError::BadRecord(line_no, line.len()));
        }
        let service = slice(line, 3, 4);
        let transaction_type = slice(line, 5, 6);
        let record_type = slice(line, 7, 8);

        match (service, record_type) {
            ("00", "10") => {
                file.data_transmitter = slice(line, 9, 16).to_string();
                file.transmission_number = slice(line, 17, 23).to_string();
            }
            ("09", "20") => {
                open_assignment = Some(OcrAssignment {
                    agreement_id: slice(line, 9, 17).to_string(),
                    assignment_number: slice(line, 18, 24).to_string(),
                    bank_account: slice(line, 25, 35).to_string(),
                    payments: Vec::new(),
                });
            }
            ("09", "30") => {
                let assignment = open_assignment.as_mut().ok_or(OcrError::NoAssignment)?;
                let sign = slice(line, 32, 32);
                let amount: i64 = slice(line, 33, 49)
                    .parse()
                    .map_err(|_| OcrError::BadField(line_no, "amount"))?;
                let kid = slice(line, 50, 74).trim().to_string();
                assignment.payments.push(OcrPayment {
                    transaction_number: slice(line, 9, 15).to_string(),
                    date: parse_nets_date(line_no, slice(line, 16, 21))?,
                    amount_ore: if sign == "-" { -amount } else { amount },
                    kid_valid: kid::is_valid(&kid),
                    kid,
                    transaction_type: transaction_type.to_string(),
                    bank_reference: None,
                    debit_account: None,
                });
            }
            ("09", "31") => {
                let assignment = open_assignment.as_mut().ok_or(OcrError::NoAssignment)?;
                let transaction_number = slice(line, 9, 15);
                if let Some(payment) = assignment
                    .payments
                    .iter_mut()
                    .rev()
                    .find(|p| p.transaction_number == transaction_number)
                {
                    let reference = slice(line, 26, 34).trim_start_matches('0');
                    if !reference.is_empty() {
                        payment.bank_reference = Some(reference.to_string());
                    }
                    let debit_account = slice(line, 48, 58);
                    if debit_account.chars().all(|c| c.is_ascii_digit())
                        && debit_account != "00000000000"
                    {
                        payment.debit_account = Some(debit_account.to_string());
                    }
                }
            }
            ("09", "88") => {
                let assignment = open_assignment.take().ok_or(OcrError::NoAssignment)?;
                let count: i64 = slice(line, 9, 16)
                    .parse()
                    .map_err(|_| OcrError::BadField(line_no, "transaction count"))?;
                let total: i64 = slice(line, 25, 41)
                    .parse()
                    .map_err(|_| OcrError::BadField(line_no, "total amount"))?;
                if count != assignment.payments.len() as i64 {
                    return Err(OcrError::CountMismatch(
                        assignment.assignment_number.clone(),
                        count,
                        assignment.payments.len(),
                    ));
                }
                let has_reversals = assignment.payments.iter().any(|p| p.amount_ore < 0);
                let sum: i64 = assignment.payments.iter().map(|p| p.amount_ore).sum();
                if !has_reversals && total != sum {
                    return Err(OcrError::SumMismatch(
                        assignment.assignment_number.clone(),
                        total,
                        sum,
                    ));
                }
                file.assignments.push(assignment);
            }
            _ => {} // transmission end (00/89) and unknown records: no data we need
        }
    }

    if file.assignments.is_empty() && open_assignment.is_none() {
        return Err(OcrError::NoAssignment);
    }
    // An assignment without its end record still counts — but uncontrolled.
    if let Some(assignment) = open_assignment {
        file.assignments.push(assignment);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(parts: &[&str]) -> String {
        let line: String = parts.concat();
        assert_eq!(line.len(), 80, "fixture record must be 80 chars: {line}");
        line
    }

    fn fixture() -> String {
        // Two payments: 12 500,00 kr on KID 1234566 (mod10) and
        // 150,50 kr on KID 1234560 (mod11).
        [
            record(&[
                "NY000010",
                "00111222",
                "0000170",
                "00008080",
                &"0".repeat(49),
            ]),
            record(&[
                "NY090020",
                "000988555",
                "0000001",
                "99991042764",
                &"0".repeat(45),
            ]),
            record(&[
                "NY091030",
                "0000001",
                "200126",
                "00",
                "20",
                "0",
                "00001",
                "0",
                "00000000001250000",
                "                  1234566",
                "000000",
            ]),
            record(&[
                "NY091031",
                "0000001",
                "0000000000",
                "000654321",
                "0000000",
                "200126",
                "12345678903",
                &"0".repeat(22),
            ]),
            record(&[
                "NY091530",
                "0000002",
                "210126",
                "00",
                "21",
                "0",
                "00002",
                "0",
                "00000000000015050",
                "                  1234560",
                "000000",
            ]),
            record(&[
                "NY090088",
                "00000002",
                "00000005",
                "00000000001265050",
                "200126",
                "210126",
                "210126",
                &"0".repeat(21),
            ]),
            record(&[
                "NY000089",
                "00000002",
                "00000007",
                "00000000001265050",
                "210126",
                &"0".repeat(33),
            ]),
        ]
        .join("\n")
    }

    #[test]
    fn parses_payments_with_kid_and_references() {
        let file = parse(&fixture()).unwrap();
        assert_eq!(file.transmission_number, "0000170");
        assert_eq!(file.assignments.len(), 1);
        let assignment = &file.assignments[0];
        assert_eq!(assignment.bank_account, "99991042764");
        assert_eq!(assignment.payments.len(), 2);

        let first = &assignment.payments[0];
        assert_eq!(first.amount_ore, 1_250_000);
        assert_eq!(first.date.to_string(), "2026-01-20");
        assert_eq!(first.kid, "1234566");
        assert!(first.kid_valid);
        assert_eq!(first.transaction_type, "10");
        assert_eq!(first.bank_reference.as_deref(), Some("654321"));
        assert_eq!(first.debit_account.as_deref(), Some("12345678903"));

        let second = &assignment.payments[1];
        assert_eq!(second.amount_ore, 15_050);
        assert_eq!(second.transaction_type, "15", "AvtaleGiro type");
        assert!(second.kid_valid);
    }

    #[test]
    fn control_records_catch_tampering() {
        let bad_sum = fixture().replace("00000000001265050", "00000000001265051");
        assert!(matches!(parse(&bad_sum), Err(OcrError::SumMismatch(..))));

        let bad_count = fixture().replace("NY09008800000002", "NY09008800000003");
        assert!(matches!(
            parse(&bad_count),
            Err(OcrError::CountMismatch(..))
        ));
    }

    #[test]
    fn invalid_kid_is_flagged_not_rejected() {
        let flipped = fixture().replace("                  1234566", "                  1234567");
        let file = parse(&flipped).unwrap();
        assert!(!file.assignments[0].payments[0].kid_valid);
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(matches!(parse("NY0910"), Err(OcrError::BadRecord(1, 6))));
        assert!(matches!(parse(""), Err(OcrError::NoAssignment)));
    }
}
