//! Fixed asset register, pure side (docs/anlegg.md, #40): straight-line
//! avskrivninger for the accounts and saldo depreciation per gruppe for
//! the skattemelding — both as pure functions over register data, all in
//! integer øre.
//!
//! For the accounts: the depreciable amount (kostpris − residual value)
//! is spread across the useful life in fixed monthly amounts; the last
//! month takes the remainder, so the plan sums EXACTLY to the depreciable
//! amount.
//!
//! For tax: the saldo method (skatteloven §14-40 flg.) — the grunnlag is
//! opening saldo + additions − proceeds; the year's depreciation is
//! grunnlag × the gruppe's rate when the grunnlag is positive. A negative
//! saldo (proceeds above the saldo) is not depreciated — it is reported
//! as a candidate for income recognition and handled by the
//! regnskapsfører (deliberately out of scope, see docs/anlegg.md).

/// The saldogrupper in skatteloven §14-41 with descriptions — the rates
/// themselves are regelverk data in the satsregister (domain
/// `saldogruppe_<letter>`), never hardcoded here.
pub const SALDOGRUPPER: &[(&str, &str)] = &[
    ("a", "Kontormaskiner o.l."),
    ("b", "Ervervet forretningsverdi"),
    ("c", "Vogntog, lastebiler, busser, varebiler mv."),
    ("d", "Personbiler, maskiner, inventar mv."),
    ("e", "Skip, fartøyer, rigger mv."),
    ("f", "Fly, helikopter"),
    (
        "g",
        "Anlegg for overføring og distribusjon av elektrisk kraft mv.",
    ),
    ("h", "Bygg og anlegg, hoteller mv."),
    ("i", "Forretningsbygg"),
    ("j", "Fast teknisk installasjon i bygninger"),
];

pub fn gyldig_saldogruppe(gruppe: &str) -> bool {
    SALDOGRUPPER.iter().any(|(g, _)| *g == gruppe)
}

/// The monthly amount for month `maned_nr` (1-based) in a straight-line
/// plan. Every month but the last gets the rounded base amount; the last
/// takes the remainder, so the sum across the life is exactly
/// `kostpris - restverdi`.
pub fn manedsbelop(
    kostpris_ore: i64,
    restverdi_ore: i64,
    levetid_maneder: i32,
    maned_nr: i32,
) -> i64 {
    debug_assert!(levetid_maneder > 0 && maned_nr >= 1 && maned_nr <= levetid_maneder);
    let avskrivbart = kostpris_ore - restverdi_ore;
    let basis = avskrivbart / levetid_maneder as i64;
    if maned_nr < levetid_maneder {
        basis
    } else {
        avskrivbart - basis * (levetid_maneder as i64 - 1)
    }
}

/// One saldo year: grunnlag, the year's depreciation and closing saldo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaldoAr {
    pub grunnlag_ore: i64,
    pub avskrivning_ore: i64,
    pub utgaende_ore: i64,
}

/// The saldo method for one year. A negative grunnlag is never
/// depreciated — it is passed on as closing saldo (a candidate for
/// income recognition).
pub fn saldo_ar(inngaende_ore: i64, tilgang_ore: i64, vederlag_ore: i64, sats_bp: i64) -> SaldoAr {
    let grunnlag = inngaende_ore + tilgang_ore - vederlag_ore;
    let avskrivning = if grunnlag > 0 {
        // Half away from zero; the grunnlag is positive here.
        (grunnlag as i128 * sats_bp as i128 + 5_000) / 10_000
    } else {
        0
    } as i64;
    SaldoAr {
        grunnlag_ore: grunnlag,
        avskrivning_ore: avskrivning,
        utgaende_ore: grunnlag - avskrivning,
    }
}

/// Gevinst (positiv) eller tap (negativ) ved avhending.
pub fn gevinst_ved_avhending(bokfort_ore: i64, vederlag_ore: i64) -> i64 {
    vederlag_ore - bokfort_ore
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_sums_exactly() {
        // 35 000 kr across 36 months does not divide evenly.
        let (kostpris, restverdi, levetid) = (3_500_000i64, 0i64, 36i32);
        let sum: i64 = (1..=levetid)
            .map(|m| manedsbelop(kostpris, restverdi, levetid, m))
            .sum();
        assert_eq!(sum, kostpris);
        assert_eq!(manedsbelop(kostpris, restverdi, levetid, 1), 97_222);
        assert_eq!(manedsbelop(kostpris, restverdi, levetid, 36), 97_230);
        // A residual value reduces the depreciable amount.
        let sum: i64 = (1..=60)
            .map(|m| manedsbelop(50_000_00, 5_000_00, 60, m))
            .sum();
        assert_eq!(sum, 45_000_00);
        assert_eq!(manedsbelop(36_000_00, 0, 36, 7), 1_000_00);
    }

    #[test]
    fn a_saldo_year_computes_grunnlag_and_depreciation() {
        // Gruppe d (20 %): opening 0, addition 46 000, proceeds 33 000.
        let ar = saldo_ar(0, 46_000_00, 33_000_00, 2000);
        assert_eq!(ar.grunnlag_ore, 13_000_00);
        assert_eq!(ar.avskrivning_ore, 2_600_00);
        assert_eq!(ar.utgaende_ore, 10_400_00);
        // The next year rolls the closing saldo in as the opening one.
        let neste = saldo_ar(ar.utgaende_ore, 0, 0, 2000);
        assert_eq!(neste.avskrivning_ore, 2_080_00);
        assert_eq!(neste.utgaende_ore, 8_320_00);
    }

    #[test]
    fn a_negative_grunnlag_is_not_depreciated() {
        let ar = saldo_ar(10_000_00, 0, 25_000_00, 2000);
        assert_eq!(ar.grunnlag_ore, -15_000_00);
        assert_eq!(ar.avskrivning_ore, 0);
        assert_eq!(ar.utgaende_ore, -15_000_00, "rapporteres, aldri gjettet");
    }

    #[test]
    fn rounding_is_half_up() {
        // 1,25 kr × 20 % = 0,25 → 25 øre / grunnlag 125 øre, rate 2000 bp
        assert_eq!(saldo_ar(0, 125, 0, 2000).avskrivning_ore, 25);
        // 33 øre × 30 % = 9,9 → 10 øre.
        assert_eq!(saldo_ar(0, 33, 0, 3000).avskrivning_ore, 10);
    }

    #[test]
    fn gain_and_loss() {
        assert_eq!(gevinst_ved_avhending(32_000_00, 33_000_00), 1_000_00);
        assert_eq!(gevinst_ved_avhending(32_000_00, 30_000_00), -2_000_00);
        assert_eq!(gevinst_ved_avhending(0, 0), 0);
    }

    #[test]
    fn the_saldogrupper_run_a_through_j() {
        assert_eq!(SALDOGRUPPER.len(), 10);
        assert!(gyldig_saldogruppe("a") && gyldig_saldogruppe("j"));
        assert!(!gyldig_saldogruppe("k") && !gyldig_saldogruppe(""));
    }
}
