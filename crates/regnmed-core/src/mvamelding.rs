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
//! only merverdiavgift; code 0 is not reported at all.
//!
//! Reverse charge and import (#82) are TWO-SIDED where the code says the
//! deduction right is full — the buyer computes the tax under
//! mval. §11-1 (2) and deducts it under (3), so the purchase costs
//! nothing. `fastsatt_kr` therefore sums each line's net effect
//! (`mva_kr + fradrag_kr`), not just the computed side. Which codes
//! carry the deduction is not our judgement; it is stated per code in
//! Skatteetaten's own code list, quoted in `mva::direction`.

use crate::mva::{Direction, SpesLine, Termin, Terminordning, direction};
use crate::xml::Xml;

pub const NAMESPACE: &str =
    "no:skatteetaten:fastsetting:avgift:mva:skattemeldingformerverdiavgift:v1.0";

#[derive(Debug)]
pub struct MvaMelding {
    pub orgnr: String,
    pub termin: Termin,
    /// The company's terminordning (docs/mva.md, #51): decides which
    /// skattleggingsperiode element the XML carries.
    pub ordning: Terminordning,
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
    /// The code's own booked tax, melding signs.
    pub mva_kr: i64,
    /// The deduction the SAME code carries (reverse charge / import with
    /// full deduction right); 0 for every other code. Signed opposite
    /// `mva_kr`, so the pair sums to the line's net effect.
    pub fradrag_kr: i64,
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
    ordning: Terminordning,
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
        let retning = direction(&line.code);
        // Cost markers are not tax calculations, and using them is not
        // even mandatory — the import's tax is reported under 81/14.
        // Skipped like code 0 rather than emitted with a zeroed amount:
        // a line carrying a 25 % sats invites the recipient to compute
        // the very tax we already reported elsewhere.
        if retning == Direction::Kostnadsmarkor {
            continue;
        }
        let (grunnlag, sats) = match retning {
            // Fradrag lines carry only the deducted amount.
            Direction::Inngaende => (None, None),
            // Everything else reports grunnlag + sats.
            Direction::Utgaende
            | Direction::Ingen
            | Direction::Kostnadsmarkor
            | Direction::OmvendtMedFradrag
            | Direction::OmvendtUtenFradrag => (Some(grunnlag_kr), Some(line.rate_bp)),
        };
        lines.push(MeldingLine {
            code: line.code.clone(),
            description: line.description.clone(),
            grunnlag_kr: grunnlag,
            sats_bp: sats,
            mva_kr,
            // The deduction the same code carries. Kept beside the line
            // rather than folded into `mva_kr`, because the two are
            // different statements: `merverdiavgift` is the code's OWN
            // booked tax (the XSD calls it «Bokført beløp for
            // merverdiavgift»), while the fastsatte total is the net
            // effect on what is payable.
            fradrag_kr: match retning {
                Direction::OmvendtMedFradrag => -mva_kr,
                _ => 0,
            },
        });
    }
    // §11-1 (2)/(3): the buyer computing reverse-charge or import tax
    // ALSO deducts it when the deduction right is full. Summing only the
    // computed side billed every such customer 25 % of a basis they owed
    // nothing on.
    let fastsatt_kr = lines.iter().map(|l| l.mva_kr + l.fradrag_kr).sum();
    MvaMelding {
        orgnr: orgnr.to_string(),
        termin,
        ordning,
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
    match melding.ordning {
        Terminordning::ToManeder => x.leaf(
            "skattleggingsperiodeToMaaneder",
            periode_name(melding.termin),
        ),
        // Both yearly ordninger are skattleggingsperiodeAar in the
        // schema; primærnæring is a registration matter, not a
        // structural one (docs/mva.md).
        Terminordning::Arlig | Terminordning::Primaernaering => {
            x.leaf("skattleggingsperiodeAar", "aarlig")
        }
    }
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
            Terminordning::ToManeder,
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

    /// Reverse charge, computed by hand.
    ///
    /// A norwegian business buys a fjernleverbar tjeneste from abroad for
    /// 100 000 kr and has FULL deduction right (code 86). Under
    /// mval. §11-1 (2) it computes 25 000 kr output tax — and under (3)
    /// it deducts the same 25 000. Nothing is owed on the purchase, so
    /// the melding must fastsette 0, not 25 000.
    ///
    /// The same purchase WITHOUT deduction right is code 87, and there
    /// the 25 000 really is payable. The two cases differ by one
    /// character in the code, which is exactly why the old single-sided
    /// treatment was invisible: it produced a plausible number.
    #[test]
    fn reverse_charge_with_full_deduction_is_a_wash() {
        let kjop = |code: &str| {
            vec![SpesLine {
                code: code.into(),
                description: "Tjenester kjøpt fra utlandet".into(),
                rate_bp: 2500,
                // A purchase basis posts as a debit; the computed tax is
                // an output tax, i.e. a credit in the ledger.
                grunnlag_ore: 100_000_00,
                avgift_ore: -25_000_00,
            }]
        };
        let melding = |code: &str| {
            build(
                "999888777",
                Termin::new(2026, 1).unwrap(),
                Terminordning::ToManeder,
                "r",
                "0.1.0",
                &kjop(code),
            )
        };

        let med = melding("86");
        assert_eq!(med.lines[0].mva_kr, 25_000, "beregnet utgående avgift");
        assert_eq!(med.lines[0].fradrag_kr, -25_000, "og fradraget for den");
        assert_eq!(
            med.fastsatt_kr, 0,
            "full fradragsrett: kjøpet skal ikke koste avgift"
        );

        let uten = melding("87");
        assert_eq!(
            uten.lines[0].fradrag_kr, 0,
            "ingen fradragsrett, intet fradrag"
        );
        assert_eq!(
            uten.fastsatt_kr, 25_000,
            "uten fradragsrett skal avgiften betales i sin helhet"
        );

        // The cost markers calculate nothing: the tax on an import is
        // computed under 81/14, so letting 21 generate it too would
        // charge the same import twice.
        let markor = melding("21");
        assert!(markor.lines.is_empty(), "kode 21 er en kostnadsmarkør");
        assert_eq!(markor.fastsatt_kr, 0, "og beregner ingen avgift");
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

    /// Årstermin (and primærnæring) render skattleggingsperiodeAar —
    /// and the result stays schema-valid. Skips without xmllint.
    #[test]
    fn aarstermin_renders_skattleggingsperiode_aar() {
        let mut melding = melding();
        melding.ordning = Terminordning::Arlig;
        melding.termin = Termin {
            year: 2026,
            number: 1,
        };
        let xml = render(&melding);
        assert!(xml.contains("<skattleggingsperiodeAar>aarlig</skattleggingsperiodeAar>"));
        assert!(!xml.contains("skattleggingsperiodeToMaaneder"));

        let dir = std::env::temp_dir().join("regnmed-mvamelding-aar-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("melding.xml");
        std::fs::write(&file, &xml).unwrap();
        let xsd = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/mva-melding/skattemeldingformerverdiavgift.v1.0.xsd"
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
