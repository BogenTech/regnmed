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
//! The melding is built ONCE as a small typed tree ([`Node`]) and written
//! either as XML ([`render`], validated against the XSD) or as JSON
//! ([`render_json`], what Skatteetatens REST API takes —
//! `innrapportering-amelding-api` 0.1.2). The JSON is the same tree field
//! for field; only lists, whole-kroner numbers and booleans are typed
//! differently. One builder means the two formats cannot drift apart.
//!
//! Amounts are integer øre throughout. The two fields the schema types as
//! whole kroner (forskuddstrekk and the sums to pay) are refused when the
//! øre part is not zero: the rounding rule belongs in the payroll
//! calculation that booked the amount, not in a renderer that would make
//! the melding disagree with the ledger.

use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::payroll::Zone;
use crate::xml::{Xml, decimal, rate};

pub const NAMESPACE: &str = "urn:ske:fastsetting:innsamling:a-meldingen:v2_3";

/// Arbeidstidsordning codes in the kodeliste (R1 2026). Public so the
/// API validates input against the same list the renderer accepts.
pub const WORKING_HOURS_SCHEMES: &[&str] = &[
    "ikkeSkift",
    "andreSkift",
    "doegnkontinuerligSkiftOgTurnus355",
    "helkontinuerligSkiftOgAndreOrdninger336",
    "offshore336",
    "skift365",
];

/// FormForAnsettelse codes in the kodeliste (R1 2026).
pub const EMPLOYMENT_FORMS: &[&str] = &[
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
    /// A value the XSD or an avvisning rule constrains (identifier
    /// pattern, kildesystem length, kalendermåned range).
    InvalidValue {
        field: &'static str,
        value: String,
    },
    /// Forskuddstrekk and income are reported with fixed signs (F277,
    /// F278-2); a negative booked amount is a correction this renderer
    /// does not model.
    Negative {
        who: String,
        field: &'static str,
        ore: i64,
    },
    /// The booked arbeidsgiveravgift is not basis × sats (F185).
    ContributionMismatch {
        booked_ore: i64,
        computed_ore: i64,
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
            Self::InvalidValue { field, value } => {
                write!(
                    f,
                    "«{value}» er ikke en gyldig verdi for «{field}» i a-meldingen"
                )
            }
            Self::Negative { who, field, ore } => write!(
                f,
                "{field} for {who} er negativt ({ore} øre) — a-meldingen tar ikke \
                 korreksjoner som negative beløp, og regnmed modellerer dem ikke ennå"
            ),
            Self::ContributionMismatch {
                booked_ore,
                computed_ore,
            } => write!(
                f,
                "bokført arbeidsgiveravgift er {booked_ore} øre, men avgiftsgrunnlaget × \
                 satsen er {computed_ore} øre — Skatteetaten krever at de er like (F185), \
                 og regnmed sender ikke en melding som spriker fra hovedboken"
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
/// and an employee may not have filled in yet (migration 0073); `None` is
/// refused.
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

impl Employment {
    /// The F291 fields this employment lacks, named exactly as
    /// [`render`]'s `Missing` error names them — so the portal can say
    /// what to fill in before a melding is ever attempted.
    pub fn missing(&self) -> Vec<&'static str> {
        [
            (
                self.full_time_hours_per_week_hundredths.is_none(),
                "antall timer per uke for full stilling",
            ),
            (self.occupation_code.is_none(), "yrkeskode"),
            (self.working_hours_scheme.is_none(), "arbeidstidsordning"),
            (self.position_percent_bp.is_none(), "stillingsprosent"),
            (self.last_salary_change.is_none(), "siste lønnsendringsdato"),
            (self.employment_form.is_none(), "ansettelsesform"),
        ]
        .into_iter()
        .filter_map(|(absent, field)| absent.then_some(field))
        .collect()
    }
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
    pub delivered_at: DateTime<Utc>,
    pub source_system: String,
    /// The opplysningspliktig: the legal entity.
    pub orgnumber: String,
    /// The virksomhet: the underenhet in Enhetsregisteret the employees
    /// work at (F290). Not the legal entity's own number.
    pub business_orgnumber: Option<String>,
    /// The lønnsutbetalingsdato. The melding's kalendermåned is the
    /// month of THIS date, not the payroll month: the a-melding reports
    /// what was paid when it was paid.
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

/// The XSD's RestriksjonIdentifikator: `[0-9a-zA-Z_.-]*`, at most 150,
/// and F181E's minimum of one character.
fn identifier(field: &'static str, value: &str) -> Result<(), AmeldingError> {
    let ok = (1..=150).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(AmeldingError::InvalidValue {
            field,
            value: value.to_string(),
        })
    }
}

fn whole_kroner(field: &'static str, ore: i64) -> Result<i64, AmeldingError> {
    if ore % 100 != 0 {
        return Err(AmeldingError::NotWholeKroner { field, ore });
    }
    Ok(ore / 100)
}

/// A leaf's value. The XML writes all three as text; the JSON types them
/// as the OpenAPI schema does (`BeloepSomHeltall` is a number, `Boolsk` a
/// boolean, everything else — decimals included — a string).
enum Value {
    Text(String),
    Int(i64),
    Bool(bool),
}

enum Node {
    Leaf(&'static str, Value),
    Group(&'static str, Vec<Node>),
}

/// Builds the tree with the same open/leaf/close calls the XML writer had.
struct Tree {
    stack: Vec<(&'static str, Vec<Node>)>,
}

impl Tree {
    fn new(root: &'static str) -> Self {
        Tree {
            stack: vec![(root, Vec::new())],
        }
    }

    fn push(&mut self, node: Node) {
        self.stack.last_mut().expect("tree is open").1.push(node);
    }

    fn open(&mut self, name: &'static str) {
        self.stack.push((name, Vec::new()));
    }

    fn close(&mut self, name: &'static str) {
        let (open, children) = self.stack.pop().expect("tree is open");
        debug_assert_eq!(open, name, "mismatched close");
        self.push(Node::Group(open, children));
    }

    fn leaf(&mut self, name: &'static str, value: &str) {
        self.push(Node::Leaf(name, Value::Text(value.to_string())));
    }

    fn int(&mut self, name: &'static str, value: i64) {
        self.push(Node::Leaf(name, Value::Int(value)));
    }

    fn boolean(&mut self, name: &'static str, value: bool) {
        self.push(Node::Leaf(name, Value::Bool(value)));
    }

    fn finish(mut self) -> Node {
        let (root, children) = self.stack.pop().expect("tree is open");
        debug_assert!(self.stack.is_empty(), "unclosed group");
        Node::Group(root, children)
    }
}

fn write_xml(x: &mut Xml, node: &Node) {
    match node {
        Node::Leaf(name, Value::Text(t)) => x.leaf(name, t),
        Node::Leaf(name, Value::Int(i)) => x.leaf(name, &i.to_string()),
        Node::Leaf(name, Value::Bool(b)) => x.leaf(name, if *b { "true" } else { "false" }),
        Node::Group(name, children) => {
            x.open(name);
            for c in children {
                write_xml(x, c);
            }
            x.close(name);
        }
    }
}

/// The elements the OpenAPI schema types as arrays. In XML they are just
/// repeated; in JSON every occurrence goes into one list, even a single one.
const JSON_LISTS: &[&str] = &[
    "virksomhet",
    "inntektsmottaker",
    "arbeidsforhold",
    "inntekt",
    "forskuddstrekk",
    "loennOgGodtgjoerelse",
    "sumForskuddstrekkPerLoennsutbetalingsdato",
];

fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json_value(out: &mut String, node: &Node) {
    match node {
        Node::Leaf(_, Value::Text(t)) => json_string(out, t),
        Node::Leaf(_, Value::Int(i)) => out.push_str(&i.to_string()),
        Node::Leaf(_, Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
        Node::Group(_, children) => write_json_object(out, children),
    }
}

/// Keys in order of first occurrence; list elements gathered under one key.
fn write_json_object(out: &mut String, children: &[Node]) {
    let name = |n: &Node| match n {
        Node::Leaf(name, _) | Node::Group(name, _) => *name,
    };
    out.push('{');
    let mut seen: Vec<&str> = Vec::new();
    for child in children {
        let key = name(child);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if seen.len() > 1 {
            out.push(',');
        }
        json_string(out, key);
        out.push(':');
        if JSON_LISTS.contains(&key) {
            out.push('[');
            for (i, item) in children.iter().filter(|c| name(c) == key).enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_value(out, item);
            }
            out.push(']');
        } else {
            write_json_value(out, child);
        }
    }
    out.push('}');
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

/// The melding as XML, validated against the vendored XSD v2.3.
pub fn render(m: &Melding) -> Result<String, AmeldingError> {
    let leveranse = build(m)?;
    let mut x = Xml::new();
    x.raw(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.raw(&format!(r#"<melding xmlns="{NAMESPACE}">"#));
    x.depth = 1;
    write_xml(&mut x, &leveranse);
    x.depth = 0;
    x.raw("</melding>");
    Ok(x.out)
}

/// The melding as the JSON body of `POST /v1/innsending/{periode}/{opplysningspliktig}`.
/// Same validation, same tree as [`render`].
pub fn render_json(m: &Melding) -> Result<String, AmeldingError> {
    let Node::Group(_, children) = build(m)? else {
        unreachable!("build returns the Leveranse group")
    };
    let mut out = String::from("{\"leveranse\":");
    write_json_object(&mut out, &children);
    out.push('}');
    Ok(out)
}

/// The kalendermåned the melding reports: the month of the payment date.
pub fn period(m: &Melding) -> String {
    m.payment_date.format("%Y-%m").to_string()
}

fn build(m: &Melding) -> Result<Node, AmeldingError> {
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
    identifier("meldingsId", &m.message_id)?;
    if let Some(id) = &m.replaces_message_id {
        identifier("erstatterMeldingsId", id)?;
    }
    // F181F: at least one character; RestriksjonTekstfelt: at most 255.
    if !(1..=255).contains(&m.source_system.chars().count()) {
        return Err(AmeldingError::InvalidValue {
            field: "kildesystem",
            value: m.source_system.clone(),
        });
    }
    let (year, month) = (m.payment_date.year(), m.payment_date.month());
    // The XSD's own bounds on kalendermaaned.
    if !(2014..=2099).contains(&year) {
        return Err(AmeldingError::InvalidValue {
            field: "kalendermaaned",
            value: m.payment_date.to_string(),
        });
    }

    let mut x = Tree::new("Leveranse");
    x.leaf(
        "leveringstidspunkt",
        // With an explicit UTC designator: a zone-less xsd:dateTime would be
        // read as Norwegian local time.
        &m.delivered_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    );
    x.leaf("kalendermaaned", &format!("{year:04}-{month:02}"));
    x.leaf("kildesystem", &m.source_system);
    if let Some(id) = &m.replaces_message_id {
        x.leaf("erstatterMeldingsId", id);
    }
    x.leaf("meldingsId", &m.message_id);
    x.open("opplysningspliktig");
    x.leaf("norskIdentifikator", &m.orgnumber);
    x.close("opplysningspliktig");

    let basis_ore: i64 = m
        .recipients
        .iter()
        .flat_map(|r| &r.income)
        .map(|i| i.amount_ore)
        .sum();
    let computed_ore = crate::payroll::bp_of(basis_ore, m.employer_contribution_rate_bp);
    if computed_ore != m.employer_contribution_ore {
        return Err(AmeldingError::ContributionMismatch {
            booked_ore: m.employer_contribution_ore,
            computed_ore,
        });
    }

    x.open("oppgave");
    let withholding: i64 = m.recipients.iter().map(|r| r.withholding_ore).sum();
    let withholding_kr = whole_kroner("forskuddstrekk", withholding)?;
    x.open("betalingsinformasjon");
    // F523/F522: from 2026-01 the sum is reported per payment date.
    if year < 2026 {
        x.int("sumForskuddstrekk", withholding_kr);
    }
    x.int(
        "sumArbeidsgiveravgift",
        whole_kroner("arbeidsgiveravgift", m.employer_contribution_ore)?,
    );
    if year >= 2026 {
        x.open("sumForskuddstrekkPerLoennsutbetalingsdato");
        x.leaf("loennsutbetalingsdato", &m.payment_date.to_string());
        x.int("beloep", withholding_kr);
        x.close("sumForskuddstrekkPerLoennsutbetalingsdato");
    }
    x.close("betalingsinformasjon");

    x.open("virksomhet");
    x.leaf("norskIdentifikator", business);
    for r in &m.recipients {
        recipient(&mut x, r)?;
    }
    // F184: every inntekt here triggers avgift, so the basis is their sum.
    x.open("arbeidsgiveravgift");
    x.open("loennOgGodtgjoerelse");
    x.leaf("beregningskodeForArbeidsgiveravgift", "generelleNaeringer");
    x.leaf("sone", zone_code(m.zone));
    x.leaf("avgiftsgrunnlagBeloep", &decimal(basis_ore));
    x.leaf(
        "prosentsatsForAvgiftsberegning",
        &rate(m.employer_contribution_rate_bp),
    );
    x.close("loennOgGodtgjoerelse");
    x.close("arbeidsgiveravgift");
    x.close("virksomhet");
    x.close("oppgave");
    Ok(x.finish())
}

fn recipient(x: &mut Tree, r: &Recipient) -> Result<(), AmeldingError> {
    if !crate::nin::is_valid(&r.nin) {
        return Err(AmeldingError::InvalidNin(r.name.clone()));
    }
    let e = &r.employment;
    let who = r.name.as_str();
    let negative = |field, ore: i64| AmeldingError::Negative {
        who: who.to_string(),
        field,
        ore,
    };
    if r.withholding_ore < 0 {
        return Err(negative("forskuddstrekket", r.withholding_ore));
    }
    if let Some(i) = r.income.iter().find(|i| i.amount_ore < 0) {
        return Err(negative("inntekten", i.amount_ore));
    }
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
        &decimal(*hours),
    );
    x.leaf("yrke", occupation);
    x.leaf("arbeidstidsordning", scheme);
    x.leaf("stillingsprosent", &decimal(*percent));
    x.leaf("sisteLoennsendringsdato", &last_change.to_string());
    x.leaf("formForAnsettelse", form);
    x.close("arbeidsforhold");

    if r.withholding_ore != 0 {
        x.open("forskuddstrekk");
        x.leaf("beskrivelse", "ordinaert");
        // F278-2: reported as a negative amount.
        x.int(
            "beloep",
            whole_kroner("forskuddstrekk", -r.withholding_ore)?,
        );
        x.close("forskuddstrekk");
    }
    for i in r.income.iter().filter(|i| i.amount_ore != 0) {
        x.open("inntekt");
        x.leaf("fordel", "kontantytelse");
        x.boolean("utloeserArbeidsgiveravgift", true);
        x.boolean("inngaarIGrunnlagForTrekk", true);
        x.leaf("beloep", &decimal(i.amount_ore));
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
            delivered_at: d(2026, 4, 2).and_hms_opt(8, 15, 0).unwrap().and_utc(),
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
        m.payment_date = d(2025, 3, 25);
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
    fn the_json_is_the_same_tree_with_the_schemas_types() {
        let m = melding();
        let json: serde_json::Value = serde_json::from_str(&render_json(&m).unwrap()).unwrap();
        let l = &json["leveranse"];
        assert_eq!(l["kalendermaaned"], "2026-03");
        assert_eq!(l["kildesystem"], "regnmed");
        assert_eq!(l["opplysningspliktig"]["norskIdentifikator"], "935115086");
        let o = &l["oppgave"];
        // BeloepSomHeltall is a number; F523 per-date list from 2026.
        assert_eq!(o["betalingsinformasjon"]["sumArbeidsgiveravgift"], 7473);
        assert_eq!(
            o["betalingsinformasjon"]["sumForskuddstrekkPerLoennsutbetalingsdato"],
            serde_json::json!([{"loennsutbetalingsdato": "2026-03-25", "beloep": 15000}])
        );
        // The schema's arrays are arrays even with one element.
        let v = &o["virksomhet"][0];
        assert_eq!(v["norskIdentifikator"], "974760673");
        let r = &v["inntektsmottaker"][0];
        assert_eq!(r["norskIdentifikator"], "26829398612");
        assert_eq!(r["arbeidsforhold"][0]["stillingsprosent"], "100.00");
        assert_eq!(r["forskuddstrekk"][0]["beloep"], -15000);
        let inntekt = r["inntekt"].as_array().unwrap();
        assert_eq!(inntekt.len(), 2);
        // Decimals stay strings; Boolsk is a boolean.
        assert_eq!(inntekt[0]["beloep"], "50000.00");
        assert_eq!(inntekt[0]["utloeserArbeidsgiveravgift"], true);
        assert_eq!(inntekt[1]["loennsinntekt"]["beskrivelse"], "feriepenger");
        assert_eq!(
            v["arbeidsgiveravgift"]["loennOgGodtgjoerelse"][0]["avgiftsgrunnlagBeloep"],
            "53000.00"
        );
        assert_eq!(period(&m), "2026-03");
    }

    /// Both formats come from one tree, so every leaf value in the XML is
    /// in the JSON too — the guard against the two drifting apart.
    #[test]
    fn xml_and_json_carry_the_same_values() {
        let m = melding();
        let xml = render(&m).unwrap();
        let json = render_json(&m).unwrap();
        let leaves: Vec<&str> = xml
            .lines()
            .filter_map(|l| {
                l.split_once('>')
                    .and_then(|(_, rest)| rest.split_once("</"))
            })
            .map(|(value, _)| value)
            .filter(|v| !v.is_empty())
            .collect();
        assert!(leaves.len() > 20, "{leaves:?}");
        for v in leaves {
            assert!(json.contains(v), "{v} is in the XML but not the JSON");
        }
    }

    #[test]
    fn json_strings_are_escaped() {
        let mut m = melding();
        m.source_system = "regn\"med\\ \u{1}".into();
        let json: serde_json::Value = serde_json::from_str(&render_json(&m).unwrap()).unwrap();
        assert_eq!(json["leveranse"]["kildesystem"], "regn\"med\\ \u{1}");
    }

    #[test]
    fn json_is_refused_exactly_where_xml_is() {
        let mut m = melding();
        m.recipients[0].employment.occupation_code = None;
        assert_eq!(render_json(&m), render(&m));
    }

    #[test]
    fn a_complete_employment_misses_nothing() {
        assert!(melding().recipients[0].employment.missing().is_empty());
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
            // The portal's checklist names the same field the renderer
            // refuses on.
            assert_eq!(m.recipients[0].employment.missing(), vec![field]);
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
        // Basis 53 000,50 kr × 14,1 % = 7 473,07 kr: consistent, but øre.
        let mut m = melding();
        m.recipients[0].income[0].amount_ore = 5_000_050;
        m.employer_contribution_ore = 747_307;
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
    fn the_calendar_month_is_the_month_the_pay_was_paid() {
        // A March run paid on 2 April belongs in April's melding.
        let mut m = melding();
        m.payment_date = d(2026, 4, 2);
        let xml = render(&m).unwrap();
        assert!(xml.contains("<kalendermaaned>2026-04</kalendermaaned>"));
        // Paid on 31 December 2025: the old single sum, not the per-date one.
        m.payment_date = d(2025, 12, 31);
        let xml = render(&m).unwrap();
        assert!(xml.contains("<kalendermaaned>2025-12</kalendermaaned>"));
        assert!(xml.contains("<sumForskuddstrekk>"));
        validate(&xml, "paid-in-december");
    }

    #[test]
    fn the_delivery_time_carries_its_zone() {
        let xml = render(&melding()).unwrap();
        assert!(xml.contains("<leveringstidspunkt>2026-04-02T08:15:00Z</leveringstidspunkt>"));
    }

    #[test]
    fn a_booked_contribution_that_is_not_basis_times_rate_is_refused() {
        let mut m = melding();
        m.employer_contribution_ore = 747_400;
        assert_eq!(
            render(&m),
            Err(AmeldingError::ContributionMismatch {
                booked_ore: 747_400,
                computed_ore: 747_300
            })
        );
    }

    #[test]
    fn negative_withholding_and_income_are_refused() {
        let mut m = melding();
        m.recipients[0].withholding_ore = -1_500_000;
        assert!(matches!(render(&m), Err(AmeldingError::Negative { .. })));
        let mut m = melding();
        m.recipients[0].income[1].amount_ore = -300_000;
        m.employer_contribution_ore = 662_700;
        assert!(matches!(render(&m), Err(AmeldingError::Negative { .. })));
    }

    #[test]
    fn identifiers_and_kildesystem_outside_the_schema_are_refused() {
        let invalid = |m: Melding| matches!(render(&m), Err(AmeldingError::InvalidValue { .. }));
        let mut m = melding();
        m.message_id = "run 42".into();
        assert!(invalid(m));
        let mut m = melding();
        m.message_id = String::new();
        assert!(invalid(m));
        let mut m = melding();
        m.replaces_message_id = Some("førre".into());
        assert!(invalid(m));
        let mut m = melding();
        m.source_system = String::new();
        assert!(invalid(m));
        let mut m = melding();
        m.payment_date = d(2013, 12, 20);
        assert!(invalid(m));
    }

    #[test]
    fn a_frikort_recipient_next_to_an_ordinary_one_validates() {
        let mut m = melding();
        let mut second = m.recipients[0].clone();
        second.name = "Ola Nordmann".into();
        second.nin = "08888797336".into();
        second.employment.id = "employee-2".into();
        second.income = vec![Income {
            kind: IncomeKind::HourlyWage,
            amount_ore: 1_000_000,
        }];
        second.withholding_ore = 0;
        m.recipients.push(second);
        // 63 000 kr × 14,1 % = 8 883 kr.
        m.employer_contribution_ore = 888_300;
        let xml = render(&m).unwrap();
        assert_eq!(xml.matches("<inntektsmottaker>").count(), 2);
        // Only the recipient with trekk carries a forskuddstrekk element.
        assert_eq!(xml.matches("<forskuddstrekk>").count(), 1);
        assert!(xml.contains("<avgiftsgrunnlagBeloep>63000.00</avgiftsgrunnlagBeloep>"));
        assert!(xml.contains("<sumArbeidsgiveravgift>8883</sumArbeidsgiveravgift>"));
        assert!(xml.contains("<beskrivelse>timeloenn</beskrivelse>"));
        validate(&xml, "two-recipients");
    }

    #[test]
    fn the_rendering_is_deterministic() {
        assert_eq!(render(&melding()), render(&melding()));
    }
}
