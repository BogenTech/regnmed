//! Remittering: ISO 20022 pain.001.001.03 (CustomerCreditTransfer-
//! Initiation) — docs/betaling.md, #33.
//!
//! Hand-rolled deterministic XML like the other authority formats
//! (SAF-T, mva-melding): same input, byte-identical file, validated
//! against the official schema (vendored in docs/pain001/) in tests
//! and CI. Amounts are integer øre until the final two-decimal
//! formatting; KID rides as a structured creditor reference (SCOR), a
//! free-text melding as unstructured.
//!
//! Scope (v1): domestic NOK to Norwegian account numbers (BBAN). Foreign
//! payments / IBAN + BIC belong to the file-exchange / PSD2 tier
//! (docs/betaling.md).

use chrono::{DateTime, NaiveDate, Utc};

use crate::kid::is_valid_mod11;
use crate::xml::Xml;

/// A Norwegian kontonummer: 11 digits with a MOD11 check digit (the same
/// cyclic weights as KID MOD11). Dots and spaces are accepted in the
/// input; validate the NORMALISED form.
pub fn normalize_account_number(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn is_valid_account_number(s: &str) -> bool {
    let digits = normalize_account_number(s);
    digits.len() == 11 && is_valid_mod11(&digits)
}

#[derive(Debug, Clone)]
pub struct Payment {
    /// EndToEndId — comes back on the kontoutskrift, ties the debit to
    /// the run item.
    pub end_to_end_id: String,
    pub amount_ore: i64,
    pub creditor_name: String,
    /// Normalized 11-digit BBAN.
    pub creditor_account: String,
    /// KID → structured creditor reference (SCOR).
    pub kid: Option<String>,
    /// Fri melding when there is no KID.
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Pain001Input {
    /// MsgId + PmtInfId — the betalingskjøring's id.
    pub msg_id: String,
    pub created: DateTime<Utc>,
    pub sender_name: String,
    /// Normalized 11-digit BBAN the payments debit.
    pub debtor_account: String,
    pub execution_date: NaiveDate,
    pub payments: Vec<Payment>,
}

/// "1234.56" from øre — integer arithmetic only.
fn amount(ore: i64) -> String {
    format!("{}.{:02}", ore / 100, ore % 100)
}

fn account(x: &mut Xml, tag: &str, bban: &str) {
    x.open(tag);
    x.open("Id");
    x.open("Othr");
    x.leaf("Id", bban);
    x.open("SchmeNm");
    x.leaf("Cd", "BBAN");
    x.close("SchmeNm");
    x.close("Othr");
    x.close("Id");
    x.close(tag);
}

pub fn render(input: &Pain001Input) -> String {
    let sum: i64 = input.payments.iter().map(|b| b.amount_ore).sum();
    let count = input.payments.len();
    let mut x = Xml::new();
    x.raw(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.raw(r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.001.001.03">"#);
    x.depth = 1;
    x.open("CstmrCdtTrfInitn");

    x.open("GrpHdr");
    x.leaf("MsgId", &input.msg_id);
    x.leaf(
        "CreDtTm",
        &input.created.format("%Y-%m-%dT%H:%M:%S").to_string(),
    );
    x.leaf("NbOfTxs", &count.to_string());
    x.leaf("CtrlSum", &amount(sum));
    x.open("InitgPty");
    x.leaf("Nm", &input.sender_name);
    x.close("InitgPty");
    x.close("GrpHdr");

    x.open("PmtInf");
    x.leaf("PmtInfId", &input.msg_id);
    x.leaf("PmtMtd", "TRF");
    x.leaf("NbOfTxs", &count.to_string());
    x.leaf("CtrlSum", &amount(sum));
    x.leaf("ReqdExctnDt", &input.execution_date.to_string());
    x.open("Dbtr");
    x.leaf("Nm", &input.sender_name);
    x.close("Dbtr");
    account(&mut x, "DbtrAcct", &input.debtor_account);
    x.open("DbtrAgt");
    x.empty("FinInstnId");
    x.close("DbtrAgt");

    for betaling in &input.payments {
        x.open("CdtTrfTxInf");
        x.open("PmtId");
        x.leaf("EndToEndId", &betaling.end_to_end_id);
        x.close("PmtId");
        x.open("Amt");
        x.raw(&format!(
            "{}<InstdAmt Ccy=\"NOK\">{}</InstdAmt>",
            "  ".repeat(x.depth),
            amount(betaling.amount_ore)
        ));
        x.close("Amt");
        x.open("Cdtr");
        x.leaf("Nm", &betaling.creditor_name);
        x.close("Cdtr");
        account(&mut x, "CdtrAcct", &betaling.creditor_account);
        match (&betaling.kid, &betaling.message) {
            (Some(kid), _) => {
                x.open("RmtInf");
                x.open("Strd");
                x.open("CdtrRefInf");
                x.open("Tp");
                x.open("CdOrPrtry");
                x.leaf("Cd", "SCOR");
                x.close("CdOrPrtry");
                x.close("Tp");
                x.leaf("Ref", kid);
                x.close("CdtrRefInf");
                x.close("Strd");
                x.close("RmtInf");
            }
            (None, Some(message)) => {
                x.open("RmtInf");
                x.leaf("Ustrd", message);
                x.close("RmtInf");
            }
            (None, None) => {}
        }
        x.close("CdtTrfTxInf");
    }

    x.close("PmtInf");
    x.close("CstmrCdtTrfInitn");
    x.depth = 0;
    x.raw("</Document>");
    x.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Pain001Input {
        Pain001Input {
            msg_id: "regnmed-run-1".into(),
            created: DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            sender_name: "Utlegg & Handel AS".into(),
            debtor_account: "86011117947".into(),
            execution_date: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            payments: vec![
                Payment {
                    end_to_end_id: "regnmed-run-1-1".into(),
                    amount_ore: 12_500_00,
                    creditor_name: "Leverandør A/S".into(),
                    creditor_account: "86011117947".into(),
                    kid: Some("001234567891".into()),
                    message: None,
                },
                Payment {
                    end_to_end_id: "regnmed-run-1-2".into(),
                    amount_ore: 999,
                    creditor_name: "Enkeltmann".into(),
                    creditor_account: "86011117947".into(),
                    kid: None,
                    message: Some("Faktura 42".into()),
                },
            ],
        }
    }

    #[test]
    fn account_number_mod11() {
        assert!(is_valid_account_number("86011117947"));
        assert!(is_valid_account_number("8601.11.17947"), "punktum godtas");
        assert!(is_valid_account_number("8601 11 17947"));
        assert!(
            !is_valid_account_number("86011117948"),
            "feil kontrollsiffer"
        );
        assert!(!is_valid_account_number("8601111794"), "for kort");
        assert!(!is_valid_account_number(""));
    }

    #[test]
    fn renders_deterministically_with_kid_and_melding() {
        let xml = render(&input());
        assert_eq!(xml, render(&input()), "byte-identical");
        assert!(xml.contains("<CtrlSum>12509.99</CtrlSum>"));
        assert!(xml.contains("<NbOfTxs>2</NbOfTxs>"));
        assert!(xml.contains("<InstdAmt Ccy=\"NOK\">12500.00</InstdAmt>"));
        assert!(xml.contains("<Cd>SCOR</Cd>"));
        assert!(xml.contains("<Ref>001234567891</Ref>"));
        assert!(xml.contains("<Ustrd>Faktura 42</Ustrd>"));
        assert!(xml.contains("<Nm>Utlegg &amp; Handel AS</Nm>"), "escaping");
    }

    /// Validates against the vendored official schema. Skips without
    /// xmllint (CI has it).
    #[test]
    fn validates_against_official_xsd() {
        let dir = std::env::temp_dir().join("regnmed-pain001-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pain001.xml");
        std::fs::write(&file, render(&input())).unwrap();
        let xsd = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/pain001/pain.001.001.03.xsd"
        );
        match std::process::Command::new("xmllint")
            .args(["--noout", "--schema", xsd])
            .arg(&file)
            .output()
        {
            Ok(output) => assert!(
                output.status.success(),
                "XSD validation failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(_) => eprintln!("xmllint not installed — skipping XSD validation"),
        }
    }
}
