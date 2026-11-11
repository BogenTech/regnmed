//! The a-melding (docs/lonn.md, #46) — rendered hand-rolled and
//! deterministically, like RF-1086, SAF-T and the mva-melding.
//!
//! One melding is one payroll run: the company is the opplysningspliktig,
//! each employee an inntektsmottaker with one ordinary arbeidsforhold, the
//! run's pay as inntekt, its withholding as forskuddstrekk, and the
//! arbeidsgiveravgift on the virksomhet. The output is validated against
//! Skatteetatens official XSD v2.3 (vendored in docs/amelding/) in tests
//! and in CI.
//!
//! The XSD alone is lax — every code is a free string and nearly every
//! arbeidsforhold field is optional — so the codes below come from
//! Skatteetatens kodeliste and inntektskombinasjoner (R1 2026), and the
//! fields the forretningsregler require (F291 for an ordinary
//! arbeidsforhold, F290 for the virksomhet) are refused when missing
//! rather than filled in. **Nothing here is a default someone did not
//! decide**: a melding with an invented yrkeskode or stillingsprosent is
//! accepted by Skatteetaten and wrong at NAV, which is worse than a
//! melding that is never sent.
//!
//! Amounts are integer øre throughout. The two fields the schema types as
//! whole kroner (forskuddstrekk and the sums to pay) are refused when the
//! øre part is not zero: the rounding rule belongs in the payroll
//! calculation that booked the amount, not in a renderer that would make
//! the melding disagree with the ledger.

use chrono::{NaiveDate, NaiveDateTime};

use crate::payroll::Zone;
use crate::xml::Xml;

pub const NAMESPACE: &str = "urn:ske:fastsetting:innsamling:a-meldingen:v2_3";

/// Arbeidstidsordning codes in the kodeliste (R1 2026).
const WORKING_HOURS_SCHEMES: &[&str] = &[
    "ikkeSkift",
    "andreSkift",
    "doegnkontinuerligSkiftOgTurnus355",
    "helkontinuerligSkiftOgAndreOrdninger336",
    "offshore336",
    "skift365",
];

/// FormForAnsettelse codes in the kodeliste (R1 2026).
const EMPLOYMENT_FORMS: &[&str] = &[
    "fast",
    "midlertidig",
    "fastAnsattUtleid",
    "midlertidigAnsattUtleid",
    "midlertidigAnsattSomTilkallingsvikar",
];

#[derive(Debug, PartialEq, Eq)]
pub enum AmeldingError {
    /// A field the forretningsregler require that regnmed does not hold.
    Missing {
        who: String,
        field: &'static str,
    },
    InvalidNin(String),
    InvalidOrgnumber(String),
    InvalidCode {
        field: &'static str,
        value: String,
    },
    /// The schema wants whole kroner and the booked amount has øre.
    NotWholeKroner {
        field: &'static str,
        ore: i64,
    },
    NoRecipients,
}

impl std::fmt::Display for AmeldingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { who, field } => write!(
                f,
                "a-meldingen krever «{field}» for {who}, og regnmed har ikke opplysningen \
                 — den gjettes ikke (se docs/lonn.md)"
            ),
            Self::InvalidNin(who) => write!(f, "fødselsnummeret til {who} er ugyldig"),
            Self::InvalidOrgnumber(n) => write!(f, "organisasjonsnummeret «{n}» er ugyldig"),
            Self::InvalidCode { field, value } => write!(
                f,
                "«{value}» er ikke en gyldig kode for «{field}» i Skatteetatens kodeliste"
            ),
            Self::NotWholeKroner { field, ore } => write!(
                f,
                "«{field}» skal leveres i hele kroner, men det bokførte beløpet er {ore} øre \
                 — regnmed runder ikke av i a-meldingen, da ville den sprike fra hovedboken \
                 (se docs/lonn.md)"
            ),
            Self::NoRecipients => write!(f, "lønnskjøringen har ingen inntektsmottakere"),
        }
    }
}

impl std::error::Error for AmeldingError {}

/// The loennsinntekt descriptions this renderer reports. All three are
/// combination 111 in the inntektskombinasjoner: kontantytelse, triggers
/// arbeidsgiveravgift, part of the trekkgrunnlag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomeKind {
    FixedSalary,
    HourlyWage,
    HolidayPay,
}

impl IncomeKind {
    fn code(self) -> &'static str {
        match self {
            Self::FixedSalary => "fastloenn",
            Self::HourlyWage => "timeloenn",
            Self::HolidayPay => "feriepenger",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Income {
    pub kind: IncomeKind,
    pub amount_ore: i64,
}

/// An ordinaertArbeidsforhold. The `Option` fields are what F291 requires
/// and the payroll model does not hold yet; `None` is refused.
#[derive(Debug, Clone)]
pub struct Employment {
    /// Stable per employee across meldinger (F108) — the employee id.
    pub id: String,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    /// Hours per week a full position equals, in hundredths (37,5 = 3750).
    pub full_time_hours_per_week_hundredths: Option<i64>,
    /// Seven-digit STYRK-08 code from SSB's yrkeskatalog (F115B).
    pub occupation_code: Option<String>,
    pub working_hours_scheme: Option<String>,
    /// Stillingsprosent in basis points (100 % = 10000).
    pub position_percent_bp: Option<i64>,
    pub last_salary_change: Option<NaiveDate>,
    pub employment_form: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Recipient {
    /// For error messages only; the melding identifies by fødselsnummer.
    pub name: String,
    /// The ONE place the fødselsnummer enters the melding.
    pub nin: String,
    pub employment: Employment,
    pub income: Vec<Income>,
    /// Withheld this month, positive øre as booked on 2600.
    pub withholding_ore: i64,
}

#[derive(Debug, Clone)]
pub struct Melding {
    /// Unique per delivery; the payroll run id.
    pub message_id: String,
    pub replaces_message_id: Option<String>,
    pub delivered_at: NaiveDateTime,
    pub year: i32,
    pub month: u32,
    pub source_system: String,
    /// The opplysningspliktig: the legal entity.
    pub orgnumber: String,
    /// The virksomhet: the underenhet in Enhetsregisteret the employees
    /// work at (F290). Not the legal entity's own number.
    pub business_orgnumber: Option<String>,
    pub payment_date: NaiveDate,
    pub zone: Zone,
    pub employer_contribution_rate_bp: i64,
    /// Arbeidsgiveravgift on the run's paid amounts (2770, not the
    /// accrual on holiday pay owed).
    pub employer_contribution_ore: i64,
    pub recipients: Vec<Recipient>,
}

fn zone_code(zone: Zone) -> &'static str {
    match zone {
        Zone::I => "1",
        Zone::Ia => "1a",
        Zone::II => "2",
        Zone::III => "3",
        Zone::IV => "4",
        Zone::IVa => "4a",
        Zone::V => "5",
    }
}

/// Øre as xsd:decimal, always two decimals so output is byte-stable.
fn amount(ore: i64) -> String {
    let sign = if ore < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", ore.abs() / 100, ore.abs() % 100)
}

fn whole_kroner(field: &'static str, ore: i64) -> Result<String, AmeldingError> {
    if ore % 100 != 0 {
        return Err(AmeldingError::NotWholeKroner { field, ore });
    }
    Ok((ore / 100).to_string())
}

fn required<'a, T>(
    value: &'a Option<T>,
    who: &str,
    field: &'static str,
) -> Result<&'a T, AmeldingError> {
    value.as_ref().ok_or_else(|| AmeldingError::Missing {
        who: who.to_string(),
        field,
    })
}

fn coded(field: &'static str, value: &str, codes: &[&str]) -> Result<(), AmeldingError> {
    if codes.contains(&value) {
        Ok(())
    } else {
        Err(AmeldingError::InvalidCode {
            field,
            value: value.to_string(),
        })
    }
}

pub fn render(m: &Melding) -> Result<String, AmeldingError> {
    for n in [Some(&m.orgnumber), m.business_orgnumber.as_ref()]
        .into_iter()
        .flatten()
    {
        if !crate::orgnumber::is_valid(n) {
            return Err(AmeldingError::InvalidOrgnumber(n.clone()));
        }
    }
    let business = required(
        &m.business_orgnumber,
        "virksomheten",
        "virksomhet (underenhet)",
    )?;
    if m.recipients.is_empty() {
        return Err(AmeldingError::NoRecipients);
    }

    let mut x = Xml::new();
    x.raw(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.raw(&format!(r#"<melding xmlns="{NAMESPACE}">"#));
    x.depth = 1;
    x.open("Leveranse");
    x.leaf(
        "leveringstidspunkt",
        &m.delivered_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
    );
    x.leaf("kalendermaaned", &format!("{:04}-{:02}", m.year, m.month));
    x.leaf("kildesystem", &m.source_system);
    if let Some(id) = &m.replaces_message_id {
        x.leaf("erstatterMeldingsId", id);
    }
    x.leaf("meldingsId", &m.message_id);
    x.open("opplysningspliktig");
    x.leaf("norskIdentifikator", &m.orgnumber);
    x.close("opplysningspliktig");

    x.open("oppgave");
    let withholding: i64 = m.recipients.iter().map(|r| r.withholding_ore).sum();
    let withholding_kr = whole_kroner("forskuddstrekk", withholding)?;
    x.open("betalingsinformasjon");
    // F523/F522: from 2026-01 the sum is reported per payment date.
    if m.year < 2026 {
        x.leaf("sumForskuddstrekk", &withholding_kr);
    }
    x.leaf(
        "sumArbeidsgiveravgift",
        &whole_kroner("arbeidsgiveravgift", m.employer_contribution_ore)?,
    );
    if m.year >= 2026 {
        x.open("sumForskuddstrekkPerLoennsutbetalingsdato");
        x.leaf("loennsutbetalingsdato", &m.payment_date.to_string());
        x.leaf("beloep", &withholding_kr);
        x.close("sumForskuddstrekkPerLoennsutbetalingsdato");
    }
    x.close("betalingsinformasjon");

    x.open("virksomhet");
    x.leaf("norskIdentifikator", business);
    let mut basis_ore = 0;
    for r in &m.recipients {
        recipient(&mut x, r)?;
        basis_ore += r.income.iter().map(|i| i.amount_ore).sum::<i64>();
    }
    // F184: every inntekt here triggers avgift, so the basis is their sum.
    x.open("arbeidsgiveravgift");
    x.open("loennOgGodtgjoerelse");
    x.leaf("beregningskodeForArbeidsgiveravgift", "generelleNaeringer");
    x.leaf("sone", zone_code(m.zone));
    x.leaf("avgiftsgrunnlagBeloep", &amount(basis_ore));
    x.leaf(
        "prosentsatsForAvgiftsberegning",
        &crate::vat_return::rate(m.employer_contribution_rate_bp),
    );
    x.close("loennOgGodtgjoerelse");
    x.close("arbeidsgiveravgift");
    x.close("virksomhet");
    x.close("oppgave");
    x.close("Leveranse");
    x.depth = 0;
    x.raw("</melding>");
    Ok(x.out)
}

fn recipient(x: &mut Xml, r: &Recipient) -> Result<(), AmeldingError> {
    if !crate::nin::is_valid(&r.nin) {
        return Err(AmeldingError::InvalidNin(r.name.clone()));
    }
    let e = &r.employment;
    let who = r.name.as_str();
    let hours = required(
        &e.full_time_hours_per_week_hundredths,
        who,
        "antall timer per uke for full stilling",
    )?;
    let occupation = required(&e.occupation_code, who, "yrkeskode")?;
    if occupation.len() != 7 || !occupation.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AmeldingError::InvalidCode {
            field: "yrkeskode",
            value: occupation.clone(),
        });
    }
    let scheme = required(&e.working_hours_scheme, who, "arbeidstidsordning")?;
    coded("arbeidstidsordning", scheme, WORKING_HOURS_SCHEMES)?;
    let percent = required(&e.position_percent_bp, who, "stillingsprosent")?;
    let last_change = required(&e.last_salary_change, who, "siste lønnsendringsdato")?;
    let form = required(&e.employment_form, who, "ansettelsesform")?;
    coded("ansettelsesform", form, EMPLOYMENT_FORMS)?;

    x.open("inntektsmottaker");
    x.leaf("norskIdentifikator", &r.nin);
    x.open("arbeidsforhold");
    x.leaf("arbeidsforholdId", &e.id);
    x.leaf("typeArbeidsforhold", "ordinaertArbeidsforhold");
    x.leaf("startdato", &e.start_date.to_string());
    if let Some(end) = e.end_date {
        x.leaf("sluttdato", &end.to_string());
    }
    x.leaf(
        "antallTimerPerUkeSomEnFullStillingTilsvarer",
        &amount(*hours),
    );
    x.leaf("yrke", occupation);
    x.leaf("arbeidstidsordning", scheme);
    x.leaf("stillingsprosent", &amount(*percent));
    x.leaf("sisteLoennsendringsdato", &last_change.to_string());
    x.leaf("formForAnsettelse", form);
    x.close("arbeidsforhold");

    if r.withholding_ore != 0 {
        x.open("forskuddstrekk");
        x.leaf("beskrivelse", "ordinaert");
        // F278-2: reported as a negative amount.
        x.leaf(
            "beloep",
            &whole_kroner("forskuddstrekk", -r.withholding_ore)?,
        );
        x.close("forskuddstrekk");
    }
    for i in r.income.iter().filter(|i| i.amount_ore != 0) {
        x.open("inntekt");
        x.leaf("fordel", "kontantytelse");
        x.leaf("utloeserArbeidsgiveravgift", "true");
        x.leaf("inngaarIGrunnlagForTrekk", "true");
        x.leaf("beloep", &amount(i.amount_ore));
        x.open("loennsinntekt");
        x.leaf("beskrivelse", i.kind.code());
        x.close("loennsinntekt");
        x.close("inntekt");
    }
    x.close("inntektsmottaker");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn employment() -> Employment {
        Employment {
            id: "3f1c2a9e-7b4d-4e8a-9c0f-1a2b3c4d5e6f".into(),
            start_date: d(2024, 8, 1),
            end_date: None,
            full_time_hours_per_week_hundredths: Some(3750),
            occupation_code: Some("2512101".into()),
            working_hours_scheme: Some("ikkeSkift".into()),
            position_percent_bp: Some(10_000),
            last_salary_change: Some(d(2026, 1, 1)),
            employment_form: Some("fast".into()),
        }
    }

    fn melding() -> Melding {
        Melding {
            message_id: "9b8f6c1e-0d2a-4f3b-8e7c-6a5b4c3d2e1f".into(),
            replaces_message_id: None,
            delivered_at: d(2026, 4, 2).and_hms_opt(10, 15, 0).unwrap(),
            year: 2026,
            month: 3,
            source_system: "regnmed".into(),
            orgnumber: "935115086".into(),
            business_orgnumber: Some("974760673".into()),
            payment_date: d(2026, 3, 25),
            zone: Zone::I,
            employer_contribution_rate_bp: 1410,
            // 14,1 % of 50 000 + 3 000 = 7 473 kr.
            employer_contribution_ore: 747_300,
            recipients: vec![Recipient {
                name: "Kari Nordmann".into(),
                // Synthetic Tenor number (month + 80), from nin.rs's own tests.
                nin: "26829398612".into(),
                employment: employment(),
                income: vec![
                    Income {
                        kind: IncomeKind::FixedSalary,
                        amount_ore: 5_000_000,
                    },
                    Income {
                        kind: IncomeKind::HolidayPay,
                        amount_ore: 300_000,
                    },
                ],
                withholding_ore: 1_500_000,
            }],
        }
    }

    /// Runs xmllint against the vendored XSD, skipping when it is absent.
    fn validate(xml: &str, tag: &str) {
        let xsd = format!(
            "{}/../../docs/amelding/a-meldingen_v2_3.xsd",
            env!("CARGO_MANIFEST_DIR")
        );
        let dir = std::env::temp_dir().join("regnmed-amelding-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{tag}.xml"));
        std::fs::write(&file, xml).unwrap();
        let output = match std::process::Command::new("xmllint")
            .args(["--noout", "--schema", &xsd])
            .arg(&file)
            .output()
        {
            Ok(output) => output,
            Err(_) => {
                eprintln!("xmllint not installed -- skipping schema validation");
                return;
            }
        };
        assert!(
            output.status.success(),
            "XSD validation failed for {tag}:\n{}\n\n{xml}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn a_monthly_melding_validates_against_the_official_xsd() {
        validate(&render(&melding()).unwrap(), "monthly");
    }

    #[test]
    fn before_2026_the_withholding_is_one_sum_and_still_validates() {
        let mut m = melding();
        m.year = 2025;
        m.recipients[0].employment.end_date = Some(d(2025, 3, 31));
        m.replaces_message_id = Some("earlier-id".into());
        let xml = render(&m).unwrap();
        assert!(xml.contains("<sumForskuddstrekk>15000</sumForskuddstrekk>"));
        assert!(!xml.contains("PerLoennsutbetalingsdato"));
        validate(&xml, "2025");
    }

    #[test]
    fn the_numbers_land_where_the_rules_put_them() {
        let xml = render(&melding()).unwrap();
        // F278-2: negative per recipient; F173-2: positive sum per date.
        assert!(xml.contains("<beloep>-15000</beloep>"));
        assert!(xml.contains("<loennsutbetalingsdato>2026-03-25</loennsutbetalingsdato>"));
        assert!(xml.contains("<beloep>15000</beloep>"));
        // F184: basis = all avgiftspliktige inntekter.
        assert!(xml.contains("<avgiftsgrunnlagBeloep>53000.00</avgiftsgrunnlagBeloep>"));
        assert!(
            xml.contains("<prosentsatsForAvgiftsberegning>14.1</prosentsatsForAvgiftsberegning>")
        );
        assert!(xml.contains("<sumArbeidsgiveravgift>7473</sumArbeidsgiveravgift>"));
        assert!(xml.contains("<stillingsprosent>100.00</stillingsprosent>"));
        assert!(xml.contains("<beskrivelse>feriepenger</beskrivelse>"));
    }

    #[test]
    fn every_field_the_model_lacks_is_refused_by_name() {
        type Clear = fn(&mut Employment);
        let cases: [(Clear, &str); 6] = [
            (
                |e| e.full_time_hours_per_week_hundredths = None,
                "antall timer per uke for full stilling",
            ),
            (|e| e.occupation_code = None, "yrkeskode"),
            (|e| e.working_hours_scheme = None, "arbeidstidsordning"),
            (|e| e.position_percent_bp = None, "stillingsprosent"),
            (|e| e.last_salary_change = None, "siste lønnsendringsdato"),
            (|e| e.employment_form = None, "ansettelsesform"),
        ];
        for (clear, field) in cases {
            let mut m = melding();
            clear(&mut m.recipients[0].employment);
            assert_eq!(
                render(&m),
                Err(AmeldingError::Missing {
                    who: "Kari Nordmann".into(),
                    field
                })
            );
        }
        let mut m = melding();
        m.business_orgnumber = None;
        assert!(matches!(
            render(&m),
            Err(AmeldingError::Missing {
                field: "virksomhet (underenhet)",
                ..
            })
        ));
    }

    #[test]
    fn ore_where_the_schema_wants_kroner_is_refused_not_rounded() {
        let mut m = melding();
        m.recipients[0].withholding_ore = 1_500_050;
        assert_eq!(
            render(&m),
            Err(AmeldingError::NotWholeKroner {
                field: "forskuddstrekk",
                ore: 1_500_050
            })
        );
        let mut m = melding();
        m.employer_contribution_ore = 747_301;
        assert!(matches!(
            render(&m),
            Err(AmeldingError::NotWholeKroner {
                field: "arbeidsgiveravgift",
                ..
            })
        ));
    }

    #[test]
    fn codes_outside_the_kodeliste_and_bad_identities_are_refused() {
        let mut m = melding();
        m.recipients[0].employment.working_hours_scheme = Some("turnus".into());
        assert!(matches!(render(&m), Err(AmeldingError::InvalidCode { .. })));
        let mut m = melding();
        m.recipients[0].employment.occupation_code = Some("2512".into());
        assert!(matches!(render(&m), Err(AmeldingError::InvalidCode { .. })));
        let mut m = melding();
        m.recipients[0].nin = "26829398613".into();
        assert!(matches!(render(&m), Err(AmeldingError::InvalidNin(_))));
        let mut m = melding();
        m.business_orgnumber = Some("974760674".into());
        assert!(matches!(
            render(&m),
            Err(AmeldingError::InvalidOrgnumber(_))
        ));
    }

    #[test]
    fn the_rendering_is_deterministic() {
        assert_eq!(render(&melding()), render(&melding()));
    }
}
