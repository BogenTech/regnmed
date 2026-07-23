//! The revisor's verification report: every guarantee the system makes,
//! checked against the live ledger and stated in one document.
//!
//! This module is pure presentation over data the persistence layer
//! assembles (`regnmed-db::revisjon`): a list of kontroller (each with
//! outcome and detail), the chain head, and the external anchors with
//! their witnesses. The text rendering is deterministic — same input,
//! same bytes — so a report can be archived, diffed and re-generated.
//!
//! The point of the document: a revisor should not have to *trust*
//! regnmed. The report states what was verified and how to re-verify it
//! independently (re-walk the chain, check the anchor root against the
//! public feed, verify RFC 3161 tokens offline — docs/anchoring.md).

/// One verification: what was checked, whether it held, and the numbers.
#[derive(Debug, Clone)]
pub struct Kontroll {
    pub navn: String,
    pub ok: bool,
    pub detalj: String,
}

#[derive(Debug, Clone)]
pub struct AnkerInfo {
    /// RFC 3339 timestamp of the snapshot.
    pub tidspunkt: String,
    pub root_hex: String,
    pub siste_sekvens: i64,
    /// Human-readable witness descriptions ("rfc3161 https://…").
    pub vitner: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RevisjonInput {
    pub orgnr: String,
    pub selskap: String,
    /// RFC 3339; passed in, never read from a clock here.
    pub generert: String,
    pub generert_av: String,
    pub programversjon: String,
    pub kjede_sekvens: i64,
    pub kjede_hode_hex: String,
    pub kontroller: Vec<Kontroll>,
    pub ankere: Vec<AnkerInfo>,
}

impl RevisjonInput {
    pub fn alle_ok(&self) -> bool {
        self.kontroller.iter().all(|k| k.ok)
    }
}

/// Deterministic plain-text rendering, suitable for archiving next to
/// the regnskapsmateriale it attests.
pub fn render_text(input: &RevisjonInput) -> String {
    let mut out = String::with_capacity(2048);
    let mut line = |s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    line("VERIFIKASJONSRAPPORT FOR HOVEDBOK");
    line("=================================");
    line("");
    line(&format!(
        "Selskap:        {} ({})",
        input.selskap, input.orgnr
    ));
    line(&format!("Generert:       {}", input.generert));
    line(&format!("Generert av:    {}", input.generert_av));
    line(&format!("Programversjon: regnmed {}", input.programversjon));
    line(&format!(
        "Kjedehode:      sekvens {}, hash {}",
        input.kjede_sekvens, input.kjede_hode_hex
    ));
    line("");
    line(&format!(
        "Samlet resultat: {}",
        if input.alle_ok() {
            "ALLE KONTROLLER OK"
        } else {
            "AVVIK FUNNET — SE KONTROLLENE UNDER"
        }
    ));
    line("");
    line("Kontroller");
    line("----------");
    for kontroll in &input.kontroller {
        line(&format!(
            "[{}] {}",
            if kontroll.ok { "OK" } else { "AVVIK" },
            kontroll.navn
        ));
        line(&format!("       {}", kontroll.detalj));
    }
    line("");
    line("Eksterne forankringer");
    line("---------------------");
    if input.ankere.is_empty() {
        line("(ingen forankringer omfatter dette selskapet ennå)");
    }
    for anker in &input.ankere {
        line(&format!(
            "{}  sekvens {}  rot {}",
            anker.tidspunkt, anker.siste_sekvens, anker.root_hex
        ));
        for vitne in &anker.vitner {
            line(&format!("    bevitnet: {vitne}"));
        }
    }
    line("");
    line("Slik etterprøver du uavhengig av regnmed:");
    line("1. Hash-kjeden: hent bilagene og beregn hashene på nytt fra");
    line("   genesis (formatet er dokumentert i docs/ledger.md).");
    line("2. Forankringen: sammenlign rothashene over med den offentlige");
    line("   /anchors-strømmen og egne kopier av røttene.");
    line("3. RFC 3161-vitner verifiseres frakoblet med openssl ts");
    line("   (docs/anchoring.md).");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RevisjonInput {
        RevisjonInput {
            orgnr: "999888777".into(),
            selskap: "Demo AS".into(),
            generert: "2026-07-23T12:00:00+00:00".into(),
            generert_av: "Randi Revisor".into(),
            programversjon: "0.1.0".into(),
            kjede_sekvens: 13,
            kjede_hode_hex: "ab".repeat(32),
            kontroller: vec![
                Kontroll {
                    navn: "Hash-kjede fra genesis".into(),
                    ok: true,
                    detalj: "13 bilag verifisert".into(),
                },
                Kontroll {
                    navn: "Reskontro mot hovedbok".into(),
                    ok: true,
                    detalj: "1 reskontrokonto avstemt".into(),
                },
            ],
            ankere: vec![AnkerInfo {
                tidspunkt: "2026-07-23T02:00:00+00:00".into(),
                root_hex: "1c".repeat(32),
                siste_sekvens: 13,
                vitner: vec!["rfc3161 https://freetsa.org/tsr".into()],
            }],
        }
    }

    #[test]
    fn rendering_is_deterministic_and_states_the_verdict() {
        let a = render_text(&sample());
        assert_eq!(a, render_text(&sample()));
        assert!(a.contains("ALLE KONTROLLER OK"));
        assert!(a.contains("[OK] Hash-kjede fra genesis"));
        assert!(a.contains("bevitnet: rfc3161 https://freetsa.org/tsr"));
        assert!(a.contains("sekvens 13"));
    }

    #[test]
    fn a_failed_kontroll_flips_the_verdict() {
        let mut input = sample();
        input.kontroller[1].ok = false;
        input.kontroller[1].detalj = "konto 1500 avviker med 100,00".into();
        assert!(!input.alle_ok());
        let text = render_text(&input);
        assert!(text.contains("AVVIK FUNNET"));
        assert!(text.contains("[AVVIK] Reskontro mot hovedbok"));
    }

    #[test]
    fn no_anchors_is_stated_not_hidden() {
        let mut input = sample();
        input.ankere.clear();
        assert!(render_text(&input).contains("ingen forankringer"));
    }
}
