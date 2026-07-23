//! camt.053 (ISO 20022 bank-to-customer statement) parsing.
//!
//! The file-based tier of bank connectivity: every Norwegian nettbank can
//! export camt.053, no agreements required. Later tiers (PSD2 aggregator
//! APIs, direct filutveksling) feed the same reconciliation engine —
//! only the transport differs (docs/bank.md).
//!
//! Deliberately tolerant: only the fields reconciliation needs are read,
//! unknown elements are skipped, and any camt.053.001.0x version works.
//! Amounts become integer øre. Signs are converted to **ledger terms for
//! the bank account**: CRDT (money in) = debit = positive, DBIT = credit
//! = negative — so a bank transaction and its ledger entry match when the
//! amounts are *equal*.

use chrono::NaiveDate;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CamtError {
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("not a camt.053 statement: no <Stmt> element")]
    NoStatement,
    #[error("unparseable amount '{0}'")]
    BadAmount(String),
    #[error("unparseable date '{0}'")]
    BadDate(String),
}

#[derive(Debug)]
pub struct Camt053Statement {
    /// The bank's statement id (`Stmt/Id`) — used for idempotent import.
    pub statement_ref: String,
    pub iban: Option<String>,
    pub from_date: Option<NaiveDate>,
    pub to_date: Option<NaiveDate>,
    /// OPBD balance in ledger sign (credit balance at the bank = our
    /// asset = positive).
    pub opening_ore: Option<i64>,
    /// CLBD balance, same convention.
    pub closing_ore: Option<i64>,
    pub transactions: Vec<Camt053Transaction>,
}

#[derive(Debug)]
pub struct Camt053Transaction {
    pub booking_date: NaiveDate,
    /// Ledger sign for the bank account: money in positive.
    pub amount_ore: i64,
    pub description: String,
    /// EndToEndId / KID when present — future reskontro matching.
    pub reference: Option<String>,
}

/// "12500.00" → 1_250_000 øre. camt amounts are unsigned; direction
/// comes from CdtDbtInd.
pub(crate) fn parse_amount(text: &str) -> Result<i64, CamtError> {
    let bad = || CamtError::BadAmount(text.to_string());
    let (whole, frac) = match text.split_once('.') {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    if frac.len() > 2 {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let mut frac_ore: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| bad())?
    };
    if frac.len() == 1 {
        frac_ore *= 10;
    }
    Ok(whole * 100 + frac_ore)
}

/// "2026-01-20" or an ISO datetime — the date part wins.
fn parse_date(text: &str) -> Result<NaiveDate, CamtError> {
    let date_part = text.split('T').next().unwrap_or(text);
    date_part
        .parse()
        .map_err(|_| CamtError::BadDate(text.to_string()))
}

pub fn parse(xml: &str) -> Result<Camt053Statement, CamtError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    // Element path of local names, e.g. ["Document","BkToCstmrStmt","Stmt","Ntry","Amt"].
    let mut path: Vec<String> = Vec::new();
    let mut seen_statement = false;

    let mut statement = Camt053Statement {
        statement_ref: String::new(),
        iban: None,
        from_date: None,
        to_date: None,
        opening_ore: None,
        closing_ore: None,
        transactions: Vec::new(),
    };

    // Balance being assembled (inside <Bal>).
    let (mut bal_code, mut bal_amount, mut bal_credit, mut bal_date) =
        (String::new(), 0i64, true, None::<NaiveDate>);
    // Transaction being assembled (inside <Ntry>).
    let (mut ntry_amount, mut ntry_credit, mut ntry_date, mut ntry_status) =
        (0i64, true, None::<NaiveDate>, String::from("BOOK"));
    let (mut ntry_description, mut ntry_reference) = (String::new(), None::<String>);

    loop {
        let event = reader
            .read_event()
            .map_err(|e| CamtError::Xml(e.to_string()))?;
        match event {
            Event::Start(start) => {
                let local = String::from_utf8_lossy(start.local_name().as_ref()).into_owned();
                if local == "Stmt" && seen_statement {
                    // One statement per import keeps idempotency simple;
                    // banks emit one file per account anyway.
                    break;
                }
                if local == "Bal" {
                    (bal_code, bal_amount, bal_credit, bal_date) = (String::new(), 0, true, None);
                }
                if local == "Ntry" {
                    (ntry_amount, ntry_credit, ntry_date, ntry_status) =
                        (0, true, None, "BOOK".into());
                    ntry_description = String::new();
                    ntry_reference = None;
                }
                path.push(local);
            }
            Event::End(end) => {
                let local = String::from_utf8_lossy(end.local_name().as_ref()).into_owned();
                if local == "Bal" && in_stmt(&path) {
                    let signed = if bal_credit { bal_amount } else { -bal_amount };
                    match bal_code.as_str() {
                        "OPBD" => {
                            statement.opening_ore = Some(signed);
                            statement.from_date = statement.from_date.or(bal_date);
                        }
                        "CLBD" => {
                            statement.closing_ore = Some(signed);
                            statement.to_date = statement.to_date.or(bal_date);
                        }
                        _ => {}
                    }
                }
                if local == "Ntry"
                    && in_stmt(&path)
                    && ntry_status == "BOOK"
                    && let Some(date) = ntry_date
                {
                    statement.transactions.push(Camt053Transaction {
                        booking_date: date,
                        amount_ore: if ntry_credit {
                            ntry_amount
                        } else {
                            -ntry_amount
                        },
                        description: std::mem::take(&mut ntry_description),
                        reference: ntry_reference.take(),
                    });
                }
                if local == "Stmt" {
                    seen_statement = true;
                }
                path.pop();
            }
            Event::Text(text) => {
                let value = text
                    .unescape()
                    .map_err(|e| CamtError::Xml(e.to_string()))?
                    .into_owned();
                let p: Vec<&str> = path.iter().map(String::as_str).collect();
                match p.as_slice() {
                    [.., "Stmt", "Id"] => statement.statement_ref = value,
                    [.., "Stmt", "Acct", "Id", "IBAN"] => statement.iban = Some(value),
                    [.., "Bal", "Tp", "CdOrPrtry", "Cd"] => bal_code = value,
                    [.., "Bal", "Amt"] => bal_amount = parse_amount(&value)?,
                    [.., "Bal", "CdtDbtInd"] => bal_credit = value == "CRDT",
                    [.., "Bal", "Dt", "Dt" | "DtTm"] => bal_date = Some(parse_date(&value)?),
                    [.., "Ntry", "Amt"] => ntry_amount = parse_amount(&value)?,
                    [.., "Ntry", "CdtDbtInd"] => ntry_credit = value == "CRDT",
                    [.., "Ntry", "Sts"] | [.., "Ntry", "Sts", "Cd"] => ntry_status = value,
                    [.., "Ntry", "BookgDt", "Dt" | "DtTm"] => ntry_date = Some(parse_date(&value)?),
                    [.., "Refs", "EndToEndId"] if within(&p, "Ntry") => {
                        if value != "NOTPROVIDED" {
                            ntry_reference = Some(value);
                        }
                    }
                    [.., "RmtInf", "Ustrd"] if within(&p, "Ntry") => {
                        if ntry_description.is_empty() {
                            ntry_description = value;
                        }
                    }
                    [.., "Ntry", "AddtlNtryInf"] => {
                        if ntry_description.is_empty() {
                            ntry_description = value;
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !seen_statement {
        return Err(CamtError::NoStatement);
    }
    Ok(statement)
}

fn in_stmt(path: &[String]) -> bool {
    path.iter().any(|p| p == "Stmt")
}

fn within(path: &[&str], element: &str) -> bool {
    path.contains(&element)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
 <BkToCstmrStmt>
  <GrpHdr><MsgId>M-1</MsgId><CreDtTm>2026-02-01T04:00:00</CreDtTm></GrpHdr>
  <Stmt>
   <Id>ST-2026-001</Id>
   <Acct><Id><IBAN>NO9386011117947</IBAN></Id><Ccy>NOK</Ccy></Acct>
   <Bal>
    <Tp><CdOrPrtry><Cd>OPBD</Cd></CdOrPrtry></Tp>
    <Amt Ccy="NOK">1000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
    <Dt><Dt>2026-01-01</Dt></Dt>
   </Bal>
   <Bal>
    <Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
    <Amt Ccy="NOK">13350.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>
    <Dt><Dt>2026-01-31</Dt></Dt>
   </Bal>
   <Ntry>
    <Amt Ccy="NOK">12500.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts>
    <BookgDt><Dt>2026-01-20</Dt></BookgDt><ValDt><Dt>2026-01-20</Dt></ValDt>
    <NtryDtls><TxDtls>
     <Refs><EndToEndId>INV-1001</EndToEndId></Refs>
     <RmtInf><Ustrd>Faktura 1001 &amp; oppgjør</Ustrd></RmtInf>
    </TxDtls></NtryDtls>
   </Ntry>
   <Ntry>
    <Amt Ccy="NOK">150.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>BOOK</Sts>
    <BookgDt><Dt>2026-01-25</Dt></BookgDt>
    <AddtlNtryInf>Gebyr</AddtlNtryInf>
   </Ntry>
   <Ntry>
    <Amt Ccy="NOK">999.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>PDNG</Sts>
    <BookgDt><Dt>2026-01-30</Dt></BookgDt>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>"#;

    #[test]
    fn parses_statement_balances_and_transactions() {
        let statement = parse(SAMPLE).unwrap();
        assert_eq!(statement.statement_ref, "ST-2026-001");
        assert_eq!(statement.iban.as_deref(), Some("NO9386011117947"));
        assert_eq!(statement.opening_ore, Some(100_000));
        assert_eq!(statement.closing_ore, Some(1_335_000));
        assert_eq!(statement.from_date.unwrap().to_string(), "2026-01-01");
        assert_eq!(statement.to_date.unwrap().to_string(), "2026-01-31");

        // The pending (PDNG) entry is skipped: 2 booked transactions.
        assert_eq!(statement.transactions.len(), 2);
        let inflow = &statement.transactions[0];
        assert_eq!(inflow.amount_ore, 1_250_000, "CRDT = money in = positive");
        assert_eq!(inflow.booking_date.to_string(), "2026-01-20");
        assert_eq!(inflow.reference.as_deref(), Some("INV-1001"));
        assert_eq!(inflow.description, "Faktura 1001 & oppgjør");

        let outflow = &statement.transactions[1];
        assert_eq!(outflow.amount_ore, -15_000, "DBIT = money out = negative");
        assert_eq!(outflow.description, "Gebyr");
    }

    #[test]
    fn amounts_parse_as_integer_ore() {
        assert_eq!(parse_amount("12500.00").unwrap(), 1_250_000);
        assert_eq!(parse_amount("0.5").unwrap(), 50);
        assert_eq!(parse_amount("7").unwrap(), 700);
        assert!(parse_amount("1.234").is_err());
        assert!(parse_amount("abc").is_err());
    }

    #[test]
    fn rejects_non_camt_documents() {
        assert!(matches!(
            parse("<foo><bar/></foo>"),
            Err(CamtError::NoStatement)
        ));
    }
}
