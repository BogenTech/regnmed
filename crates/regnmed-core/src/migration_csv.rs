//! Migration import, file tier (docs/migration.md, #19).
//!
//! SAF-T moves the hovedbok (crate::saft_import), but it does not carry
//! everything a byrå needs in order to stop using the old system:
//! **contact details** and **open reskontro items** are left behind. All
//! the Norwegian systems (Tripletex, Fiken, Visma eAccounting,
//! PowerOffice, Conta) can export both as CSV/Excel today, without API
//! keys — so the file tier comes first, exactly as for bank
//! (docs/bank.md).
//!
//! The layout is read from the **column headers**, not from a per-vendor
//! profile: the header vocabulary changes more slowly than the product
//! names, and a file we do not understand fails loudly listing the
//! columns we actually saw — never a silent half import.
//!
//! Two sign rules are worth noting:
//! - Open items are read as the **outstanding amount**, with the file's
//!   own sign. A kreditnota in a customer list arrives negative and stays
//!   negative.
//! - Which way the item points in the hovedbok is decided by the party
//!   kind (kunde = debit, leverandør = credit), not by the file: the
//!   exports disagree with each other about signs, but never about who
//!   owes whom.

use chrono::NaiveDate;

use crate::csvutil::{find_column, find_column_ranked, read_header, split_record};

#[derive(Debug, PartialEq, Eq)]
pub enum MigrationError {
    Empty,
    /// Carries the headers we saw, so the human can see what we read.
    UnknownLayout(String),
    BadDate(String),
    BadAmount(String),
    /// A row without the one thing that makes it usable.
    MissingField {
        line: usize,
        field: &'static str,
    },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "filen er tom"),
            Self::UnknownLayout(headers) => write!(
                f,
                "forstod ikke kolonnene i filen (kolonner: {headers}) — \
                 eksporter listen med kolonneoverskrifter fra det gamle systemet"
            ),
            Self::BadDate(value) => write!(f, "ugyldig dato '{value}'"),
            Self::BadAmount(value) => write!(f, "ugyldig beløp '{value}'"),
            Self::MissingField { line, field } => {
                write!(f, "linje {line}: mangler {field}")
            }
        }
    }
}

impl std::error::Error for MigrationError {}

/// Kunde or leverandør — the same two kinds the reskontro knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyKind {
    Customer,
    Supplier,
}

impl PartyKind {
    pub fn parse(raw: &str) -> Option<Self> {
        let n = raw.trim().to_lowercase();
        if n.starts_with("kunde") || n.starts_with("customer") || n == "debitor" {
            Some(Self::Customer)
        } else if n.starts_with("lever") || n.starts_with("supplier") || n == "kreditor" {
            Some(Self::Supplier)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Customer => "kunde",
            Self::Supplier => "leverandor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRow {
    pub kind: PartyKind,
    /// The old system's customer/supplier number, when the export has
    /// one — the most reliable key back to the source.
    pub number: Option<String>,
    pub name: String,
    pub orgnumber: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub bank_account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenItemRow {
    /// Number if the export has one, otherwise the name — resolution
    /// against the party register happens in regnmed-db.
    pub party: String,
    pub party_name: Option<String>,
    pub document: Option<String>,
    pub kid: Option<String>,
    pub date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    /// Outstanding amount with the file's own sign.
    pub amount_ore: i64,
}

fn cell(fields: &[String], index: Option<usize>) -> String {
    index
        .and_then(|i| fields.get(i))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn some_nonempty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

/// Kontakter (kunder eller leverandører). `default_kind` is what the
/// human said the file is; a `type`-column in the file wins per row —
/// some systems export one combined contact list.
pub fn parse_contacts(
    text: &str,
    default_kind: PartyKind,
) -> Result<Vec<ContactRow>, MigrationError> {
    let (header, lines) = read_header(text).ok_or(MigrationError::Empty)?;
    let h = &header.headers;

    let navn_col = find_column_ranked(
        h,
        &[
            "navn",
            "kundenavn",
            "leverandornavn",
            "firmanavn",
            "name",
            "customer",
            "supplier",
        ],
        &["kontaktperson", "contact person"],
    );
    let nummer_col = find_column_ranked(
        h,
        &[
            "kundenr",
            "kundenummer",
            "leverandornr",
            "leverandornummer",
            "kundeleverandornr",
            "nummer",
            "number",
            "nr",
        ],
        &["orgnr", "organisasjonsnummer", "kontonr", "kontonummer"],
    );
    let orgnr_col = find_column_ranked(
        h,
        &["orgnr", "organisasjonsnummer", "org nr", "vat", "vatnumber"],
        &[],
    );
    let epost_col = find_column_ranked(h, &["epost", "e post", "email", "e mail"], &[]);
    let adresse_col = find_column_ranked(
        h,
        &["adresse", "gateadresse", "postadresse", "address"],
        &["epost", "e post", "email"],
    );
    let account_col = find_column_ranked(
        h,
        &[
            "kontonummer",
            "bankkonto",
            "bankkontonummer",
            "bank account",
        ],
        &["kundenr", "leverandornr"],
    );
    let type_col = find_column(h, &["type", "kundeleverandor", "parttype"], &[]);

    let navn_col = navn_col.ok_or_else(|| MigrationError::UnknownLayout(h.join(", ")))?;

    let mut rader = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_record(line, header.delimiter);
        let name = cell(&fields, Some(navn_col));
        if name.is_empty() {
            continue;
        }
        let kind = some_nonempty(cell(&fields, type_col))
            .and_then(|t| PartyKind::parse(&t))
            .unwrap_or(default_kind);
        rader.push(ContactRow {
            kind,
            number: some_nonempty(cell(&fields, nummer_col)),
            name,
            orgnumber: some_nonempty(
                cell(&fields, orgnr_col)
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect(),
            ),
            email: some_nonempty(cell(&fields, epost_col)),
            address: some_nonempty(cell(&fields, adresse_col)),
            bank_account: some_nonempty(cell(&fields, account_col)),
        });
        let _ = i;
    }
    Ok(rader)
}

/// Åpne poster. The amount column is chosen by PRIORITY: a file with
/// both "Beløp" (invoice total) and "Restbeløp" (outstanding) must
/// yield the remainder — importing gross amounts as open items would
/// overstate the reskontro silently, which is exactly the kind of
/// quiet wrongness this system refuses.
pub fn parse_open_items(text: &str) -> Result<Vec<OpenItemRow>, MigrationError> {
    let (header, lines) = read_header(text).ok_or(MigrationError::Empty)?;
    let h = &header.headers;

    let amount_col = find_column_ranked(
        h,
        &[
            "restbelop",
            "restsum",
            "utestaende",
            "apent belop",
            "gjenstar",
            "saldo",
            "belop",
            "amount",
            "sum",
        ],
        &["valuta", "mva", "vat"],
    );
    let part_col = find_column_ranked(
        h,
        &[
            "kundenr",
            "kundenummer",
            "leverandornr",
            "leverandornummer",
            "partsnr",
            "kontonr",
        ],
        &["fakturanr", "bilagsnr", "orgnr"],
    );
    let navn_col = find_column_ranked(
        h,
        &["navn", "kundenavn", "leverandornavn", "name", "motpart"],
        &["kontaktperson"],
    );
    let dokument_col = find_column_ranked(
        h,
        &[
            "fakturanr",
            "fakturanummer",
            "bilagsnr",
            "bilagsnummer",
            "dokumentnr",
            "invoice",
            "invoice no",
        ],
        &[],
    );
    let kid_col = find_column_ranked(h, &["kid"], &[]);
    let date_col = find_column_ranked(
        h,
        &["fakturadato", "bilagsdato", "dato", "date"],
        &["forfall", "due", "betalt"],
    );
    let forfall_col = find_column_ranked(h, &["forfallsdato", "forfall", "due date", "due"], &[]);

    let amount_col = amount_col.ok_or_else(|| MigrationError::UnknownLayout(h.join(", ")))?;
    if part_col.is_none() && navn_col.is_none() {
        return Err(MigrationError::UnknownLayout(h.join(", ")));
    }

    let mut rader = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 2; // header is line 1
        let fields = split_record(line, header.delimiter);
        let amount_raw = cell(&fields, Some(amount_col));
        if amount_raw.is_empty() {
            continue;
        }
        let amount_ore =
            crate::csvutil::parse_amount(&amount_raw).map_err(MigrationError::BadAmount)?;
        if amount_ore == 0 {
            // A settled item is not an open item.
            continue;
        }
        let number = some_nonempty(cell(&fields, part_col));
        let name = some_nonempty(cell(&fields, navn_col));
        let party =
            number
                .clone()
                .or_else(|| name.clone())
                .ok_or(MigrationError::MissingField {
                    line: line_no,
                    field: "kunde/leverandør",
                })?;
        let parse_day = |raw: String| -> Result<Option<NaiveDate>, MigrationError> {
            match some_nonempty(raw) {
                None => Ok(None),
                Some(value) => crate::csvutil::parse_date(&value)
                    .map(Some)
                    .map_err(MigrationError::BadDate),
            }
        };
        rader.push(OpenItemRow {
            party,
            party_name: name,
            document: some_nonempty(cell(&fields, dokument_col)),
            kid: some_nonempty(
                cell(&fields, kid_col)
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect(),
            ),
            date: parse_day(cell(&fields, date_col))?,
            due_date: parse_day(cell(&fields, forfall_col))?,
            amount_ore,
        });
    }
    Ok(rader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tripletex_style_customer_list() {
        // Semicolon, Norwegian headers, quoted name with the delimiter.
        let csv = "Kundenr;Navn;Organisasjonsnummer;E-post;Adresse;Kontonummer\n\
                   10001;\"Hansen; Berg AS\";915933149;post@hansen.no;Storgata 1;8601.11.17947\n\
                   10002;Lille Bakeri;;;;\n";
        let rader = parse_contacts(csv, PartyKind::Customer).unwrap();
        assert_eq!(rader.len(), 2);
        assert_eq!(
            rader[0].name, "Hansen; Berg AS",
            "sitat beskytter skilletegn"
        );
        assert_eq!(rader[0].number.as_deref(), Some("10001"));
        assert_eq!(rader[0].orgnumber.as_deref(), Some("915933149"));
        assert_eq!(rader[0].bank_account.as_deref(), Some("8601.11.17947"));
        assert_eq!(rader[0].kind, PartyKind::Customer);
        assert_eq!(
            rader[1].orgnumber, None,
            "empty fields become None, not empty strings"
        );
    }

    #[test]
    fn an_english_export_with_a_type_column_beats_the_default_kind() {
        let csv = "Number,Name,Type,Email\n\
                   S-1,Supplier AS,Supplier,post@supplier.no\n\
                   C-1,Customer AS,Customer,post@customer.no\n";
        let rader = parse_contacts(csv, PartyKind::Customer).unwrap();
        assert_eq!(rader[0].kind, PartyKind::Supplier, "typekolonnen bestemmer");
        assert_eq!(rader[1].kind, PartyKind::Customer);
        assert_eq!(rader[0].email.as_deref(), Some("post@supplier.no"));
    }

    #[test]
    fn a_contact_list_without_a_name_column_fails_loudly() {
        let csv = "Kundenr;Saldo\n10001;500,00\n";
        let error = parse_contacts(csv, PartyKind::Customer).unwrap_err();
        match error {
            MigrationError::UnknownLayout(headers) => {
                assert!(
                    headers.contains("kundenr"),
                    "feilen viser kolonnene: {headers}"
                );
            }
            other => panic!("forventet UnknownLayout, fikk {other:?}"),
        }
    }

    #[test]
    fn remaining_amount_beats_the_invoice_total() {
        // The trap: importing "Beløp" would overstate the reskontro.
        let csv = "Kundenr;Navn;Fakturanr;Fakturadato;Forfallsdato;Beløp;Restbeløp;KID\n\
                   10001;Hansen AS;F-100;15.01.2026;29.01.2026;12 500,00;2 500,00;1234567897\n";
        let rader = parse_open_items(csv).unwrap();
        assert_eq!(rader.len(), 1);
        assert_eq!(
            rader[0].amount_ore, 2_500_00,
            "the outstanding amount, not the invoice total"
        );
        assert_eq!(rader[0].party, "10001");
        assert_eq!(rader[0].party_name.as_deref(), Some("Hansen AS"));
        assert_eq!(rader[0].document.as_deref(), Some("F-100"));
        assert_eq!(rader[0].kid.as_deref(), Some("1234567897"));
        assert_eq!(
            rader[0].date,
            Some(NaiveDate::from_ymd_opt(2026, 1, 15).unwrap())
        );
        assert_eq!(
            rader[0].due_date,
            Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap())
        );
    }

    #[test]
    fn a_kreditnota_keeps_its_negative_sign() {
        let csv = "Navn;Fakturanr;Restbeløp\n\
                   Hansen AS;F-101;-1 000,00\n\
                   Hansen AS;F-102;0,00\n";
        let rader = parse_open_items(csv).unwrap();
        assert_eq!(rader.len(), 1, "settled items are not open items");
        assert_eq!(rader[0].amount_ore, -1_000_00);
        assert_eq!(rader[0].party, "Hansen AS", "navn når nummer mangler");
    }

    #[test]
    fn open_items_without_an_amount_column_fail_loudly() {
        let csv = "Kundenr;Navn;Fakturanr\n10001;Hansen AS;F-100\n";
        let error = parse_open_items(csv).unwrap_err();
        assert!(matches!(error, MigrationError::UnknownLayout(_)));
        assert!(error.to_string().contains("fakturanr"));
    }

    #[test]
    fn tabs_and_iso_dates_work_too() {
        let csv = "Leverandørnr\tNavn\tBilagsnr\tDato\tSaldo\n\
                   L-9\tGrossisten AS\tI-77\t2026-02-01\t-4500.50\n";
        let rader = parse_open_items(csv).unwrap();
        assert_eq!(rader[0].party, "L-9");
        assert_eq!(rader[0].amount_ore, -4_500_50);
        assert_eq!(
            rader[0].date,
            Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap())
        );
    }

    #[test]
    fn the_due_date_does_not_steal_the_invoice_date_column() {
        let csv = "Navn;Forfallsdato;Fakturadato;Restbeløp\n\
                   Hansen AS;29.01.2026;15.01.2026;100,00\n";
        let rader = parse_open_items(csv).unwrap();
        assert_eq!(
            rader[0].date,
            Some(NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
            "fakturadato, selv om forfall står først"
        );
        assert_eq!(
            rader[0].due_date,
            Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap())
        );
    }
}
