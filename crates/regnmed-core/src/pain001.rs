//! Remittering: ISO 20022 pain.001.001.03 (CustomerCreditTransfer-
//! Initiation) — docs/betaling.md, #33.
//!
//! Hand-rolled deterministic XML like the other authority formats
//! (SAF-T, mva-melding): same input, byte-identical file, validated
//! against the official schema (vendored in docs/pain001/) in tests
//! and CI. Amounts are integer øre until the final two-decimal
//! formatting; KID rides as structured creditor reference (SCOR), fri
//! melding as ustrukturert.
//!
//! Scope (v1): innenlands NOK til norske kontonumre (BBAN). Utland/
//! IBAN + BIC er filutvekslings-/PSD2-tierens sak (docs/betaling.md).

use chrono::{DateTime, NaiveDate, Utc};

use crate::kid::is_valid_mod11;
use crate::xml::Xml;

/// Et norsk kontonummer: 11 siffer med MOD11-kontrollsiffer (samme
/// sykliske vekter som KID MOD11). Punktum og mellomrom godtas i
/// input; valider den NORMALISERTE formen.
pub fn normaliser_kontonummer(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn gyldig_kontonummer(s: &str) -> bool {
    let digits = normaliser_kontonummer(s);
    digits.len() == 11 && is_valid_mod11(&digits)
}

#[derive(Debug, Clone)]
pub struct Betaling {
    /// EndToEndId — comes back on the kontoutskrift, ties the debit to
    /// the run item.
    pub end_to_end_id: String,
    pub belop_ore: i64,
    pub kreditor_navn: String,
    /// Normalized 11-digit BBAN.
    pub kreditor_konto: String,
    /// KID → structured creditor reference (SCOR).
    pub kid: Option<String>,
    /// Fri melding when there is no KID.
    pub melding: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Pain001Input {
    /// MsgId + PmtInfId — the betalingskjøring's id.
    pub msg_id: String,
    pub created: DateTime<Utc>,
    pub avsender_navn: String,
    /// Normalized 11-digit BBAN the payments debit.
    pub debitor_konto: String,
    pub execution_date: NaiveDate,
    pub betalinger: Vec<Betaling>,
}

/// "1234.56" from øre — integer arithmetic only.
fn amount(ore: i64) -> String {
    format!("{}.{:02}", ore / 100, ore % 100)
}

fn konto(x: &mut Xml, tag: &str, bban: &str) {
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
    let sum: i64 = input.betalinger.iter().map(|b| b.belop_ore).sum();
    let count = input.betalinger.len();
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
    x.leaf("Nm", &input.avsender_navn);
    x.close("InitgPty");
    x.close("GrpHdr");

    x.open("PmtInf");
    x.leaf("PmtInfId", &input.msg_id);
    x.leaf("PmtMtd", "TRF");
    x.leaf("NbOfTxs", &count.to_string());
    x.leaf("CtrlSum", &amount(sum));
    x.leaf("ReqdExctnDt", &input.execution_date.to_string());
    x.open("Dbtr");
    x.leaf("Nm", &input.avsender_navn);
    x.close("Dbtr");
    konto(&mut x, "DbtrAcct", &input.debitor_konto);
    x.open("DbtrAgt");
    x.empty("FinInstnId");
    x.close("DbtrAgt");

    for betaling in &input.betalinger {
        x.open("CdtTrfTxInf");
        x.open("PmtId");
        x.leaf("EndToEndId", &betaling.end_to_end_id);
        x.close("PmtId");
        x.open("Amt");
        x.raw(&format!(
            "{}<InstdAmt Ccy=\"NOK\">{}</InstdAmt>",
            "  ".repeat(x.depth),
            amount(betaling.belop_ore)
        ));
        x.close("Amt");
        x.open("Cdtr");
        x.leaf("Nm", &betaling.kreditor_navn);
        x.close("Cdtr");
        konto(&mut x, "CdtrAcct", &betaling.kreditor_konto);
        match (&betaling.kid, &betaling.melding) {
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
            (None, Some(melding)) => {
                x.open("RmtInf");
                x.leaf("Ustrd", melding);
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
            avsender_navn: "Utlegg & Handel AS".into(),
            debitor_konto: "86011117947".into(),
            execution_date: NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
            betalinger: vec![
                Betaling {
                    end_to_end_id: "regnmed-run-1-1".into(),
                    belop_ore: 12_500_00,
                    kreditor_navn: "Leverandør A/S".into(),
                    kreditor_konto: "86011117947".into(),
                    kid: Some("001234567891".into()),
                    melding: None,
                },
                Betaling {
                    end_to_end_id: "regnmed-run-1-2".into(),
                    belop_ore: 999,
                    kreditor_navn: "Enkeltmann".into(),
                    kreditor_konto: "86011117947".into(),
                    kid: None,
                    melding: Some("Faktura 42".into()),
                },
            ],
        }
    }

    #[test]
    fn kontonummer_mod11() {
        assert!(gyldig_kontonummer("86011117947"));
        assert!(gyldig_kontonummer("8601.11.17947"), "punktum godtas");
        assert!(gyldig_kontonummer("8601 11 17947"));
        assert!(!gyldig_kontonummer("86011117948"), "feil kontrollsiffer");
        assert!(!gyldig_kontonummer("8601111794"), "for kort");
        assert!(!gyldig_kontonummer(""));
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
