//! Fixed asset register, pure side (docs/anlegg.md, #40): straight-line
//! avskrivninger for the accounts and saldo depreciation per gruppe (tax group) for
//! the skattemelding — both as pure functions over register data, all in
//! integer øre.
//!
//! For the accounts: the depreciable amount (cost − residual value)
//! is spread across the useful life in fixed monthly amounts; the last
//! month takes the remainder, so the plan sums EXACTLY to the depreciable
//! amount.
//!
//! For tax: the saldo method (skatteloven §14-40 flg.) — the basis is
//! opening saldo + additions − proceeds; the year's depreciation is
//! basis × the gruppe's rate when the basis is positive. A negative
//! saldo (proceeds above the saldo) is not depreciated — it is reported
//! as a candidate for income recognition and handled by the
//! regnskapsfører (deliberately out of scope, see docs/anlegg.md).

/// The saldogrupper in skatteloven §14-41 with descriptions — the rates
/// themselves are regelverk data in the satsregister (domain
/// `saldogruppe_<letter>`), never hardcoded here.
pub const DEPRECIATION_GROUPS: &[(&str, &str)] = &[
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

pub fn is_valid_depreciation_group(group: &str) -> bool {
    DEPRECIATION_GROUPS.iter().any(|(g, _)| *g == group)
}

/// The monthly amount for month `month_no` (1-based) in a straight-line
/// plan. Every month but the last gets the rounded base amount; the last
/// takes the remainder, so the sum across the life is exactly
/// `cost_ore - residual_ore`.
pub fn monthly_amount(cost_ore: i64, residual_ore: i64, life_months: i32, month_no: i32) -> i64 {
    debug_assert!(life_months > 0 && month_no >= 1 && month_no <= life_months);
    let depreciable = cost_ore - residual_ore;
    let base = depreciable / life_months as i64;
    if month_no < life_months {
        base
    } else {
        depreciable - base * (life_months as i64 - 1)
    }
}

/// One saldo year: basis, the year's depreciation and closing saldo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxDepreciationYear {
    pub basis_ore: i64,
    pub depreciation_ore: i64,
    pub closing_ore: i64,
}

/// The saldo method for one year. A negative basis is never
/// depreciated — it is passed on as closing saldo (a candidate for
/// income recognition).
pub fn tax_depreciation_year(
    opening_ore: i64,
    additions_ore: i64,
    proceeds_ore: i64,
    sats_bp: i64,
) -> TaxDepreciationYear {
    let basis = opening_ore + additions_ore - proceeds_ore;
    let depreciation = if basis > 0 {
        // Half away from zero; the basis is positive here.
        (basis as i128 * sats_bp as i128 + 5_000) / 10_000
    } else {
        0
    } as i64;
    TaxDepreciationYear {
        basis_ore: basis,
        depreciation_ore: depreciation,
        closing_ore: basis - depreciation,
    }
}

/// Gain (positive) or loss (negative) on disposal.
pub fn gain_on_disposal(book_value_ore: i64, proceeds_ore: i64) -> i64 {
    proceeds_ore - book_value_ore
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_sums_exactly() {
        // 35 000 kr across 36 months does not divide evenly.
        let (cost, residual, life) = (3_500_000i64, 0i64, 36i32);
        let sum: i64 = (1..=life)
            .map(|m| monthly_amount(cost, residual, life, m))
            .sum();
        assert_eq!(sum, cost);
        assert_eq!(monthly_amount(cost, residual, life, 1), 97_222);
        assert_eq!(monthly_amount(cost, residual, life, 36), 97_230);
        // A residual value reduces the depreciable amount.
        let sum: i64 = (1..=60)
            .map(|m| monthly_amount(50_000_00, 5_000_00, 60, m))
            .sum();
        assert_eq!(sum, 45_000_00);
        assert_eq!(monthly_amount(36_000_00, 0, 36, 7), 1_000_00);
    }

    #[test]
    fn a_saldo_year_computes_basis_and_depreciation() {
        // Gruppe d (20 %): opening 0, addition 46 000, proceeds 33 000.
        let year = tax_depreciation_year(0, 46_000_00, 33_000_00, 2000);
        assert_eq!(year.basis_ore, 13_000_00);
        assert_eq!(year.depreciation_ore, 2_600_00);
        assert_eq!(year.closing_ore, 10_400_00);
        // The next year rolls the closing saldo in as the opening one.
        let next = tax_depreciation_year(year.closing_ore, 0, 0, 2000);
        assert_eq!(next.depreciation_ore, 2_080_00);
        assert_eq!(next.closing_ore, 8_320_00);
    }

    #[test]
    fn a_negative_basis_is_not_depreciated() {
        let year = tax_depreciation_year(10_000_00, 0, 25_000_00, 2000);
        assert_eq!(year.basis_ore, -15_000_00);
        assert_eq!(year.depreciation_ore, 0);
        assert_eq!(year.closing_ore, -15_000_00, "reported, never guessed");
    }

    #[test]
    fn rounding_is_half_up() {
        // 1,25 kr × 20 % = 0,25 → 25 øre / basis 125 øre, rate 2000 bp
        assert_eq!(tax_depreciation_year(0, 125, 0, 2000).depreciation_ore, 25);
        // 33 øre × 30 % = 9,9 → 10 øre.
        assert_eq!(tax_depreciation_year(0, 33, 0, 3000).depreciation_ore, 10);
    }

    #[test]
    fn gain_and_loss() {
        assert_eq!(gain_on_disposal(32_000_00, 33_000_00), 1_000_00);
        assert_eq!(gain_on_disposal(32_000_00, 30_000_00), -2_000_00);
        assert_eq!(gain_on_disposal(0, 0), 0);
    }

    #[test]
    fn the_saldogrupper_run_a_through_j() {
        assert_eq!(DEPRECIATION_GROUPS.len(), 10);
        assert!(is_valid_depreciation_group("a") && is_valid_depreciation_group("j"));
        assert!(!is_valid_depreciation_group("k") && !is_valid_depreciation_group(""));
    }
}
