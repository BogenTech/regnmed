//! Norwegian SAF-T Financial v1.30 export.
//!
//! Renders an `AuditFile` XML document per Skatteetaten's official schema
//! (`docs/saft/Norwegian_SAF-T_Financial_Schema_v_1.30.xsd` in this repo).
//! Pure and deterministic: the same input renders byte-identical XML.
//! Loading the input from the database and writing the file live outside
//! this crate.
//!
//! Amounts stay integer øre all the way; the two-decimal SAF-T format is
//! produced with integer arithmetic only. Signs follow the ledger
//! convention (positive = debit, negative = credit) and are mapped onto
//! SAF-T's DebitAmount/CreditAmount choice.

use std::sync::OnceLock;

use crate::xml::Xml;

use chrono::{DateTime, Datelike, NaiveDate, Utc};

/// Everything needed to render one audit file. Assembled by the caller
/// (regnmed-db for real exports, test fixtures here).
#[derive(Debug)]
pub struct SaftInput {
    pub orgnr: String,
    pub company_name: String,
    /// Norwegian SAF-T requires a contact person in the header.
    pub contact_first_name: String,
    pub contact_last_name: String,
    pub file_created: NaiveDate,
    pub software_version: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub accounts: Vec<SaftAccount>,
    pub customers: Vec<SaftParty>,
    pub suppliers: Vec<SaftParty>,
    pub tax_codes: Vec<SaftTaxCode>,
    /// Dimension registry (avdeling/prosjekt) → AnalysisTypeTable.
    pub analysis_types: Vec<SaftAnalysisType>,
    pub journals: Vec<SaftJournal>,
}

/// One dimension registry entry. `analysis_type` is the SAF-T type
/// code shared by all entries of a kind (regnmed uses "AVD" for
/// avdeling, "PRO" for prosjekt).
#[derive(Debug)]
pub struct SaftAnalysisType {
    pub analysis_type: String,
    pub type_description: String,
    pub id: String,
    pub id_description: String,
    pub active: bool,
}

/// A reskontro party (kunde/leverandør) with its subledger balances.
#[derive(Debug)]
pub struct SaftParty {
    pub party_no: String,
    pub name: String,
    pub orgnr: Option<String>,
    /// The reskontro account the party posts to (1500/2400), when known.
    pub balance_account: Option<String>,
    pub opening_ore: i64,
    pub closing_ore: i64,
}

#[derive(Debug)]
pub struct SaftAccount {
    pub number: String,
    pub name: String,
    pub created: NaiveDate,
    /// Balance at the day before `start`: SUM(amount_ore) of all earlier entries.
    pub opening_ore: i64,
    /// Balance at `end`, inclusive.
    pub closing_ore: i64,
}

#[derive(Debug)]
pub struct SaftTaxCode {
    pub code: String,
    pub description: String,
    /// Rate in basis points (25 % = 2500) so no floats touch tax math.
    pub percent_bp: i64,
}

#[derive(Debug)]
pub struct SaftJournal {
    pub code: String,
    pub name: String,
    pub transactions: Vec<SaftTransaction>,
}

#[derive(Debug)]
pub struct SaftTransaction {
    pub fiscal_year: i32,
    pub number: i64,
    pub date: NaiveDate,
    pub description: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    /// `fiscal_year-number` of the voucher this one reverses, if any.
    pub reverses: Option<String>,
    pub lines: Vec<SaftLine>,
}

#[derive(Debug)]
pub struct SaftLine {
    pub line_no: i32,
    pub account_number: String,
    pub description: Option<String>,
    pub amount_ore: i64,
    pub vat_code: Option<String>,
    /// The rate valid on the voucher date, in basis points — resolved
    /// against the dated `vat_rate` table when loading, so historical
    /// vouchers export with the rate that actually applied.
    pub tax_percent_bp: Option<i64>,
    /// Reskontro party on the line (kundenummer or leverandørnummer).
    pub customer_id: Option<String>,
    pub supplier_id: Option<String>,
    /// Dimension codes on the line → Analysis elements ("AVD"/"PRO").
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

/// An account's mapping onto Skatteetaten's grouping code list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouping {
    pub category: &'static str,
    pub code: &'static str,
    /// False when the account is not itself a standard account and the
    /// nearest one was chosen — worth an accountant's review.
    pub exact: bool,
}

// Skatteetaten owns the næringsspesifikasjon code list and publishes it
// per inntektsår; each vintage is vendored side by side (source: the
// SAF-T GitHub repo, mirrored under docs/saft/) and SELECTED BY THE
// YEAR BEING EXPORTED — exporting 2025 in 2028 must use the list that
// governed 2025, regardless of what is newest. A year no vendored list
// covers fails loudly (docs/regelverk.md, yearly regelverksrevisjon:
// vendor the new list, extend ARGANGER — one reviewed commit).
struct KodelisteArgang {
    label: &'static str,
    first: i32,
    last: i32,
    csv: &'static str,
    grouping: OnceLock<Vec<(u32, &'static str, &'static str)>>,
    names: OnceLock<Vec<(u32, &'static str)>>,
}

static ARGANGER: [KodelisteArgang; 1] = [KodelisteArgang {
    label: "2025-2026",
    first: 2025,
    last: 2026,
    csv: include_str!("saft/naeringsspesifikasjon_2025-2026.csv"),
    grouping: OnceLock::new(),
    names: OnceLock::new(),
}];

fn argang_for(inntektsaar: i32) -> Result<&'static KodelisteArgang, String> {
    ARGANGER
        .iter()
        .find(|a| (a.first..=a.last).contains(&inntektsaar))
        .ok_or_else(|| {
            format!(
                "ingen næringsspesifikasjonsliste dekker inntektsår {inntektsaar} \
                 (vendored: {}) — vendor Skatteetatens liste for året og utvid \
                 registeret (docs/regelverk.md)",
                ARGANGER
                    .iter()
                    .map(|a| a.label)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

impl KodelisteArgang {
    fn grouping_table(&self) -> &[(u32, &'static str, &'static str)] {
        self.grouping.get_or_init(|| {
            let mut rows: Vec<(u32, &str, &str)> = self
                .csv
                .trim_start_matches('\u{feff}')
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let mut fields = line.split(';');
                    let category = fields.next()?;
                    let code = fields.nth(2)?;
                    Some((code.parse().ok()?, category, code))
                })
                .collect();
            rows.sort_by_key(|r| r.0);
            rows
        })
    }

    fn names(&self) -> &[(u32, &'static str)] {
        self.names.get_or_init(|| {
            let mut rows: Vec<(u32, &str)> = self
                .csv
                .trim_start_matches('\u{feff}')
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let mut fields = line.split(';');
                    let code = fields.nth(3)?;
                    let name = fields.next()?;
                    Some((code.parse().ok()?, name))
                })
                .collect();
            rows.sort_by_key(|r| r.0);
            rows.dedup_by_key(|r| r.0);
            rows
        })
    }

    fn grouping_for(&self, account_number: &str) -> Option<Grouping> {
        let number: u32 = account_number.parse().ok()?;
        let table = self.grouping_table();
        let make = |i: usize, exact| Grouping {
            category: table[i].1,
            code: table[i].2,
            exact,
        };
        match table.binary_search_by_key(&number, |r| r.0) {
            Ok(i) => Some(make(i, true)),
            Err(i) => {
                let below = i.checked_sub(1);
                let above = (i < table.len()).then_some(i);
                match (below, above) {
                    (Some(b), Some(a)) if number - table[b].0 <= table[a].0 - number => {
                        Some(make(b, false))
                    }
                    (_, Some(a)) => Some(make(a, false)),
                    (Some(b), None) => Some(make(b, false)),
                    (None, None) => None,
                }
            }
        }
    }
}

/// The label of the code-list vintage governing an inntektsår, or a
/// loud error naming what is vendored.
pub fn kodeliste_argang(inntektsaar: i32) -> Result<&'static str, String> {
    argang_for(inntektsaar).map(|a| a.label)
}

/// The standard accounts (code + Norwegian description) from the NEWEST
/// vendored list — the vocabulary the kontoplan mapping wizard suggests
/// from (`crate::kontoplan`); migration mapping happens "now", so the
/// newest vintage is the right one.
pub fn standard_accounts() -> &'static [(u32, &'static str)] {
    ARGANGER
        .iter()
        .max_by_key(|a| a.last)
        .expect("at least one vendored list")
        .names()
}

/// Maps an account number onto the grouping code list GOVERNING THE
/// GIVEN INNTEKTSÅR: exact match when the account is itself a standard
/// account, otherwise the nearest standard account by number (ties go
/// down). Errs when no vendored list covers the year.
pub fn grouping_for(account_number: &str, inntektsaar: i32) -> Result<Option<Grouping>, String> {
    Ok(argang_for(inntektsaar)?.grouping_for(account_number))
}

/// Renders the complete audit file as UTF-8 XML. Fails loudly when no
/// vendored code list covers the exported inntektsår (the year of the
/// selection start date).
pub fn render(input: &SaftInput) -> Result<String, String> {
    let argang = argang_for(input.start.year())?;
    let mut x = Xml::new();
    x.raw(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.raw(r#"<AuditFile xmlns="urn:StandardAuditFile-Taxation-Financial:NO">"#);
    x.depth = 1;

    header(&mut x, input);
    master_files(&mut x, input, argang);
    entries(&mut x, input);

    x.depth = 0;
    x.raw("</AuditFile>");
    Ok(x.out)
}

fn header(x: &mut Xml, input: &SaftInput) {
    x.open("Header");
    x.leaf("AuditFileVersion", "1.30");
    x.leaf("AuditFileCountry", "NO");
    x.leaf("AuditFileDateCreated", &input.file_created.to_string());
    x.leaf("SoftwareCompanyName", "regnmed");
    x.leaf("SoftwareID", "regnmed");
    x.leaf("SoftwareVersion", &trunc(&input.software_version, 18));
    x.open("Company");
    x.leaf("RegistrationNumber", &input.orgnr);
    x.leaf("Name", &trunc(&input.company_name, 256));
    x.open("Contact");
    x.open("ContactPerson");
    x.leaf("FirstName", &trunc(&input.contact_first_name, 35));
    x.leaf("LastName", &trunc(&input.contact_last_name, 70));
    x.close("ContactPerson");
    x.close("Contact");
    x.close("Company");
    x.leaf("DefaultCurrencyCode", "NOK");
    x.open("SelectionCriteria");
    x.leaf("SelectionStartDate", &input.start.to_string());
    x.leaf("SelectionEndDate", &input.end.to_string());
    x.close("SelectionCriteria");
    x.leaf("TaxAccountingBasis", "A");
    x.close("Header");
}

fn master_files(x: &mut Xml, input: &SaftInput, argang: &KodelisteArgang) {
    x.open("MasterFiles");

    if !input.accounts.is_empty() {
        x.open("GeneralLedgerAccounts");
        for account in &input.accounts {
            x.open("Account");
            x.leaf("AccountID", &account.number);
            x.leaf("AccountDescription", &trunc(&account.name, 256));
            // The schema makes the grouping mandatory; an unmappable account
            // number would be a data error caught long before export.
            let grouping = argang
                .grouping_for(&account.number)
                .expect("account number is numeric and the grouping table is non-empty");
            x.leaf("GroupingCategory", grouping.category);
            x.leaf("GroupingCode", grouping.code);
            x.leaf("AccountType", "GL");
            x.leaf("AccountCreationDate", &account.created.to_string());
            balance(x, "Opening", account.opening_ore);
            balance(x, "Closing", account.closing_ore);
            x.close("Account");
        }
        x.close("GeneralLedgerAccounts");
    }

    render_parties(x, "Customers", "Customer", "CustomerID", &input.customers);
    render_parties(x, "Suppliers", "Supplier", "SupplierID", &input.suppliers);

    if !input.tax_codes.is_empty() {
        x.open("TaxTable");
        x.open("TaxTableEntry");
        x.leaf("TaxType", "MVA");
        x.leaf("Description", "Merverdiavgift");
        for tax in &input.tax_codes {
            x.open("TaxCodeDetails");
            x.leaf("TaxCode", &tax.code);
            x.leaf("Description", &trunc(&tax.description, 256));
            x.leaf("TaxPercentage", &percent(tax.percent_bp));
            x.leaf("Country", "NO");
            // regnmed uses the standard codes directly as its VAT codes.
            x.leaf("StandardTaxCode", &tax.code);
            x.leaf("BaseRate", "100");
            x.close("TaxCodeDetails");
        }
        x.close("TaxTableEntry");
        x.close("TaxTable");
    }

    // Schema order: AnalysisTypeTable follows TaxTable (and UOMTable).
    if !input.analysis_types.is_empty() {
        x.open("AnalysisTypeTable");
        for at in &input.analysis_types {
            x.open("AnalysisTypeTableEntry");
            x.leaf("AnalysisType", &trunc(&at.analysis_type, 9));
            x.leaf("AnalysisTypeDescription", &trunc(&at.type_description, 256));
            x.leaf("AnalysisID", &trunc(&at.id, 35));
            x.leaf("AnalysisIDDescription", &trunc(&at.id_description, 256));
            x.leaf("Status", if at.active { "Active" } else { "Closed" });
            x.close("AnalysisTypeTableEntry");
        }
        x.close("AnalysisTypeTable");
    }

    x.close("MasterFiles");
}

/// One Analysis element on a line, with the amount on the line's side —
/// so per-dimension sums can be read straight from the audit file.
fn render_analysis(x: &mut Xml, analysis_type: &str, id: &str, amount_ore: i64) {
    x.open("Analysis");
    x.leaf("AnalysisType", analysis_type);
    x.leaf("AnalysisID", &trunc(id, 256));
    let side = if amount_ore >= 0 {
        "DebitAnalysisAmount"
    } else {
        "CreditAnalysisAmount"
    };
    x.open(side);
    x.leaf("Amount", &amount(amount_ore));
    x.close(side);
    x.close("Analysis");
}

/// Kunde-/leverandørspesifikasjon in the audit file: minimal mandatory
/// fields plus the subledger balances the schema supports.
fn render_parties(x: &mut Xml, outer: &str, item: &str, id_tag: &str, parties: &[SaftParty]) {
    if parties.is_empty() {
        return;
    }
    x.open(outer);
    for party in parties {
        x.open(item);
        if let Some(orgnr) = &party.orgnr {
            x.leaf("RegistrationNumber", orgnr);
        }
        x.leaf("Name", &trunc(&party.name, 256));
        x.leaf(id_tag, &party.party_no);
        if let Some(account) = &party.balance_account {
            x.open("BalanceAccount");
            x.leaf("AccountID", account);
            balance(x, "Opening", party.opening_ore);
            balance(x, "Closing", party.closing_ore);
            x.close("BalanceAccount");
        }
        x.close(item);
    }
    x.close(outer);
}

fn entries(x: &mut Xml, input: &SaftInput) {
    let transactions: i64 = input
        .journals
        .iter()
        .map(|j| j.transactions.len() as i64)
        .sum();
    let all_lines = || {
        input
            .journals
            .iter()
            .flat_map(|j| &j.transactions)
            .flat_map(|t| &t.lines)
    };
    let total_debit: i64 = all_lines().map(|l| l.amount_ore.max(0)).sum();
    let total_credit: i64 = all_lines().map(|l| (-l.amount_ore).max(0)).sum();

    x.open("GeneralLedgerEntries");
    x.leaf("NumberOfEntries", &transactions.to_string());
    x.leaf("TotalDebit", &amount(total_debit));
    x.leaf("TotalCredit", &amount(total_credit));

    for journal in &input.journals {
        x.open("Journal");
        x.leaf("JournalID", &trunc(&journal.code, 18));
        x.leaf("Description", &trunc(&journal.name, 256));
        x.leaf("Type", &trunc(&journal.code, 9));
        for tx in &journal.transactions {
            x.open("Transaction");
            x.leaf(
                "TransactionID",
                &format!("{}-{}", tx.fiscal_year, tx.number),
            );
            x.leaf("Period", &tx.date.month().to_string());
            x.leaf("PeriodYear", &tx.date.year().to_string());
            x.leaf("TransactionDate", &tx.date.to_string());
            x.leaf("SourceID", &trunc(&tx.created_by, 35));
            x.leaf("Description", &trunc(&tx.description, 256));
            x.leaf("SystemEntryDate", &tx.created_at.date_naive().to_string());
            x.leaf("GLPostingDate", &tx.date.to_string());
            for line in &tx.lines {
                x.open("Line");
                x.leaf("RecordID", &line.line_no.to_string());
                x.leaf("AccountID", &line.account_number);
                // Schema order: Analysis directly after AccountID.
                if let Some(avdeling) = &line.avdeling {
                    render_analysis(x, "AVD", avdeling, line.amount_ore);
                }
                if let Some(prosjekt) = &line.prosjekt {
                    render_analysis(x, "PRO", prosjekt, line.amount_ore);
                }
                if let Some(customer) = &line.customer_id {
                    x.leaf("CustomerID", customer);
                }
                if let Some(supplier) = &line.supplier_id {
                    x.leaf("SupplierID", supplier);
                }
                let description = line.description.as_deref().unwrap_or(&tx.description);
                x.leaf("Description", &trunc(description, 256));
                let side = if line.amount_ore >= 0 {
                    "DebitAmount"
                } else {
                    "CreditAmount"
                };
                x.open(side);
                x.leaf("Amount", &amount(line.amount_ore));
                x.close(side);
                if let Some(code) = &line.vat_code
                    && let Some(bp) = line.tax_percent_bp
                {
                    x.open("TaxInformation");
                    x.leaf("TaxType", "MVA");
                    x.leaf("TaxCode", code);
                    x.leaf("TaxPercentage", &percent(bp));
                    let tax_side = if line.amount_ore >= 0 {
                        "DebitTaxAmount"
                    } else {
                        "CreditTaxAmount"
                    };
                    x.open(tax_side);
                    x.leaf(
                        "Amount",
                        &amount(crate::mva::vat_of_base(line.amount_ore, bp)),
                    );
                    x.close(tax_side);
                    x.close("TaxInformation");
                }
                if let Some(reverses) = &tx.reverses {
                    x.leaf("CrossReference", reverses);
                }
                x.leaf(
                    "SystemEntryTime",
                    &tx.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                );
                x.close("Line");
            }
            x.close("Transaction");
        }
        x.close("Journal");
    }

    x.close("GeneralLedgerEntries");
}

/// Emits the schema's debit/credit choice for a signed øre balance.
fn balance(x: &mut Xml, prefix: &str, ore: i64) {
    let side = if ore >= 0 {
        "DebitBalance"
    } else {
        "CreditBalance"
    };
    x.leaf(&format!("{prefix}{side}"), &amount(ore));
}

/// SAF-T monetary format: absolute value, dot decimal, two decimals.
/// The debit/credit element choice carries the sign.
fn amount(ore: i64) -> String {
    let abs = ore.unsigned_abs();
    format!("{}.{:02}", abs / 100, abs % 100)
}

/// Basis points as a decimal percentage: 2500 → "25", 1550 → "15.5".
fn percent(bp: i64) -> String {
    match (bp / 100, bp % 100) {
        (whole, 0) => format!("{whole}"),
        (whole, frac) if frac % 10 == 0 => format!("{whole}.{}", frac / 10),
        (whole, frac) => format!("{whole}.{frac:02}"),
    }
}

/// Truncates to the schema's character limit for a field (limits are in
/// characters, not bytes).
fn trunc(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixture() -> SaftInput {
        let created_at = Utc.with_ymd_and_hms(2026, 3, 5, 12, 30, 45).unwrap();
        SaftInput {
            orgnr: "999888777".into(),
            company_name: "Demo & Sønn AS".into(),
            contact_first_name: "Kari".into(),
            contact_last_name: "Nordmann".into(),
            file_created: NaiveDate::from_ymd_opt(2026, 7, 22).unwrap(),
            software_version: "0.1.0".into(),
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
            accounts: vec![
                SaftAccount {
                    number: "1920".into(),
                    name: "Bankinnskudd".into(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    opening_ore: 0,
                    closing_ore: 1_250_000,
                },
                SaftAccount {
                    number: "3000".into(),
                    name: "Salgsinntekt <avgiftspliktig>".into(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    opening_ore: 0,
                    closing_ore: -1_000_000,
                },
                SaftAccount {
                    number: "2700".into(),
                    name: "Utgående merverdiavgift".into(),
                    created: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    opening_ore: 0,
                    closing_ore: -250_000,
                },
            ],
            customers: vec![SaftParty {
                party_no: "10001".into(),
                name: "Kunde & Co AS".into(),
                orgnr: Some("911111111".into()),
                balance_account: Some("1500".into()),
                opening_ore: 0,
                closing_ore: 1_250_000,
            }],
            suppliers: vec![],
            tax_codes: vec![SaftTaxCode {
                code: "3".into(),
                description: "Utgående mva, alminnelig sats".into(),
                percent_bp: 2500,
            }],
            analysis_types: vec![
                SaftAnalysisType {
                    analysis_type: "AVD".into(),
                    type_description: "Avdeling".into(),
                    id: "100".into(),
                    id_description: "Oslo".into(),
                    active: true,
                },
                SaftAnalysisType {
                    analysis_type: "PRO".into(),
                    type_description: "Prosjekt".into(),
                    id: "P42".into(),
                    id_description: "Nybygg".into(),
                    active: false,
                },
            ],
            journals: vec![SaftJournal {
                code: "GL".into(),
                name: "Hovedbok".into(),
                transactions: vec![SaftTransaction {
                    fiscal_year: 2026,
                    number: 1,
                    date: NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
                    description: "Salg av konsulenttjenester".into(),
                    created_by: "demo".into(),
                    created_at,
                    reverses: None,
                    lines: vec![
                        SaftLine {
                            line_no: 1,
                            account_number: "1920".into(),
                            description: None,
                            amount_ore: 1_250_000,
                            vat_code: None,
                            tax_percent_bp: None,
                            customer_id: Some("10001".into()),
                            supplier_id: None,
                            avdeling: None,
                            prosjekt: None,
                        },
                        SaftLine {
                            line_no: 2,
                            account_number: "3000".into(),
                            description: Some("Konsulentbistand".into()),
                            amount_ore: -1_000_000,
                            vat_code: Some("3".into()),
                            tax_percent_bp: Some(2500),
                            customer_id: None,
                            supplier_id: None,
                            avdeling: Some("100".into()),
                            prosjekt: Some("P42".into()),
                        },
                        SaftLine {
                            line_no: 3,
                            account_number: "2700".into(),
                            description: None,
                            amount_ore: -250_000,
                            vat_code: None,
                            tax_percent_bp: None,
                            customer_id: None,
                            supplier_id: None,
                            avdeling: None,
                            prosjekt: None,
                        },
                    ],
                }],
            }],
        }
    }

    #[test]
    fn renders_expected_structure() {
        let xml = render(&fixture()).unwrap();
        for expected in [
            "<AuditFileVersion>1.30</AuditFileVersion>",
            "<RegistrationNumber>999888777</RegistrationNumber>",
            "<Name>Demo &amp; Sønn AS</Name>",
            "<FirstName>Kari</FirstName>",
            "<GroupingCategory>balanseverdiForOmloepsmiddel</GroupingCategory>",
            "<AccountDescription>Salgsinntekt &lt;avgiftspliktig&gt;</AccountDescription>",
            "<OpeningDebitBalance>0.00</OpeningDebitBalance>",
            "<ClosingCreditBalance>10000.00</ClosingCreditBalance>",
            "<NumberOfEntries>1</NumberOfEntries>",
            "<TotalDebit>12500.00</TotalDebit>",
            "<TotalCredit>12500.00</TotalCredit>",
            "<TransactionID>2026-1</TransactionID>",
            "<CreditAmount>",
            "<TaxCode>3</TaxCode>",
            "<TaxPercentage>25</TaxPercentage>",
            "<CustomerID>10001</CustomerID>",
            "<Name>Kunde &amp; Co AS</Name>",
            "<SystemEntryTime>2026-03-05T12:30:45</SystemEntryTime>",
        ] {
            assert!(xml.contains(expected), "missing {expected} in:\n{xml}");
        }
        // Deterministic: rendering twice is byte-identical.
        assert_eq!(xml, render(&fixture()).unwrap());
    }

    #[test]
    fn tax_amount_follows_line_side() {
        let xml = render(&fixture()).unwrap();
        // 10000.00 credit at 25 % → 2500.00 CreditTaxAmount.
        assert!(xml.contains("<CreditTaxAmount>"));
        assert!(xml.contains("<Amount>2500.00</Amount>"));
        assert!(!xml.contains("<DebitTaxAmount>"));
    }

    #[test]
    fn grouping_prefers_exact_then_nearest() {
        let exact = grouping_for("1920", 2026).unwrap().unwrap();
        assert!(exact.exact);
        assert_eq!(exact.code, "1920");
        assert_eq!(exact.category, "balanseverdiForOmloepsmiddel");

        // 1921 is not a standard account; nearest is 1920.
        let near = grouping_for("1921", 2026).unwrap().unwrap();
        assert!(!near.exact);
        assert_eq!(near.code, "1920");

        assert!(grouping_for("abcd", 2026).unwrap().is_none());
    }

    #[test]
    fn formats_amounts_and_percentages() {
        assert_eq!(amount(1_250_000), "12500.00");
        assert_eq!(amount(-50), "0.50");
        assert_eq!(amount(0), "0.00");
        assert_eq!(percent(2500), "25");
        assert_eq!(percent(1550), "15.5");
        assert_eq!(percent(1234), "12.34");
    }

    /// Validates the rendered file against Skatteetaten's official XSD.
    /// Skips (with a note) when xmllint is not installed.
    #[test]
    fn validates_against_official_xsd() {
        let xsd = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/saft/Norwegian_SAF-T_Financial_Schema_v_1.30.xsd"
        );
        let dir = std::env::temp_dir().join("regnmed-saft-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("audit.xml");
        std::fs::write(&file, render(&fixture()).unwrap()).unwrap();

        let output = match std::process::Command::new("xmllint")
            .args(["--noout", "--schema", xsd])
            .arg(&file)
            .output()
        {
            Ok(output) => output,
            Err(_) => {
                eprintln!("xmllint not installed — skipping XSD validation");
                return;
            }
        };
        assert!(
            output.status.success(),
            "XSD validation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The code list is selected by the inntektsår being exported —
    /// and a year no vendored list covers fails loudly, never silently
    /// using the nearest vintage (docs/regelverk.md, issue #50).
    #[test]
    fn code_list_is_selected_per_inntektsaar() {
        assert_eq!(kodeliste_argang(2025).unwrap(), "2025-2026");
        assert_eq!(kodeliste_argang(2026).unwrap(), "2025-2026");
        for uncovered in [2024, 2027] {
            let err = kodeliste_argang(uncovered).unwrap_err();
            assert!(err.contains("2025-2026"), "{err}");
            assert!(grouping_for("1920", uncovered).is_err());
        }
        let mut input = fixture();
        input.start = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();
        input.end = NaiveDate::from_ymd_opt(2027, 12, 31).unwrap();
        let err = render(&input).unwrap_err();
        assert!(err.contains("2027"), "{err}");
    }
}
