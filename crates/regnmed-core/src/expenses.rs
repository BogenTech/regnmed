//! Utlegg and kjøregodtgjørelse, pure side (docs/utlegg.md, #42).
//!
//! Kjøregodtgjørelse: km × the state's rate, with the trekkfri part
//! (Skattedirektoratet's forskuddssats) separated from the trekkpliktig
//! one. The rates are regelverk data in the satsregister
//! (`km_godtgjorelse`, `km_godtgjorelse_trekkfri`, øre per km) — never
//! hardcoded here. The trekkpliktig part becomes payroll reporting the
//! day lønn / a-melding exists (#46); until then it is reported as a
//! warning, never hidden (per the issue).
//!
//! The mva split for utlegg (gross receipt → net + mva) is
//! [`crate::mva::split_gross`] — the same rounding as all other mva.

/// A computed kjøregodtgjørelse: everything in integer øre, stored on
/// the claim when it is submitted so the row is evidence on its own (rate
/// changes never touch submitted claims).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MileageAllowance {
    pub amount_ore: i64,
    pub tax_free_ore: i64,
    pub taxable_ore: i64,
}

/// km × rate, split into a trekkfri and a trekkpliktig part. If the
/// trekkfri rate is higher than the state's rate (which has happened
/// historically), the whole amount is trekkfri — never a negative
/// trekkplikt.
pub fn mileage_allowance(
    km: i64,
    rate_ore_per_km: i64,
    tax_free_ore_per_km: i64,
) -> MileageAllowance {
    let amount = km * rate_ore_per_km;
    let tax_free = (km * tax_free_ore_per_km).min(amount);
    MileageAllowance {
        amount_ore: amount,
        tax_free_ore: tax_free,
        taxable_ore: amount - tax_free,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_2026_rates_yield_a_trekkpliktig_part() {
        // 120 km à 5,30 kr, trekkfritt 3,50 kr.
        let k = mileage_allowance(120, 530, 350);
        assert_eq!(k.amount_ore, 63_600);
        assert_eq!(k.tax_free_ore, 42_000);
        assert_eq!(k.taxable_ore, 21_600);
    }

    #[test]
    fn a_trekkfri_rate_above_the_states_yields_no_trekkplikt() {
        let k = mileage_allowance(100, 350, 500);
        assert_eq!(k.amount_ore, 35_000);
        assert_eq!(k.tax_free_ore, 35_000, "never more than the amount");
        assert_eq!(k.taxable_ore, 0);
    }

    #[test]
    fn zero_km_is_zero() {
        assert_eq!(
            mileage_allowance(0, 530, 350),
            MileageAllowance {
                amount_ore: 0,
                tax_free_ore: 0,
                taxable_ore: 0
            }
        );
    }
}
