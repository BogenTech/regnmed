//! Migreringsimport, filtier (docs/migration.md, #19).
//!
//! SAF-T flytter hovedboken (crate::saft_import), men den bærer ikke
//! alt et byrå trenger for å slutte å bruke det gamle systemet:
//! **kontaktopplysninger** og **åpne reskontroposter** står igjen.
//! Alle de norske systemene (Tripletex, Fiken, Visma eAccounting,
//! PowerOffice, Conta) kan eksportere begge deler som CSV/Excel i dag,
//! uten API-nøkler — så filtieren kommer først, akkurat som for bank
//! (docs/bank.md).
//!
//! Layouten leses av **kolonneoverskriftene**, ikke av en profil per
//! leverandør: overskriftsvokabularet endrer seg langsommere enn
//! produktnavnene, og en fil vi ikke forstår feiler høyt med
//! kolonnene vi faktisk så — aldri en stille halv import.
//!
//! To fortegnsregler er verdt å merke seg:
//! - Åpne poster leses som **utestående beløp**, med filens eget
//!   fortegn. En kreditnota i en kundeliste kommer negativ og forblir
//!   negativ.
//! - Hvilken vei posten peker i hovedboken bestemmes av parts-typen
//!   (kunde = debet, leverandør = kredit), ikke av filen: eksportene
//!   er uenige med hverandre om fortegn, men aldri om hvem som
//!   skylder hvem.

use chrono::NaiveDate;

use crate::csvutil::{find_column, find_column_ranked, read_header, split_record};

#[derive(Debug, PartialEq, Eq)]
pub enum MigreringError {
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

impl std::fmt::Display for MigreringError {
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

impl std::error::Error for MigreringError {}

/// Kunde eller leverandør — the same two kinds the reskontro knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Kunde,
    Leverandor,
}

impl PartKind {
    pub fn parse(raw: &str) -> Option<Self> {
        let n = raw.trim().to_lowercase();
        if n.starts_with("kunde") || n.starts_with("customer") || n == "debitor" {
            Some(Self::Kunde)
        } else if n.starts_with("lever") || n.starts_with("supplier") || n == "kreditor" {
            Some(Self::Leverandor)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kunde => "kunde",
            Self::Leverandor => "leverandor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KontaktRad {
    pub kind: PartKind,
    /// The old system's customer/supplier number, when the export has
    /// one — the most reliable key back to the source.
    pub nummer: Option<String>,
    pub navn: String,
    pub orgnr: Option<String>,
    pub epost: Option<String>,
    pub adresse: Option<String>,
    pub kontonummer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApenPostRad {
    /// Number if the export has one, otherwise the name — resolution
    /// against the party register happens in regnmed-db.
    pub part: String,
    pub part_navn: Option<String>,
    pub dokument: Option<String>,
    pub kid: Option<String>,
    pub dato: Option<NaiveDate>,
    pub forfall: Option<NaiveDate>,
    /// Outstanding amount with the file's own sign.
    pub belop_ore: i64,
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

/// Kontakter (kunder eller leverandører). `standard_kind` is what the
/// human said the file is; a `type`-column in the file wins per row —
/// some systems export one combined contact list.
pub fn parse_kontakter(
    text: &str,
    standard_kind: PartKind,
) -> Result<Vec<KontaktRad>, MigreringError> {
    let (header, lines) = read_header(text).ok_or(MigreringError::Empty)?;
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
    let konto_col = find_column_ranked(
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

    let navn_col = navn_col.ok_or_else(|| MigreringError::UnknownLayout(h.join(", ")))?;

    let mut rader = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_record(line, header.delimiter);
        let navn = cell(&fields, Some(navn_col));
        if navn.is_empty() {
            continue;
        }
        let kind = some_nonempty(cell(&fields, type_col))
            .and_then(|t| PartKind::parse(&t))
            .unwrap_or(standard_kind);
        rader.push(KontaktRad {
            kind,
            nummer: some_nonempty(cell(&fields, nummer_col)),
            navn,
            orgnr: some_nonempty(
                cell(&fields, orgnr_col)
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect(),
            ),
            epost: some_nonempty(cell(&fields, epost_col)),
            adresse: some_nonempty(cell(&fields, adresse_col)),
            kontonummer: some_nonempty(cell(&fields, konto_col)),
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
pub fn parse_apne_poster(text: &str) -> Result<Vec<ApenPostRad>, MigreringError> {
    let (header, lines) = read_header(text).ok_or(MigreringError::Empty)?;
    let h = &header.headers;

    let belop_col = find_column_ranked(
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
    let dato_col = find_column_ranked(
        h,
        &["fakturadato", "bilagsdato", "dato", "date"],
        &["forfall", "due", "betalt"],
    );
    let forfall_col = find_column_ranked(h, &["forfallsdato", "forfall", "due date", "due"], &[]);

    let belop_col = belop_col.ok_or_else(|| MigreringError::UnknownLayout(h.join(", ")))?;
    if part_col.is_none() && navn_col.is_none() {
        return Err(MigreringError::UnknownLayout(h.join(", ")));
    }

    let mut rader = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = i + 2; // header is line 1
        let fields = split_record(line, header.delimiter);
        let belop_raw = cell(&fields, Some(belop_col));
        if belop_raw.is_empty() {
            continue;
        }
        let belop_ore =
            crate::csvutil::parse_amount(&belop_raw).map_err(MigreringError::BadAmount)?;
        if belop_ore == 0 {
            // A settled item is not an open item.
            continue;
        }
        let nummer = some_nonempty(cell(&fields, part_col));
        let navn = some_nonempty(cell(&fields, navn_col));
        let part = nummer
            .clone()
            .or_else(|| navn.clone())
            .ok_or(MigreringError::MissingField {
                line: line_no,
                field: "kunde/leverandør",
            })?;
        let parse_day = |raw: String| -> Result<Option<NaiveDate>, MigreringError> {
            match some_nonempty(raw) {
                None => Ok(None),
                Some(value) => crate::csvutil::parse_date(&value)
                    .map(Some)
                    .map_err(MigreringError::BadDate),
            }
        };
        rader.push(ApenPostRad {
            part,
            part_navn: navn,
            dokument: some_nonempty(cell(&fields, dokument_col)),
            kid: some_nonempty(
                cell(&fields, kid_col)
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect(),
            ),
            dato: parse_day(cell(&fields, dato_col))?,
            forfall: parse_day(cell(&fields, forfall_col))?,
            belop_ore,
        });
    }
    Ok(rader)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tripletex_style_kundeliste() {
        // Semicolon, Norwegian headers, quoted name with the delimiter.
        let csv = "Kundenr;Navn;Organisasjonsnummer;E-post;Adresse;Kontonummer\n\
                   10001;\"Hansen; Berg AS\";915933149;post@hansen.no;Storgata 1;8601.11.17947\n\
                   10002;Lille Bakeri;;;;\n";
        let rader = parse_kontakter(csv, PartKind::Kunde).unwrap();
        assert_eq!(rader.len(), 2);
        assert_eq!(
            rader[0].navn, "Hansen; Berg AS",
            "sitat beskytter skilletegn"
        );
        assert_eq!(rader[0].nummer.as_deref(), Some("10001"));
        assert_eq!(rader[0].orgnr.as_deref(), Some("915933149"));
        assert_eq!(rader[0].kontonummer.as_deref(), Some("8601.11.17947"));
        assert_eq!(rader[0].kind, PartKind::Kunde);
        assert_eq!(rader[1].orgnr, None, "tomme felt blir None, ikke tomme");
    }

    #[test]
    fn engelsk_eksport_med_typekolonne_vinner_over_standardvalget() {
        let csv = "Number,Name,Type,Email\n\
                   S-1,Supplier AS,Supplier,post@supplier.no\n\
                   C-1,Customer AS,Customer,post@customer.no\n";
        let rader = parse_kontakter(csv, PartKind::Kunde).unwrap();
        assert_eq!(
            rader[0].kind,
            PartKind::Leverandor,
            "typekolonnen bestemmer"
        );
        assert_eq!(rader[1].kind, PartKind::Kunde);
        assert_eq!(rader[0].epost.as_deref(), Some("post@supplier.no"));
    }

    #[test]
    fn kontaktliste_uten_navnekolonne_feiler_hoyt() {
        let csv = "Kundenr;Saldo\n10001;500,00\n";
        let error = parse_kontakter(csv, PartKind::Kunde).unwrap_err();
        match error {
            MigreringError::UnknownLayout(headers) => {
                assert!(
                    headers.contains("kundenr"),
                    "feilen viser kolonnene: {headers}"
                );
            }
            other => panic!("forventet UnknownLayout, fikk {other:?}"),
        }
    }

    #[test]
    fn restbelop_vinner_over_fakturabelop() {
        // The trap: importing "Beløp" would overstate the reskontro.
        let csv = "Kundenr;Navn;Fakturanr;Fakturadato;Forfallsdato;Beløp;Restbeløp;KID\n\
                   10001;Hansen AS;F-100;15.01.2026;29.01.2026;12 500,00;2 500,00;1234567897\n";
        let rader = parse_apne_poster(csv).unwrap();
        assert_eq!(rader.len(), 1);
        assert_eq!(rader[0].belop_ore, 2_500_00, "utestående, ikke fakturasum");
        assert_eq!(rader[0].part, "10001");
        assert_eq!(rader[0].part_navn.as_deref(), Some("Hansen AS"));
        assert_eq!(rader[0].dokument.as_deref(), Some("F-100"));
        assert_eq!(rader[0].kid.as_deref(), Some("1234567897"));
        assert_eq!(
            rader[0].dato,
            Some(NaiveDate::from_ymd_opt(2026, 1, 15).unwrap())
        );
        assert_eq!(
            rader[0].forfall,
            Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap())
        );
    }

    #[test]
    fn kreditnota_beholder_sitt_negative_fortegn() {
        let csv = "Navn;Fakturanr;Restbeløp\n\
                   Hansen AS;F-101;-1 000,00\n\
                   Hansen AS;F-102;0,00\n";
        let rader = parse_apne_poster(csv).unwrap();
        assert_eq!(rader.len(), 1, "oppgjorte poster er ikke åpne poster");
        assert_eq!(rader[0].belop_ore, -1_000_00);
        assert_eq!(rader[0].part, "Hansen AS", "navn når nummer mangler");
    }

    #[test]
    fn apne_poster_uten_belopskolonne_feiler_hoyt() {
        let csv = "Kundenr;Navn;Fakturanr\n10001;Hansen AS;F-100\n";
        let error = parse_apne_poster(csv).unwrap_err();
        assert!(matches!(error, MigreringError::UnknownLayout(_)));
        assert!(error.to_string().contains("fakturanr"));
    }

    #[test]
    fn tabulator_og_iso_datoer_gaar_ogsaa() {
        let csv = "Leverandørnr\tNavn\tBilagsnr\tDato\tSaldo\n\
                   L-9\tGrossisten AS\tI-77\t2026-02-01\t-4500.50\n";
        let rader = parse_apne_poster(csv).unwrap();
        assert_eq!(rader[0].part, "L-9");
        assert_eq!(rader[0].belop_ore, -4_500_50);
        assert_eq!(
            rader[0].dato,
            Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap())
        );
    }

    #[test]
    fn forfallsdato_stjeler_ikke_fakturadatokolonnen() {
        let csv = "Navn;Forfallsdato;Fakturadato;Restbeløp\n\
                   Hansen AS;29.01.2026;15.01.2026;100,00\n";
        let rader = parse_apne_poster(csv).unwrap();
        assert_eq!(
            rader[0].dato,
            Some(NaiveDate::from_ymd_opt(2026, 1, 15).unwrap()),
            "fakturadato, selv om forfall står først"
        );
        assert_eq!(
            rader[0].forfall,
            Some(NaiveDate::from_ymd_opt(2026, 1, 29).unwrap())
        );
    }
}
