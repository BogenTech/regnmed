//! The revisor's verification report: every guarantee the system makes,
//! checked against the live ledger and stated in one document.
//!
//! This module holds the pure side of the report over data the
//! persistence layer assembles (`regnmed-db::revisjon`): a list of
//! kontroller (each with outcome and detail), the chain head, and the
//! external anchors with their witnesses. The text rendering is
//! deterministic — same input, same bytes — so a report can be archived,
//! diffed and re-generated. The reskontro tie-out (kontroll 4) also
//! reconciles here, over sums the database hands over, so its findings
//! can be tested without a database.
//!
//! The point of the document: a revisor should not have to *trust*
//! regnmed. The report states what was verified and how to re-verify it
//! independently (re-walk the chain, check the anchor root against the
//! public feed, verify RFC 3161 tokens offline — docs/anchoring.md).

use crate::Ore;

/// One verification: what was checked, whether it held, and the numbers.
///
/// `detalj` may hold several lines separated by `\n` — one per finding.
/// The text rendering indents every line under the kontroll.
#[derive(Debug, Clone)]
pub struct Kontroll {
    pub navn: String,
    pub ok: bool,
    pub detalj: String,
}

/// One account as the reskontro tie-out sees it. `regnmed-db::revisjon`
/// assembles these with pure SUM queries; the reconciliation itself is
/// the function below, so it can be tested without a database.
#[derive(Debug, Clone)]
pub struct ReskontroKonto {
    pub konto: String,
    /// The account's reskontro flag ('kunde'/'leverandor'). `None` means
    /// the account is not a reskontro account and is only in the list
    /// because postings on it carry a party anyway.
    pub flagg: Option<String>,
    /// SUM(amount_ore) over every entry on the account: the hovedbok saldo.
    pub hovedbok_ore: i64,
    /// SUM(amount_ore) over the entries that carry a party: what the
    /// subledger holds for this account.
    pub reskontro_ore: i64,
    pub uten_part_antall: i64,
    pub med_part_antall: i64,
    /// Postings whose party is of a different kind than the flag.
    pub feil_kind_antall: i64,
    pub feil_kind_ore: i64,
    /// Party kinds actually seen on the account, sorted and deduplicated.
    pub part_kinds: Vec<String>,
}

fn posteringer(n: i64) -> String {
    if n == 1 {
        "1 postering".into()
    } else {
        format!("{n} posteringer")
    }
}

fn reskontrokontoer(n: usize) -> String {
    if n == 1 {
        "1 reskontrokonto".into()
    } else {
        format!("{n} reskontrokontoer")
    }
}

/// Kontroll 4: **Σ reskontro = hovedbokskonto, konto for konto.**
///
/// The subledger is not a second bookkeeping — it is the same entries
/// seen per party — so the two can only diverge in three ways, and all
/// three are checked here:
///
/// 1. a posting on a reskontro account without a party: it is in the
///    account's saldo but in nobody's spesifikasjon (the difference is
///    reported in øre, and party-less postings that happen to net to
///    zero are reported too — the count is the defect, not the sum);
/// 2. a party of the wrong kind on the account: the amount lands in the
///    kunde- or leverandørspesifikasjon the account is not;
/// 3. a party on an account that is *not* flagged: the amount is in the
///    party's saldo while no reskontro account in the hovedbok holds it.
///    Flags are cleared during åpningsbalanse and SAF-T import
///    (docs/migration.md) and can be changed by hand afterwards, so this
///    state is reachable without anything in the ledger being edited.
///
/// Findings become AVVIK lines, never errors — the report reports.
pub fn reskontro_kontroll(kontoer: &[ReskontroKonto]) -> Kontroll {
    let mut avvik: Vec<String> = Vec::new();
    let mut flagget = 0usize;
    let mut sum_reskontro = 0i64;

    for konto in kontoer {
        let Some(flagg) = konto.flagg.as_deref() else {
            // 3. Party postings outside every reskontro account.
            let kinds = konto.part_kinds.join("/");
            avvik.push(format!(
                "konto {} er ikke merket som reskontrokonto, men {} bærer part ({}, {}) — \
                 beløpet står i spesifikasjonen uten å stå på en reskontrokonto i hovedboken",
                konto.konto,
                posteringer(konto.med_part_antall),
                Ore(konto.reskontro_ore),
                if kinds.is_empty() { "ukjent" } else { &kinds },
            ));
            continue;
        };
        flagget += 1;
        sum_reskontro += konto.reskontro_ore;

        // 1. The tie-out itself.
        let differanse = konto.hovedbok_ore - konto.reskontro_ore;
        if differanse != 0 {
            avvik.push(format!(
                "konto {}: hovedbok {} mot reskontro {} — differanse {} ({} øre), {} uten part",
                konto.konto,
                Ore(konto.hovedbok_ore),
                Ore(konto.reskontro_ore),
                Ore(differanse),
                differanse,
                posteringer(konto.uten_part_antall),
            ));
        } else if konto.uten_part_antall > 0 {
            avvik.push(format!(
                "konto {}: {} uten part — de summerer til null, så saldoen stemmer, \
                 men beløpene står ikke i noen partsspesifikasjon",
                konto.konto,
                posteringer(konto.uten_part_antall),
            ));
        }

        // 2. Parties of the wrong kind.
        if konto.feil_kind_antall > 0 {
            let feil: Vec<&str> = konto
                .part_kinds
                .iter()
                .map(String::as_str)
                .filter(|kind| *kind != flagg)
                .collect();
            avvik.push(format!(
                "konto {} er merket {flagg}, men {} bærer part av typen {} ({})",
                konto.konto,
                posteringer(konto.feil_kind_antall),
                if feil.is_empty() {
                    "en annen".to_string()
                } else {
                    feil.join("/")
                },
                Ore(konto.feil_kind_ore),
            ));
        }
    }

    Kontroll {
        navn: "Reskontro mot hovedbok".into(),
        ok: avvik.is_empty(),
        detalj: if avvik.is_empty() {
            format!(
                "{} avstemt mot hovedboken: summen av partenes poster er lik kontosaldoen \
                 øre for øre (til sammen {}); ingen part av feil type, og ingen part \
                 utenfor en reskontrokonto",
                reskontrokontoer(flagget),
                Ore(sum_reskontro),
            )
        } else {
            avvik.join("\n")
        },
    }
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
        // A kontroll may report several findings; each gets its own
        // indented line so the archived text stays readable.
        for detalj in kontroll.detalj.lines() {
            line(&format!("       {detalj}"));
        }
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
    line("4. Importert historikk: hash kildesystemets SAF-T-filer");
    line("   (shasum -a 256 <fil>) og sammenlign med importloggens");
    line("   hasher i kontrollen over.");
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

    #[test]
    fn every_finding_gets_its_own_line_in_the_text() {
        let mut input = sample();
        input.kontroller[1].ok = false;
        input.kontroller[1].detalj = "konto 1500 avviker\nkonto 2400 avviker".into();
        let text = render_text(&input);
        assert!(text.contains("       konto 1500 avviker\n       konto 2400 avviker\n"));
    }

    /// A flagged account whose every posting carries a party.
    fn avstemt(konto: &str, kind: &str, ore: i64, antall: i64) -> ReskontroKonto {
        ReskontroKonto {
            konto: konto.into(),
            flagg: Some(kind.into()),
            hovedbok_ore: ore,
            reskontro_ore: ore,
            uten_part_antall: 0,
            med_part_antall: antall,
            feil_kind_antall: 0,
            feil_kind_ore: 0,
            part_kinds: vec![kind.into()],
        }
    }

    #[test]
    fn a_reskontro_account_whose_parties_sum_to_the_saldo_ties_out() {
        let kontroll = reskontro_kontroll(&[avstemt("1500", "kunde", 12_500_00, 2)]);
        assert!(kontroll.ok, "{}", kontroll.detalj);
        assert!(kontroll.detalj.contains("1 reskontrokonto avstemt"));
        assert!(kontroll.detalj.contains("12500,00"));
    }

    #[test]
    fn a_flagged_account_without_postings_ties_out() {
        // Zero is a saldo like any other: nothing on either side, no
        // finding. (Counting rows instead of amounts once made an
        // untouched account look like it held a party-less posting.)
        let kontroll = reskontro_kontroll(&[avstemt("2400", "leverandor", 0, 0)]);
        assert!(kontroll.ok, "{}", kontroll.detalj);
    }

    #[test]
    fn a_posting_without_a_party_breaks_the_tie_out_by_its_amount() {
        let mut konto = avstemt("1500", "kunde", 12_500_00, 1);
        konto.hovedbok_ore = 15_000_00; // 2 500,00 posted without a party
        konto.uten_part_antall = 1;
        let kontroll = reskontro_kontroll(&[konto]);
        assert!(!kontroll.ok);
        assert!(
            kontroll.detalj.contains("konto 1500"),
            "{}",
            kontroll.detalj
        );
        assert!(kontroll.detalj.contains("hovedbok 15000,00"));
        assert!(kontroll.detalj.contains("reskontro 12500,00"));
        assert!(kontroll.detalj.contains("differanse 2500,00 (250000 øre)"));
        assert!(kontroll.detalj.contains("1 postering uten part"));
    }

    #[test]
    fn party_less_postings_that_net_to_zero_are_reported_anyway() {
        // The saldo ties out, but two amounts are in no spesifikasjon —
        // reporting only the difference would let that pass silently.
        let mut konto = avstemt("1500", "kunde", 12_500_00, 1);
        konto.uten_part_antall = 2;
        let kontroll = reskontro_kontroll(&[konto]);
        assert!(!kontroll.ok);
        assert!(
            kontroll.detalj.contains("2 posteringer uten part"),
            "{}",
            kontroll.detalj
        );
        assert!(kontroll.detalj.contains("summerer til null"));
    }

    #[test]
    fn a_party_of_the_wrong_kind_on_the_account_is_reported() {
        let mut konto = avstemt("2400", "leverandor", -8_000_00, 2);
        konto.feil_kind_antall = 1;
        konto.feil_kind_ore = -3_000_00;
        konto.part_kinds = vec!["kunde".into(), "leverandor".into()];
        let kontroll = reskontro_kontroll(&[konto]);
        assert!(!kontroll.ok);
        assert!(
            kontroll
                .detalj
                .contains("konto 2400 er merket leverandor, men 1 postering bærer part av typen kunde (-3000,00)"),
            "{}",
            kontroll.detalj
        );
    }

    #[test]
    fn a_party_on_an_unflagged_account_is_reported() {
        let kontroll = reskontro_kontroll(&[ReskontroKonto {
            konto: "1500".into(),
            flagg: None,
            hovedbok_ore: 12_500_00,
            reskontro_ore: 12_500_00,
            uten_part_antall: 0,
            med_part_antall: 3,
            feil_kind_antall: 0,
            feil_kind_ore: 0,
            part_kinds: vec!["kunde".into()],
        }]);
        assert!(!kontroll.ok);
        assert!(
            kontroll
                .detalj
                .contains("konto 1500 er ikke merket som reskontrokonto"),
            "{}",
            kontroll.detalj
        );
        assert!(kontroll.detalj.contains("3 posteringer bærer part"));
        assert!(kontroll.detalj.contains("(12500,00, kunde)"));
    }

    #[test]
    fn several_accounts_yield_one_finding_line_each() {
        let mut uten_part = avstemt("1500", "kunde", 12_500_00, 1);
        uten_part.hovedbok_ore = 15_000_00;
        uten_part.uten_part_antall = 1;
        let mut ikke_flagget = avstemt("2400", "leverandor", -8_000_00, 1);
        ikke_flagget.flagg = None;
        let kontroll = reskontro_kontroll(&[uten_part, ikke_flagget]);
        assert!(!kontroll.ok);
        assert_eq!(kontroll.detalj.lines().count(), 2, "{}", kontroll.detalj);
    }
}
