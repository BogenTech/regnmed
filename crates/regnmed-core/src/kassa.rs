//! Kassaoppgjør (#89): the daily settlement from a kassasystem, as
//! bookkeeping — bokføringsforskriften §5-3 og §5-4.
//!
//! **regnmed is not a kassasystem.** Kassasystemlova puts the register
//! itself, with its produkterklæring, on its supplier. What belongs here
//! is the other end: turning the day's Z-report into one voucher with
//! the mva split, and doing it so the numbers can be checked against the
//! report afterwards.
//!
//! Pure and deterministic: the caller supplies the rate that applied on
//! the day (looked up in the dated table, like everywhere else), and
//! this module only splits and arranges.

use crate::Ore;
use crate::mva::split_gross;
use crate::voucher::{EntryDraft, VoucherDraft};

/// One line of the day's sales, as the register reports it: gross, on an
/// income account, at one VAT rate.
#[derive(Debug, Clone)]
pub struct Salgslinje {
    pub konto: String,
    pub vat_code: Option<String>,
    /// The rate that applied on the settlement date, basis points.
    pub rate_bp: i64,
    pub brutto_ore: i64,
}

/// What the day was paid with, as the register reports it: cash, bank,
/// or a clearing account for cards and Vipps awaiting settlement.
#[derive(Debug, Clone)]
pub struct Betalingslinje {
    pub konto: String,
    pub belop_ore: i64,
}

#[derive(Debug, Clone)]
pub struct Dagsoppgjor {
    pub dato: chrono::NaiveDate,
    /// The Z-report number. Goes in the voucher text so the bilag can be
    /// tied back to the register's own numbered report — that link is
    /// the point of §5-4, and a settlement without it documents nothing.
    pub z_nummer: String,
    pub salg: Vec<Salgslinje>,
    pub betaling: Vec<Betalingslinje>,
    pub mva_konto: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum KassaFeil {
    IngenSalg,
    TomtZNummer,
    /// The register says it sold one amount and took another. This is
    /// refused rather than balanced against a difference account: a
    /// mismatch INSIDE the Z-report is a broken report, not a till
    /// discrepancy, and papering over it would hide the one number the
    /// settlement exists to reconcile.
    Ubalanse {
        salg_ore: i64,
        betaling_ore: i64,
    },
}

impl std::fmt::Display for KassaFeil {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KassaFeil::IngenSalg => write!(f, "dagsoppgjøret har ingen salgslinjer"),
            KassaFeil::TomtZNummer => write!(
                f,
                "dagsoppgjøret må ha Z-nummer fra kassasystemet (bokføringsforskriften §5-4)"
            ),
            KassaFeil::Ubalanse {
                salg_ore,
                betaling_ore,
            } => write!(
                f,
                "kassaoppgjøret går ikke opp: salg {salg_ore} øre mot betalingsmidler \
                 {betaling_ore} øre. Differansen mellom TALT kasse og registrert salg \
                 føres som eget kassadifferansebilag — den skjules aldri her"
            ),
        }
    }
}

/// The day's settlement as one voucher: payment means debited, income
/// credited net per rate, mva credited as the sum of the parts.
///
/// The income lines keep their `vat_code`, so the mva-spesifikasjon sees
/// the day's sales the same way it sees an invoice's. The mva line
/// itself is uncoded, exactly as the invoice engine posts it — a code
/// there would count the same grunnlag twice.
pub fn bygg_oppgjor(oppgjor: &Dagsoppgjor) -> Result<VoucherDraft, KassaFeil> {
    if oppgjor.salg.is_empty() {
        return Err(KassaFeil::IngenSalg);
    }
    if oppgjor.z_nummer.trim().is_empty() {
        return Err(KassaFeil::TomtZNummer);
    }
    let salg_ore: i64 = oppgjor.salg.iter().map(|l| l.brutto_ore).sum();
    let betaling_ore: i64 = oppgjor.betaling.iter().map(|b| b.belop_ore).sum();
    if salg_ore != betaling_ore {
        return Err(KassaFeil::Ubalanse {
            salg_ore,
            betaling_ore,
        });
    }

    let mut entries = Vec::with_capacity(oppgjor.salg.len() + oppgjor.betaling.len() + 1);
    for b in &oppgjor.betaling {
        entries.push(EntryDraft {
            account_number: b.konto.clone(),
            amount: Ore(b.belop_ore),
            vat_code: None,
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    let mut mva_sum = 0;
    for l in &oppgjor.salg {
        let (netto, mva) = split_gross(l.brutto_ore, l.rate_bp);
        mva_sum += mva;
        entries.push(EntryDraft {
            account_number: l.konto.clone(),
            amount: Ore(-netto),
            vat_code: l.vat_code.clone(),
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    if mva_sum != 0 {
        entries.push(EntryDraft {
            account_number: oppgjor.mva_konto.clone(),
            amount: Ore(-mva_sum),
            vat_code: None,
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: None,
            valuta: None,
        });
    }
    Ok(VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: oppgjor.dato,
        description: format!("Kassaoppgjør Z-{}", oppgjor.z_nummer.trim()),
        reverses: None,
        entries,
    })
}

/// The kassadifferanse as its OWN voucher: counted cash against what the
/// register says should be in the till.
///
/// Its own bilag rather than a line on the settlement, because a
/// discrepancy is a finding about the day, not a rounding of it. A
/// shortfall credits the cash account and charges the difference
/// account; a surplus does the reverse. `None` when they agree — we do
/// not post a zero voucher to prove nothing happened.
pub fn bygg_differanse(
    dato: chrono::NaiveDate,
    z_nummer: &str,
    kontantkonto: &str,
    differansekonto: &str,
    registrert_kontant_ore: i64,
    opptalt_kontant_ore: i64,
) -> Option<VoucherDraft> {
    let differanse = opptalt_kontant_ore - registrert_kontant_ore;
    if differanse == 0 {
        return None;
    }
    let linje = |konto: &str, belop: i64| EntryDraft {
        account_number: konto.to_string(),
        amount: Ore(belop),
        vat_code: None,
        description: None,
        party_no: None,
        avdeling: None,
        prosjekt: None,
        valuta: None,
    };
    Some(VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: dato,
        description: format!("Kassadifferanse Z-{} ({} øre)", z_nummer.trim(), differanse),
        reverses: None,
        entries: vec![
            linje(kontantkonto, differanse),
            linje(differansekonto, -differanse),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dag() -> Dagsoppgjor {
        Dagsoppgjor {
            dato: chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            z_nummer: "0042".into(),
            salg: vec![
                Salgslinje {
                    konto: "3000".into(),
                    vat_code: Some("3".into()),
                    rate_bp: 2500,
                    brutto_ore: 12_500_00,
                },
                Salgslinje {
                    konto: "3010".into(),
                    vat_code: Some("31".into()),
                    rate_bp: 1500,
                    brutto_ore: 2_300_00,
                },
            ],
            betaling: vec![
                Betalingslinje {
                    konto: "1900".into(),
                    belop_ore: 4_800_00,
                },
                Betalingslinje {
                    konto: "1571".into(),
                    belop_ore: 10_000_00,
                },
            ],
            mva_konto: "2700".into(),
        }
    }

    #[test]
    fn the_settlement_balances_and_splits_mva_per_rate() {
        let v = bygg_oppgjor(&dag()).unwrap();
        assert_eq!(
            v.entries.iter().map(|e| e.amount.0).sum::<i64>(),
            0,
            "bilaget må gå i null"
        );
        assert!(v.description.contains("Z-0042"), "{}", v.description);

        // 12 500 incl. 25 % = 10 000 + 2 500; 2 300 incl. 15 % = 2 000 + 300.
        let konto = |nr: &str| {
            v.entries
                .iter()
                .find(|e| e.account_number == nr)
                .unwrap_or_else(|| panic!("mangler {nr}"))
                .amount
                .0
        };
        assert_eq!(konto("3000"), -10_000_00);
        assert_eq!(konto("3010"), -2_000_00);
        assert_eq!(
            konto("2700"),
            -2_800_00,
            "summen av delene, ikke av totalen"
        );
        assert_eq!(konto("1900"), 4_800_00);
        assert_eq!(konto("1571"), 10_000_00);

        // Income keeps its code so the mva-spesifikasjon sees the day's
        // sales; the mva line is uncoded like the invoice engine's.
        assert_eq!(
            v.entries
                .iter()
                .find(|e| e.account_number == "3000")
                .unwrap()
                .vat_code
                .as_deref(),
            Some("3")
        );
        assert!(
            v.entries
                .iter()
                .find(|e| e.account_number == "2700")
                .unwrap()
                .vat_code
                .is_none()
        );
    }

    /// A Z-report that does not add up is a broken report. Balancing it
    /// against the difference account would hide the very number the
    /// settlement exists to reconcile.
    #[test]
    fn a_z_report_that_does_not_add_up_is_refused_loudly() {
        let mut d = dag();
        d.betaling[0].belop_ore = 4_000_00;
        let feil = bygg_oppgjor(&d).unwrap_err();
        assert!(matches!(feil, KassaFeil::Ubalanse { .. }));
        assert!(feil.to_string().contains("kassadifferansebilag"), "{feil}");

        d.z_nummer = "  ".into();
        assert_eq!(bygg_oppgjor(&d).unwrap_err(), KassaFeil::TomtZNummer);
    }

    #[test]
    fn the_kassadifferanse_is_its_own_voucher_and_only_when_there_is_one() {
        let dato = chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap();
        assert!(
            bygg_differanse(dato, "0042", "1900", "7830", 4_800_00, 4_800_00).is_none(),
            "ingen differanse, intet bilag"
        );

        // 50 kr missing from the till.
        let v = bygg_differanse(dato, "0042", "1900", "7830", 4_800_00, 4_750_00).unwrap();
        assert_eq!(v.entries.iter().map(|e| e.amount.0).sum::<i64>(), 0);
        assert_eq!(v.entries[0].account_number, "1900");
        assert_eq!(v.entries[0].amount.0, -50_00, "kassen skrives ned");
        assert_eq!(v.entries[1].amount.0, 50_00, "differansen kostnadsføres");
        assert!(
            v.description.contains("Kassadifferanse"),
            "{}",
            v.description
        );

        // A surplus goes the other way — it is not "no discrepancy".
        let over = bygg_differanse(dato, "0042", "1900", "7830", 4_800_00, 4_820_00).unwrap();
        assert_eq!(over.entries[0].amount.0, 20_00);
    }
}
