//! The revisor's verification report: every guarantee the system makes,
//! checked against the live ledger and stated in one document.
//!
//! This module holds the pure side of the report over data the
//! persistence layer assembles (`regnmed-db::audit`): a list of
//! checks (each with outcome and detail), the chain head, and the
//! external anchors with their witnesses. The text rendering is
//! deterministic — same input, same bytes — so a report can be archived,
//! diffed and re-generated. The reskontro tie-out (check 4) also
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
/// The text rendering indents every line under the check.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// One account as the reskontro tie-out sees it. `regnmed-db::audit`
/// assembles these with pure SUM queries; the reconciliation itself is
/// the function below, so it can be tested without a database.
#[derive(Debug, Clone)]
pub struct SubledgerAccount {
    pub account: String,
    /// The account's reskontro flag ('kunde'/'leverandor'). `None` means
    /// the account is not a reskontro account and is only in the list
    /// because postings on it carry a party anyway.
    pub flagg: Option<String>,
    /// SUM(amount_ore) over every entry on the account: the hovedbok saldo.
    pub general_ledger_ore: i64,
    /// SUM(amount_ore) over the entries that carry a party: what the
    /// subledger holds for this account.
    pub subledger_ore: i64,
    pub without_party_count: i64,
    pub with_party_count: i64,
    /// Postings whose party is of a different kind than the flag.
    pub wrong_kind_count: i64,
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

fn subledger_accounts(n: usize) -> String {
    if n == 1 {
        "1 reskontrokonto".into()
    } else {
        format!("{n} reskontrokontoer")
    }
}

/// Check 4: **Σ reskontro = hovedbokskonto, konto for konto.**
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
pub fn subledger_check(accounts: &[SubledgerAccount]) -> Check {
    let mut deviations: Vec<String> = Vec::new();
    let mut flagget = 0usize;
    let mut subledger_sum = 0i64;

    for account in accounts {
        let Some(flagg) = account.flagg.as_deref() else {
            // 3. Party postings outside every reskontro account.
            let kinds = account.part_kinds.join("/");
            deviations.push(format!(
                "konto {} er ikke merket som reskontrokonto, men {} bærer part ({}, {}) — \
                 beløpet står i spesifikasjonen uten å stå på en reskontrokonto i hovedboken",
                account.account,
                posteringer(account.with_party_count),
                Ore(account.subledger_ore),
                if kinds.is_empty() { "ukjent" } else { &kinds },
            ));
            continue;
        };
        flagget += 1;
        subledger_sum += account.subledger_ore;

        // 1. The tie-out itself.
        let difference = account.general_ledger_ore - account.subledger_ore;
        if difference != 0 {
            deviations.push(format!(
                "konto {}: hovedbok {} mot reskontro {} — differanse {} ({} øre), {} uten part",
                account.account,
                Ore(account.general_ledger_ore),
                Ore(account.subledger_ore),
                Ore(difference),
                difference,
                posteringer(account.without_party_count),
            ));
        } else if account.without_party_count > 0 {
            deviations.push(format!(
                "konto {}: {} uten part — de summerer til null, så saldoen stemmer, \
                 men beløpene står ikke i noen partsspesifikasjon",
                account.account,
                posteringer(account.without_party_count),
            ));
        }

        // 2. Parties of the wrong kind.
        if account.wrong_kind_count > 0 {
            let error: Vec<&str> = account
                .part_kinds
                .iter()
                .map(String::as_str)
                .filter(|kind| *kind != flagg)
                .collect();
            deviations.push(format!(
                "konto {} er merket {flagg}, men {} bærer part av typen {} ({})",
                account.account,
                posteringer(account.wrong_kind_count),
                if error.is_empty() {
                    "en annen".to_string()
                } else {
                    error.join("/")
                },
                Ore(account.feil_kind_ore),
            ));
        }
    }

    Check {
        name: "Reskontro mot hovedbok".into(),
        ok: deviations.is_empty(),
        detail: if deviations.is_empty() {
            format!(
                "{} avstemt mot hovedboken: summen av partenes poster er lik kontosaldoen \
                 øre for øre (til sammen {}); ingen part av feil type, og ingen part \
                 utenfor en reskontrokonto",
                subledger_accounts(flagget),
                Ore(subledger_sum),
            )
        } else {
            deviations.join("\n")
        },
    }
}

#[derive(Debug, Clone)]
pub struct AnchorInfo {
    /// RFC 3339 timestamp of the snapshot.
    pub timestamp: String,
    pub root_hex: String,
    pub last_sequence: i64,
    /// Human-readable witness descriptions ("rfc3161 https://…").
    pub vitner: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AuditInput {
    pub orgnumber: String,
    pub company: String,
    /// RFC 3339; passed in, never read from a clock here.
    pub generert: String,
    pub generated_by: String,
    pub programversjon: String,
    pub chain_sequence: i64,
    pub chain_head_hex: String,
    pub checks: Vec<Check>,
    pub anchors: Vec<AnchorInfo>,
}

impl AuditInput {
    pub fn alle_ok(&self) -> bool {
        self.checks.iter().all(|k| k.ok)
    }
}

/// Deterministic plain-text rendering, suitable for archiving next to
/// the regnskapsmateriale it attests.
pub fn render_text(input: &AuditInput) -> String {
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
        input.company, input.orgnumber
    ));
    line(&format!("Generert:       {}", input.generert));
    line(&format!("Generert av:    {}", input.generated_by));
    line(&format!("Programversjon: regnmed {}", input.programversjon));
    line(&format!(
        "Kjedehode:      sekvens {}, hash {}",
        input.chain_sequence, input.chain_head_hex
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
    for check in &input.checks {
        line(&format!(
            "[{}] {}",
            if check.ok { "OK" } else { "AVVIK" },
            check.name
        ));
        // A check may report several findings; each gets its own
        // indented line so the archived text stays readable.
        for detail in check.detail.lines() {
            line(&format!("       {detail}"));
        }
    }
    line("");
    line("Eksterne forankringer");
    line("---------------------");
    if input.anchors.is_empty() {
        line("(ingen forankringer omfatter dette selskapet ennå)");
    }
    for anchor in &input.anchors {
        line(&format!(
            "{}  sekvens {}  rot {}",
            anchor.timestamp, anchor.last_sequence, anchor.root_hex
        ));
        for vitne in &anchor.vitner {
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

    fn sample() -> AuditInput {
        AuditInput {
            orgnumber: "999888777".into(),
            company: "Demo AS".into(),
            generert: "2026-07-23T12:00:00+00:00".into(),
            generated_by: "Randi Revisor".into(),
            programversjon: "0.1.0".into(),
            chain_sequence: 13,
            chain_head_hex: "ab".repeat(32),
            checks: vec![
                Check {
                    name: "Hash-kjede fra genesis".into(),
                    ok: true,
                    detail: "13 bilag verifisert".into(),
                },
                Check {
                    name: "Reskontro mot hovedbok".into(),
                    ok: true,
                    detail: "1 reskontrokonto avstemt".into(),
                },
            ],
            anchors: vec![AnchorInfo {
                timestamp: "2026-07-23T02:00:00+00:00".into(),
                root_hex: "1c".repeat(32),
                last_sequence: 13,
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
    fn a_failed_check_flips_the_verdict() {
        let mut input = sample();
        input.checks[1].ok = false;
        input.checks[1].detail = "konto 1500 avviker med 100,00".into();
        assert!(!input.alle_ok());
        let text = render_text(&input);
        assert!(text.contains("AVVIK FUNNET"));
        assert!(text.contains("[AVVIK] Reskontro mot hovedbok"));
    }

    #[test]
    fn no_anchors_is_stated_not_hidden() {
        let mut input = sample();
        input.anchors.clear();
        assert!(render_text(&input).contains("ingen forankringer"));
    }

    #[test]
    fn every_finding_gets_its_own_line_in_the_text() {
        let mut input = sample();
        input.checks[1].ok = false;
        input.checks[1].detail = "konto 1500 avviker\nkonto 2400 avviker".into();
        let text = render_text(&input);
        assert!(text.contains("       konto 1500 avviker\n       konto 2400 avviker\n"));
    }

    /// A flagged account whose every posting carries a party.
    fn reconciled(account: &str, kind: &str, ore: i64, count: i64) -> SubledgerAccount {
        SubledgerAccount {
            account: account.into(),
            flagg: Some(kind.into()),
            general_ledger_ore: ore,
            subledger_ore: ore,
            without_party_count: 0,
            with_party_count: count,
            wrong_kind_count: 0,
            feil_kind_ore: 0,
            part_kinds: vec![kind.into()],
        }
    }

    #[test]
    fn a_reskontro_account_whose_parties_sum_to_the_balance_ties_out() {
        let check = subledger_check(&[reconciled("1500", "kunde", 12_500_00, 2)]);
        assert!(check.ok, "{}", check.detail);
        assert!(check.detail.contains("1 reskontrokonto avstemt"));
        assert!(check.detail.contains("12500,00"));
    }

    #[test]
    fn a_flagged_account_without_postings_ties_out() {
        // Zero is a saldo like any other: nothing on either side, no
        // finding. (Counting rows instead of amounts once made an
        // untouched account look like it held a party-less posting.)
        let check = subledger_check(&[reconciled("2400", "leverandor", 0, 0)]);
        assert!(check.ok, "{}", check.detail);
    }

    #[test]
    fn a_posting_without_a_party_breaks_the_tie_out_by_its_amount() {
        let mut account = reconciled("1500", "kunde", 12_500_00, 1);
        account.general_ledger_ore = 15_000_00; // 2 500,00 posted without a party
        account.without_party_count = 1;
        let check = subledger_check(&[account]);
        assert!(!check.ok);
        assert!(check.detail.contains("konto 1500"), "{}", check.detail);
        assert!(check.detail.contains("hovedbok 15000,00"));
        assert!(check.detail.contains("reskontro 12500,00"));
        assert!(check.detail.contains("differanse 2500,00 (250000 øre)"));
        assert!(check.detail.contains("1 postering uten part"));
    }

    #[test]
    fn party_less_postings_that_net_to_zero_are_reported_anyway() {
        // The saldo ties out, but two amounts are in no spesifikasjon —
        // reporting only the difference would let that pass silently.
        let mut account = reconciled("1500", "kunde", 12_500_00, 1);
        account.without_party_count = 2;
        let check = subledger_check(&[account]);
        assert!(!check.ok);
        assert!(
            check.detail.contains("2 posteringer uten part"),
            "{}",
            check.detail
        );
        assert!(check.detail.contains("summerer til null"));
    }

    #[test]
    fn a_party_of_the_wrong_kind_on_the_account_is_reported() {
        let mut account = reconciled("2400", "leverandor", -8_000_00, 2);
        account.wrong_kind_count = 1;
        account.feil_kind_ore = -3_000_00;
        account.part_kinds = vec!["kunde".into(), "leverandor".into()];
        let check = subledger_check(&[account]);
        assert!(!check.ok);
        assert!(
            check
                .detail
                .contains("konto 2400 er merket leverandor, men 1 postering bærer part av typen kunde (-3000,00)"),
            "{}",
            check.detail
        );
    }

    #[test]
    fn a_party_on_an_unflagged_account_is_reported() {
        let check = subledger_check(&[SubledgerAccount {
            account: "1500".into(),
            flagg: None,
            general_ledger_ore: 12_500_00,
            subledger_ore: 12_500_00,
            without_party_count: 0,
            with_party_count: 3,
            wrong_kind_count: 0,
            feil_kind_ore: 0,
            part_kinds: vec!["kunde".into()],
        }]);
        assert!(!check.ok);
        assert!(
            check
                .detail
                .contains("konto 1500 er ikke merket som reskontrokonto"),
            "{}",
            check.detail
        );
        assert!(check.detail.contains("3 posteringer bærer part"));
        assert!(check.detail.contains("(12500,00, kunde)"));
    }

    #[test]
    fn several_accounts_yield_one_finding_line_each() {
        let mut without_party = reconciled("1500", "kunde", 12_500_00, 1);
        without_party.general_ledger_ore = 15_000_00;
        without_party.without_party_count = 1;
        let mut ikke_flagget = reconciled("2400", "leverandor", -8_000_00, 1);
        ikke_flagget.flagg = None;
        let check = subledger_check(&[without_party, ikke_flagget]);
        assert!(!check.ok);
        assert_eq!(check.detail.lines().count(), 2, "{}", check.detail);
    }
}
