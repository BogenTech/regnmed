//! Payments to self-employed people without a fixed place of business —
//! Skatteetaten's third-party report «Betalinger til selvstendig
//! næringsdrivende», formerly RF-1321 (docs/selvstendig.md, #363).
//!
//! One delivery is the whole income year for one payer (oppgavegiver):
//! one `oppgave` per recipient with what was paid, and a summary that
//! must agree with them. The format is JSON, the `Melding` of
//! Skatteetaten's OpenAPI 1.0.0 (vendored in docs/selvstendig/), and the
//! rendered document is validated against that schema in the tests.
//!
//! Rendered by hand like the a-melding's JSON: the document is small, its
//! order is fixed, and the bytes stored before sending must be the bytes
//! sent (the idempotency key is only safe with the same body).
//!
//! Amounts are integer øre until [`kroner`]; the schema wants whole kroner.

use crate::{nin, orgnumber};

/// How the recipient is identified: fødselsnummer (or D-nummer) for a
/// person, organisasjonsnummer for an enterprise. The schema takes
/// exactly one of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipientId {
    Nin(String),
    Orgnumber(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub id: RecipientId,
    pub name: String,
    /// Paid in the income year, øre. Positive.
    pub paid_ore: i64,
}

/// `Kontaktinformasjon`: Skatteetaten asks for it when one sender files for
/// many payers, which is what a system user does. Every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contact {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub income_year: i32,
    pub payer_orgnumber: String,
    /// `oppgavegiversLeveranseReferanse`: unique per payer, year and form.
    pub reference: String,
    /// `kildesystem`, e.g. «regnmed v0.9.1».
    pub source_system: String,
    pub contact: Contact,
    /// Empty: the payer reports that it has nothing to report
    /// (`leveransetype` `ingenoppgaver`).
    pub recipients: Vec<Recipient>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReportError {
    InvalidOrgnumber(String),
    /// A recipient's fødselsnummer that fails the check digits.
    InvalidNin {
        recipient: String,
    },
    InvalidValue {
        field: &'static str,
        value: String,
    },
    /// Nothing paid (or a net refund): such a recipient has no oppgave.
    NotPositive {
        recipient: String,
        ore: i64,
    },
    /// The same person or enterprise twice: one oppgave per recipient.
    Duplicate {
        recipient: String,
    },
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOrgnumber(o) => write!(f, "«{o}» er ikke et gyldig organisasjonsnummer"),
            Self::InvalidNin { recipient } => {
                write!(f, "fødselsnummeret til «{recipient}» er ikke gyldig")
            }
            Self::InvalidValue { field, value } => {
                write!(f, "«{value}» kan ikke brukes som {field}")
            }
            Self::NotPositive { recipient, ore } => write!(
                f,
                "«{recipient}» har ikke fått noe betalt i året (netto {} øre)",
                ore
            ),
            Self::Duplicate { recipient } => write!(
                f,
                "«{recipient}» står to ganger: hver mottaker skal ha én oppgave"
            ),
        }
    }
}

impl std::error::Error for ReportError {}

/// Øre to whole kroner, half away from zero (50 øre rounds up). The
/// schema has no øre, and the amounts are payments that rarely are whole.
pub fn kroner(ore: i64) -> i64 {
    (ore + ore.signum() * 50) / 100
}

/// The `Melding` document, ready to send or to file by hand.
pub fn render(d: &Delivery) -> Result<String, ReportError> {
    if !orgnumber::is_valid(&d.payer_orgnumber) {
        return Err(ReportError::InvalidOrgnumber(d.payer_orgnumber.clone()));
    }
    if !(1000..=9999).contains(&d.income_year) {
        return Err(ReportError::InvalidValue {
            field: "inntektsår",
            value: d.income_year.to_string(),
        });
    }
    let bounded = |field: &'static str, value: &str, min: usize, max: usize| {
        let n = value.chars().count();
        if n < min || n > max {
            return Err(ReportError::InvalidValue {
                field,
                value: value.to_string(),
            });
        }
        Ok(())
    };
    bounded("leveransereferanse", &d.reference, 1, 100)?;
    bounded("kildesystem", &d.source_system, 1, 100)?;

    let mut out = String::from("{\"leveranse\":{");
    field(&mut out, "kildesystem", &d.source_system);
    out.push_str(&format!(",\"inntektsaar\":{}", d.income_year));
    out.push(',');
    field(&mut out, "oppgavegiversLeveranseReferanse", &d.reference);
    out.push_str(",\"oppgavegiver\":{");
    field(&mut out, "organisasjonsnummer", &d.payer_orgnumber);
    let c = &d.contact;
    let contact: Vec<(&str, &str, usize)> = [
        ("navn", &c.name, 200),
        ("telefonnummer", &c.phone, 20),
        ("varselEpostadresse", &c.email, 254),
    ]
    .into_iter()
    .filter_map(|(k, v, max)| v.as_deref().filter(|v| !v.is_empty()).map(|v| (k, v, max)))
    .collect();
    if !contact.is_empty() {
        out.push_str(",\"kontaktinformasjon\":{");
        for (i, (k, v, max)) in contact.iter().enumerate() {
            bounded("kontaktinformasjon", v, 1, *max)?;
            if i > 0 {
                out.push(',');
            }
            field(&mut out, k, v);
        }
        out.push('}');
    }
    out.push_str("},");
    let kind = if d.recipients.is_empty() {
        "ingenoppgaver"
    } else {
        "ordinaer"
    };
    field(&mut out, "leveransetype", kind);

    let mut seen: Vec<&RecipientId> = Vec::new();
    let mut sum = 0i64;
    if !d.recipients.is_empty() {
        out.push_str(",\"oppgave\":[");
        for (i, r) in d.recipients.iter().enumerate() {
            if r.paid_ore <= 0 {
                return Err(ReportError::NotPositive {
                    recipient: r.name.clone(),
                    ore: r.paid_ore,
                });
            }
            if seen.contains(&&r.id) {
                return Err(ReportError::Duplicate {
                    recipient: r.name.clone(),
                });
            }
            seen.push(&r.id);
            bounded("navn", &r.name, 1, 200)?;
            let amount = kroner(r.paid_ore);
            sum += amount;
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"oppgaveeier\":{");
            match &r.id {
                RecipientId::Nin(n) => {
                    if !nin::is_valid(n) {
                        return Err(ReportError::InvalidNin {
                            recipient: r.name.clone(),
                        });
                    }
                    field(&mut out, "foedselsnummer", n);
                }
                RecipientId::Orgnumber(o) => {
                    if !orgnumber::is_valid(o) {
                        return Err(ReportError::InvalidOrgnumber(o.clone()));
                    }
                    field(&mut out, "organisasjonsnummer", o);
                }
            }
            out.push(',');
            field(&mut out, "navn", &r.name);
            out.push_str(&format!("}},\"betaltBeloep\":{amount}}}"));
        }
        out.push(']');
    }
    if sum > 999_999_999_999 {
        return Err(ReportError::InvalidValue {
            field: "sumBetaltBeloep",
            value: sum.to_string(),
        });
    }
    out.push_str(&format!(
        ",\"oppgaveoppsummering\":{{\"antallOppgaver\":{},\"sumBetaltBeloep\":{sum}}}}}}}",
        d.recipients.len()
    ));
    Ok(out)
}

fn field(out: &mut String, key: &str, value: &str) {
    out.push('"');
    out.push_str(key);
    out.push_str("\":\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = include_str!(
        "../../../docs/selvstendig/innrapportering-betalinger-naeringsdrivende-api-1.0.0.json"
    );

    /// Validates against Skatteetaten's published schema (`Melding` in the
    /// vendored OpenAPI 1.0.0) with a real JSON Schema validator.
    fn validate(json: &str) -> Result<serde_json::Value, String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut schemas = boon::Schemas::new();
        let mut compiler = boon::Compiler::new();
        // OpenAPI 3.0's schema objects are draft-4 JSON Schema.
        compiler.set_default_draft(boon::Draft::V4);
        compiler
            .add_resource("urn:ske", serde_json::from_str(SCHEMA).unwrap())
            .unwrap();
        let id = compiler
            .compile("urn:ske#/components/schemas/Melding", &mut schemas)
            .unwrap();
        schemas.validate(&value, id).map_err(|e| format!("{e:#}"))?;
        Ok(value)
    }

    fn delivery() -> Delivery {
        Delivery {
            income_year: 2026,
            payer_orgnumber: "311032240".into(),
            reference: "0193b2a4-7c1e-7d3a-9e2f-3b1c5d7e9f01".into(),
            source_system: "regnmed test".into(),
            contact: Contact {
                name: Some("Kari \"Regnskap\" Nordmann".into()),
                phone: None,
                email: Some("post@example.no".into()),
            },
            recipients: vec![
                Recipient {
                    // Skatteetaten's own synthetic test number.
                    id: RecipientId::Nin("26829398612".into()),
                    name: "Ola Snekker".into(),
                    paid_ore: 7_500_049,
                },
                Recipient {
                    id: RecipientId::Orgnumber("974760673".into()),
                    name: "Bedriften A/S".into(),
                    paid_ore: 250_050,
                },
            ],
        }
    }

    #[test]
    fn the_report_is_valid_against_skatteetatens_schema() {
        let v = validate(&render(&delivery()).unwrap()).unwrap();
        let l = &v["leveranse"];
        assert_eq!(l["leveransetype"], "ordinaer");
        assert_eq!(
            l["oppgave"][0]["oppgaveeier"]["foedselsnummer"],
            "26829398612"
        );
        assert_eq!(l["oppgave"][0]["betaltBeloep"], 75_000);
        assert_eq!(
            l["oppgave"][1]["oppgaveeier"]["organisasjonsnummer"],
            "974760673"
        );
        assert_eq!(l["oppgave"][1]["betaltBeloep"], 2_501);
        // The summary is the sum of what is reported, after rounding.
        assert_eq!(
            l["oppgaveoppsummering"],
            serde_json::json!({"antallOppgaver": 2, "sumBetaltBeloep": 77_501})
        );
        assert_eq!(
            l["oppgavegiver"]["kontaktinformasjon"]["navn"],
            "Kari \"Regnskap\" Nordmann"
        );
    }

    #[test]
    fn nothing_paid_is_a_nil_report_valid_against_the_schema() {
        let mut d = delivery();
        d.recipients.clear();
        d.contact = Contact::default();
        let v = validate(&render(&d).unwrap()).unwrap();
        assert_eq!(v["leveranse"]["leveransetype"], "ingenoppgaver");
        assert!(v["leveranse"].get("oppgave").is_none());
        assert!(
            v["leveranse"]["oppgavegiver"]
                .get("kontaktinformasjon")
                .is_none()
        );
        assert_eq!(v["leveranse"]["oppgaveoppsummering"]["antallOppgaver"], 0);
    }

    /// The validator is not a rubber stamp: the schema's own constraints
    /// reject what Skatteetaten would.
    #[test]
    fn the_schema_check_rejects_what_skatteetaten_would() {
        let ok = render(&delivery()).unwrap();
        for (from, to) in [
            ("\"ordinaer\"", "\"vanlig\""),
            ("\"betaltBeloep\":75000", "\"betaltBeloep\":-1"),
            (
                "\"foedselsnummer\":\"26829398612\"",
                "\"foedselsnummer\":\"26829398612\",\"organisasjonsnummer\":\"974760673\"",
            ),
            ("\"inntektsaar\":2026,", ""),
        ] {
            assert!(ok.contains(from), "{from}");
            assert!(validate(&ok.replacen(from, to, 1)).is_err(), "{to}");
        }
    }

    #[test]
    fn what_the_schema_or_skatteetaten_would_refuse_is_refused_here() {
        let mut d = delivery();
        d.recipients[0].id = RecipientId::Nin("26829398613".into());
        assert_eq!(
            render(&d),
            Err(ReportError::InvalidNin {
                recipient: "Ola Snekker".into()
            })
        );
        let mut d = delivery();
        d.recipients[1].paid_ore = 0;
        assert!(matches!(render(&d), Err(ReportError::NotPositive { .. })));
        let mut d = delivery();
        d.recipients[1].id = d.recipients[0].id.clone();
        assert!(matches!(render(&d), Err(ReportError::Duplicate { .. })));
        let mut d = delivery();
        d.payer_orgnumber = "311032241".into();
        assert!(matches!(render(&d), Err(ReportError::InvalidOrgnumber(_))));
        let mut d = delivery();
        d.recipients[0].name = "x".repeat(201);
        assert!(matches!(render(&d), Err(ReportError::InvalidValue { .. })));
    }

    #[test]
    fn kroner_round_half_away_from_zero() {
        assert_eq!(kroner(149), 1);
        assert_eq!(kroner(150), 2);
        assert_eq!(kroner(100), 1);
        assert_eq!(kroner(49), 0);
        assert_eq!(kroner(-150), -2);
    }
}
