//! SAF-T Financial *import* parsing — the universal migration path:
//! every Norwegian accounting system must export SAF-T, so this one
//! parser covers moving in from Visma, Tripletex, Fiken, Conta,
//! PowerOffice, Unimicro and the rest.
//!
//! Pure and tolerant, in the camt.053 parser's style: only the fields
//! migration needs are read, unknown elements are skipped, any
//! `1.x` file that follows the structure works. Amounts become integer
//! øre with ledger signs (debit positive). What the ledger does with the
//! parsed file — chain-posted vouchers in one transaction — lives in
//! regnmed-db.

use chrono::NaiveDate;
use thiserror::Error;

use crate::camt053::parse_amount;

#[derive(Debug, Error)]
pub enum SaftImportError {
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("not a SAF-T file: no <AuditFile>/<MasterFiles> content found")]
    NotSaft,
    #[error("line {0}: unparseable {1}")]
    BadField(usize, &'static str),
}

#[derive(Debug, Default)]
pub struct SaftFile {
    pub selection_start: Option<NaiveDate>,
    pub accounts: Vec<ImportAccount>,
    pub customers: Vec<ImportParty>,
    pub suppliers: Vec<ImportParty>,
    pub transactions: Vec<ImportTransaction>,
}

#[derive(Debug)]
pub struct ImportAccount {
    pub account_id: String,
    pub name: String,
    /// Ledger sign: debit positive.
    pub opening_ore: i64,
}

#[derive(Debug)]
pub struct ImportParty {
    pub source_id: String,
    pub name: String,
    pub orgnr: Option<String>,
}

#[derive(Debug)]
pub struct ImportTransaction {
    pub source_id: String,
    pub date: NaiveDate,
    pub description: String,
    pub lines: Vec<ImportLine>,
}

#[derive(Debug)]
pub struct ImportLine {
    pub account_id: String,
    /// Ledger sign: debit positive.
    pub amount_ore: i64,
    pub description: Option<String>,
    pub customer_id: Option<String>,
    pub supplier_id: Option<String>,
    pub tax_code: Option<String>,
}

pub fn parse(xml: &str) -> Result<SaftFile, SaftImportError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut path: Vec<String> = Vec::new();
    let mut file = SaftFile::default();

    // Elements being assembled.
    let mut account = (String::new(), String::new(), 0i64, true); // id, name, amount, debit
    let mut party = (String::new(), String::new(), None::<String>);
    let mut tx = (String::new(), None::<NaiveDate>, String::new());
    let mut lines: Vec<ImportLine> = Vec::new();
    let mut line = ImportLine {
        account_id: String::new(),
        amount_ore: 0,
        description: None,
        customer_id: None,
        supplier_id: None,
        tax_code: None,
    };
    let mut line_amount = (0i64, true); // magnitude, debit

    let mut line_no; // XML source line, for error messages only
    loop {
        let event = reader
            .read_event()
            .map_err(|e| SaftImportError::Xml(e.to_string()))?;
        line_no = xml[..reader.buffer_position() as usize]
            .matches('\n')
            .count()
            + 1;
        match event {
            Event::Start(start) => {
                let local = String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
                match local.as_str() {
                    "Account" if in_path(&path, "GeneralLedgerAccounts") => {
                        account = (String::new(), String::new(), 0, true);
                    }
                    "Customer" | "Supplier" => party = (String::new(), String::new(), None),
                    "Transaction" => {
                        tx = (String::new(), None, String::new());
                        lines = Vec::new();
                    }
                    "Line" => {
                        line = ImportLine {
                            account_id: String::new(),
                            amount_ore: 0,
                            description: None,
                            customer_id: None,
                            supplier_id: None,
                            tax_code: None,
                        };
                        line_amount = (0, true);
                    }
                    _ => {}
                }
                path.push(local);
            }
            Event::End(end) => {
                let local = String::from_utf8_lossy(end.local_name().as_ref()).into_owned();
                match local.as_str() {
                    "Account" if in_path(&path, "GeneralLedgerAccounts") => {
                        file.accounts.push(ImportAccount {
                            account_id: account.0.clone(),
                            name: account.1.clone(),
                            opening_ore: if account.3 { account.2 } else { -account.2 },
                        });
                    }
                    "Customer" => file.customers.push(ImportParty {
                        source_id: party.0.clone(),
                        name: party.1.clone(),
                        orgnr: party.2.take(),
                    }),
                    "Supplier" => file.suppliers.push(ImportParty {
                        source_id: party.0.clone(),
                        name: party.1.clone(),
                        orgnr: party.2.take(),
                    }),
                    "DebitAmount" if in_path(&path, "Line") => {
                        line.amount_ore = line_amount.0;
                    }
                    "CreditAmount" if in_path(&path, "Line") => {
                        line.amount_ore = -line_amount.0;
                    }
                    "Line" if in_path(&path, "Transaction") => {
                        lines.push(std::mem::replace(
                            &mut line,
                            ImportLine {
                                account_id: String::new(),
                                amount_ore: 0,
                                description: None,
                                customer_id: None,
                                supplier_id: None,
                                tax_code: None,
                            },
                        ));
                    }
                    "Transaction" => {
                        let date =
                            tx.1.ok_or(SaftImportError::BadField(line_no, "TransactionDate"))?;
                        file.transactions.push(ImportTransaction {
                            source_id: tx.0.clone(),
                            date,
                            description: tx.2.clone(),
                            lines: std::mem::take(&mut lines),
                        });
                    }
                    _ => {}
                }
                path.pop();
            }
            Event::Text(text) => {
                let value = text
                    .unescape()
                    .map_err(|e| SaftImportError::Xml(e.to_string()))?
                    .into_owned();
                let p: Vec<&str> = path.iter().map(String::as_str).collect();
                match p.as_slice() {
                    [.., "SelectionCriteria", "SelectionStartDate"] => {
                        file.selection_start = value.parse().ok();
                    }
                    [.., "Account", "AccountID"] => account.0 = value,
                    [.., "Account", "AccountDescription"] => account.1 = value,
                    [.., "Account", "OpeningDebitBalance"] => {
                        account.2 = parse_amount(&value)
                            .map_err(|_| SaftImportError::BadField(line_no, "opening balance"))?;
                        account.3 = true;
                    }
                    [.., "Account", "OpeningCreditBalance"] => {
                        account.2 = parse_amount(&value)
                            .map_err(|_| SaftImportError::BadField(line_no, "opening balance"))?;
                        account.3 = false;
                    }
                    [.., "Customer", "CustomerID"] | [.., "Supplier", "SupplierID"] => {
                        party.0 = value;
                    }
                    [.., "Customer", "Name"] | [.., "Supplier", "Name"] => party.1 = value,
                    [.., "Customer", "RegistrationNumber"]
                    | [.., "Supplier", "RegistrationNumber"] => party.2 = Some(value),
                    [.., "Transaction", "TransactionID"] => tx.0 = value,
                    [.., "Transaction", "TransactionDate"] => {
                        tx.1 =
                            Some(value.split('T').next().unwrap_or(&value).parse().map_err(
                                |_| SaftImportError::BadField(line_no, "TransactionDate"),
                            )?);
                    }
                    [.., "Transaction", "Description"] => tx.2 = value,
                    [.., "Line", "AccountID"] => line.account_id = value,
                    [.., "Line", "Description"] => line.description = Some(value),
                    [.., "Line", "CustomerID"] => line.customer_id = Some(value),
                    [.., "Line", "SupplierID"] => line.supplier_id = Some(value),
                    [.., "Line", "DebitAmount", "Amount"]
                    | [.., "Line", "CreditAmount", "Amount"] => {
                        line_amount.0 = parse_amount(&value)
                            .map_err(|_| SaftImportError::BadField(line_no, "amount"))?;
                    }
                    [.., "TaxInformation", "TaxCode"] if in_path(&path, "Line") => {
                        line.tax_code = Some(value);
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if file.accounts.is_empty() && file.transactions.is_empty() {
        return Err(SaftImportError::NotSaft);
    }
    Ok(file)
}

fn in_path(path: &[String], element: &str) -> bool {
    path.iter().any(|p| p == element)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saft::{
        SaftAccount, SaftInput, SaftJournal, SaftLine, SaftParty, SaftTaxCode, SaftTransaction,
    };
    use chrono::{TimeZone, Utc};

    /// Our own exporter's output must survive the importer — the
    /// round-trip that proves both sides speak the same SAF-T.
    #[test]
    fn roundtrip_through_our_own_exporter() {
        let input = SaftInput {
            orgnr: "923609016".into(),
            company_name: "Roundtrip AS".into(),
            contact_first_name: "Kari".into(),
            contact_last_name: "Nordmann".into(),
            file_created: NaiveDate::from_ymd_opt(2026, 7, 23).unwrap(),
            software_version: "test".into(),
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
            accounts: vec![
                SaftAccount {
                    number: "1500".into(),
                    name: "Kundefordringer".into(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    opening_ore: 500_00,
                    closing_ore: 13_000_00,
                },
                SaftAccount {
                    number: "3000".into(),
                    name: "Salgsinntekt".into(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    opening_ore: -500_00,
                    closing_ore: -13_000_00,
                },
            ],
            customers: vec![SaftParty {
                party_no: "10000".into(),
                name: "Kunde & Co AS".into(),
                orgnr: Some("911111111".into()),
                balance_account: Some("1500".into()),
                opening_ore: 500_00,
                closing_ore: 13_000_00,
            }],
            suppliers: vec![],
            tax_codes: vec![SaftTaxCode {
                code: "3".into(),
                description: "Utgående mva".into(),
                percent_bp: 2500,
            }],
            journals: vec![SaftJournal {
                code: "GL".into(),
                name: "Hovedbok".into(),
                transactions: vec![SaftTransaction {
                    fiscal_year: 2026,
                    number: 1,
                    date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                    description: "Faktura 1".into(),
                    created_by: "test".into(),
                    created_at: Utc.with_ymd_and_hms(2026, 2, 1, 12, 0, 0).unwrap(),
                    reverses: None,
                    lines: vec![
                        SaftLine {
                            line_no: 1,
                            account_number: "1500".into(),
                            description: None,
                            amount_ore: 12_500_00,
                            vat_code: None,
                            tax_percent_bp: None,
                            customer_id: Some("10000".into()),
                            supplier_id: None,
                        },
                        SaftLine {
                            line_no: 2,
                            account_number: "3000".into(),
                            description: Some("Salg".into()),
                            amount_ore: -12_500_00,
                            vat_code: Some("3".into()),
                            tax_percent_bp: Some(2500),
                            customer_id: None,
                            supplier_id: None,
                        },
                    ],
                }],
            }],
        };
        let xml = crate::saft::render(&input);
        let parsed = parse(&xml).unwrap();

        assert_eq!(parsed.selection_start.unwrap().to_string(), "2026-01-01");
        assert_eq!(parsed.accounts.len(), 2);
        assert_eq!(parsed.accounts[0].account_id, "1500");
        assert_eq!(parsed.accounts[0].opening_ore, 500_00);
        assert_eq!(parsed.accounts[1].opening_ore, -500_00, "credit opening");

        assert_eq!(parsed.customers.len(), 1);
        assert_eq!(parsed.customers[0].source_id, "10000");
        assert_eq!(parsed.customers[0].orgnr.as_deref(), Some("911111111"));

        assert_eq!(parsed.transactions.len(), 1);
        let tx = &parsed.transactions[0];
        assert_eq!(tx.source_id, "2026-1");
        assert_eq!(tx.date.to_string(), "2026-02-01");
        assert_eq!(tx.lines.len(), 2);
        assert_eq!(tx.lines[0].amount_ore, 12_500_00);
        assert_eq!(tx.lines[0].customer_id.as_deref(), Some("10000"));
        assert_eq!(tx.lines[1].amount_ore, -12_500_00);
        assert_eq!(tx.lines[1].tax_code.as_deref(), Some("3"));
        let sum: i64 = tx.lines.iter().map(|l| l.amount_ore).sum();
        assert_eq!(sum, 0, "signs survive the round-trip");
    }

    #[test]
    fn rejects_non_saft_documents() {
        assert!(matches!(parse("<foo/>"), Err(SaftImportError::NotSaft)));
    }
}
