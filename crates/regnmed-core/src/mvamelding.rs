//! Mva-melding (skattemelding for merverdiavgift): builds the XML that
//! Skatteetaten's validation and submission APIs accept, from the same
//! spesifikasjon lines the mva-report shows.
//!
//! Format: `mvaMeldingDto` per Skatteetaten's published XSD (vendored in
//! `docs/mva-melding/`). Pure and deterministic, like the SAF-T renderer.
//!
//! Sign and unit conventions differ from the ledger and are converted
//! here, in one place:
//! - The ledger is signed øre, positive = debit (so utgående avgift is
//!   negative, deductible inngående positive).
//! - The melding is **whole kroner**, signed by effect on the amount
//!   payable: utgående positive, fradrag negative. Both grunnlag and
//!   avgift therefore negate on the way in, and øre round half away from
//!   zero to kroner.
//!
//! Per Skatteetaten's rules: utgående and omsetning codes report
//! grunnlag + sats + merverdiavgift; inngående (fradrag) codes report
//! only merverdiavgift; code 0 is not reported at all. Import /
//! reverse-charge codes are emitted with their beregnet side; the full
//! two-sided treatment is documented as a limitation until real
//! submissions begin.

use crate::mva::{Direction, SpesLine, Termin, direction};
use crate::xml::Xml;

pub const NAMESPACE: &str =
    "no:skatteetaten:fastsetting:avgift:mva:skattemeldingformerverdiavgift:v1.0";

#[derive(Debug)]
pub struct MvaMelding {
    pub orgnr: String,
    pub termin: Termin,
    /// Reference into our own system, echoed back in feedback
    /// (regnskapssystemsreferanse).
    pub referanse: String,
    pub system_version: String,
    pub lines: Vec<MeldingLine>,
    /// Sum of all line VAT amounts, melding signs, whole kroner.
    pub fastsatt_kr: i64,
}

#[derive(Debug)]
pub struct MeldingLine {
    pub code: String,
    pub description: String,
    pub grunnlag_kr: Option<i64>,
    pub sats_bp: Option<i64>,
    pub mva_kr: i64,
}

/// Whole kroner from øre, rounded half away from zero.
fn kroner(ore: i64) -> i64 {
    ((ore.unsigned_abs() + 50) / 100) as i64 * ore.signum()
}

/// Builds the melding from spesifikasjon lines (ledger signs, øre).
/// Code 0 has no place in the melding and is skipped.
pub fn build(
    orgnr: &str,
    termin: Termin,
    referanse: &str,
    system_version: &str,
    spes: &[SpesLine],
) -> MvaMelding {
    let mut lines = Vec::new();
    for line in spes {
        if line.code == "0" {
            continue;
        }
        // Ledger → melding: negate (payable-positive) and round to kroner.
        let mva_kr = kroner(-line.avgift_ore);
        let grunnlag_kr = kroner(-line.grunnlag_ore);
        let (grunnlag, sats) = match direction(&line.code) {
            // Fradrag lines carry only the deducted amount.
            Direction::Inngaende => (None, None),
            // Utgående and omsetning codes report grunnlag + sats.
            Direction::Utgaende | Direction::Ingen => (Some(grunnlag_kr), Some(line.rate_bp)),
            // Import/reverse-charge: beregnet side, grunnlag as observed.
            Direction::OmvendtAvgiftsplikt => (Some(grunnlag_kr), Some(line.rate_bp)),
        };
        lines.push(MeldingLine {
            code: line.code.clone(),
            description: line.description.clone(),
            grunnlag_kr: grunnlag,
            sats_bp: sats,
            mva_kr,
        });
    }
    let fastsatt_kr = lines.iter().map(|l| l.mva_kr).sum();
    MvaMelding {
        orgnr: orgnr.to_string(),
        termin,
        referanse: referanse.to_string(),
        system_version: system_version.to_string(),
        lines,
        fastsatt_kr,
    }
}

/// The XSD's two-month period names, indexed by termin number.
fn periode_name(termin: Termin) -> &'static str {
    match termin.number {
        1 => "januar-februar",
        2 => "mars-april",
        3 => "mai-juni",
        4 => "juli-august",
        5 => "september-oktober",
        _ => "november-desember",
    }
}

/// Sats as the kodeliste expects: "25", "11.11".
fn sats(bp: i64) -> String {
    match (bp / 100, bp % 100) {
        (whole, 0) => format!("{whole}"),
        (whole, frac) if frac % 10 == 0 => format!("{whole}.{}", frac / 10),
        (whole, frac) => format!("{whole}.{frac:02}"),
    }
}

pub fn render(melding: &MvaMelding) -> String {
    let mut x = Xml::new();
    x.raw(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    x.raw(&format!(r#"<mvaMeldingDto xmlns="{NAMESPACE}">"#));
    x.depth = 1;

    x.open("innsending");
    x.leaf("regnskapssystemsreferanse", &melding.referanse);
    x.open("regnskapssystem");
    x.leaf("systemnavn", "regnmed");
    x.leaf("systemversjon", &melding.system_version);
    x.close("regnskapssystem");
    x.close("innsending");

    x.open("skattegrunnlagOgBeregnetSkatt");
    x.open("skattleggingsperiode");
    x.open("periode");
    x.leaf(
        "skattleggingsperiodeToMaaneder",
        periode_name(melding.termin),
    );
    x.close("periode");
    x.leaf("aar", &melding.termin.year.to_string());
    x.close("skattleggingsperiode");
    x.leaf("fastsattMerverdiavgift", &melding.fastsatt_kr.to_string());
    for line in &melding.lines {
        x.open("mvaSpesifikasjonslinje");
        x.leaf("mvaKode", &line.code);
        x.leaf("mvaKodeRegnskapsystem", &line.description);
        if let Some(grunnlag) = line.grunnlag_kr {
            x.leaf("grunnlag", &grunnlag.to_string());
        }
        if let Some(bp) = line.sats_bp {
            x.leaf("sats", &sats(bp));
        }
        x.leaf("merverdiavgift", &line.mva_kr.to_string());
        x.close("mvaSpesifikasjonslinje");
    }
    x.close("skattegrunnlagOgBeregnetSkatt");

    x.empty("betalingsinformasjon");
    x.open("skattepliktig");
    x.leaf("organisasjonsnummer", &melding.orgnr);
    x.close("skattepliktig");
    x.leaf("meldingskategori", "alminnelig");

    x.depth = 0;
    x.raw("</mvaMeldingDto>");
    x.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spes() -> Vec<SpesLine> {
        vec![
            SpesLine {
                code: "1".into(),
                description: "Inngående mva, alminnelig sats".into(),
                rate_bp: 2500,
                grunnlag_ore: 8_000_00,
                avgift_ore: 2_000_00,
            },
            SpesLine {
                code: "3".into(),
                description: "Utgående mva, alminnelig sats".into(),
                rate_bp: 2500,
                grunnlag_ore: -10_000_49, // odd øre to prove rounding
                avgift_ore: -2_500_12,
            },
            SpesLine {
                code: "0".into(),
                description: "Ingen mva-behandling".into(),
                rate_bp: 0,
                grunnlag_ore: 5_000_00,
                avgift_ore: 0,
            },
        ]
    }

    fn melding() -> MvaMelding {
        build(
            "999888777",
            Termin::new(2026, 1).unwrap(),
            "regnmed-2026-1",
            "0.1.0",
            &spes(),
        )
    }

    #[test]
    fn converts_signs_units_and_skips_code_0() {
        let m = melding();
        assert_eq!(m.lines.len(), 2, "code 0 is not reported");

        let utg = m.lines.iter().find(|l| l.code == "3").unwrap();
        assert_eq!(
            utg.mva_kr, 2500,
            "utgående: ledger credit → payable positive, kroner"
        );
        assert_eq!(utg.grunnlag_kr, Some(10_000), "10000,49 rounds to 10000");
        assert_eq!(utg.sats_bp, Some(2500));

        let inn = m.lines.iter().find(|l| l.code == "1").unwrap();
        assert_eq!(inn.mva_kr, -2000, "fradrag is negative in the melding");
        assert_eq!(inn.grunnlag_kr, None, "inngående lines carry no grunnlag");
        assert_eq!(inn.sats_bp, None);

        assert_eq!(m.fastsatt_kr, 500, "fastsatt = sum of line effects");
    }

    #[test]
    fn kroner_rounds_half_away_from_zero() {
        assert_eq!(kroner(50), 1);
        assert_eq!(kroner(49), 0);
        assert_eq!(kroner(-50), -1);
        assert_eq!(kroner(-2_500_12), -2500);
    }

    #[test]
    fn renders_expected_structure() {
        let xml = render(&melding());
        for expected in [
            r#"<mvaMeldingDto xmlns="no:skatteetaten:fastsetting:avgift:mva:skattemeldingformerverdiavgift:v1.0">"#,
            "<regnskapssystemsreferanse>regnmed-2026-1</regnskapssystemsreferanse>",
            "<systemnavn>regnmed</systemnavn>",
            "<skattleggingsperiodeToMaaneder>januar-februar</skattleggingsperiodeToMaaneder>",
            "<aar>2026</aar>",
            "<fastsattMerverdiavgift>500</fastsattMerverdiavgift>",
            "<grunnlag>10000</grunnlag>",
            "<sats>25</sats>",
            "<merverdiavgift>2500</merverdiavgift>",
            "<merverdiavgift>-2000</merverdiavgift>",
            "<betalingsinformasjon/>",
            "<organisasjonsnummer>999888777</organisasjonsnummer>",
            "<meldingskategori>alminnelig</meldingskategori>",
        ] {
            assert!(xml.contains(expected), "missing {expected} in:\n{xml}");
        }
        assert_eq!(xml, render(&melding()), "deterministic");
    }

    /// Validates the rendered melding against Skatteetaten's official XSD.
    /// Skips when xmllint is unavailable.
    #[test]
    fn validates_against_official_xsd() {
        let xsd = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/mva-melding/skattemeldingformerverdiavgift.v1.0.xsd"
        );
        let dir = std::env::temp_dir().join("regnmed-mvamelding-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("melding.xml");
        std::fs::write(&file, render(&melding())).unwrap();

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
}
