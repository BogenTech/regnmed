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

/// The rate-register domains a vehicle is paid by: (the state's rate,
/// the trekkfri rate). A car is a car whatever it runs on -- the
/// særavtale and Skatteetaten pay electric and fossil alike; the
/// distinction is kept on the claim because tolls differ. None = not a
/// vehicle we know.
pub fn vehicle_rate_domains(vehicle: &str) -> Option<(&'static str, &'static str)> {
    Some(match vehicle {
        "bil" | "bil_utslippsfri" => ("km_godtgjorelse", "km_godtgjorelse_trekkfri"),
        "motorsykkel" => (
            "km_godtgjorelse_motorsykkel",
            "km_godtgjorelse_motorsykkel_trekkfri",
        ),
        "moped" => ("km_godtgjorelse_moped", "km_godtgjorelse_moped_trekkfri"),
        "snoscooter" => (
            "km_godtgjorelse_snoscooter",
            "km_godtgjorelse_snoscooter_trekkfri",
        ),
        "bat" => ("km_godtgjorelse_bat", "km_godtgjorelse_bat_trekkfri"),
        _ => return None,
    })
}

/// The per-passenger supplement's domains (state, trekkfri).
pub const PASSENGER_RATE_DOMAINS: (&str, &str) =
    ("km_passasjertillegg", "km_passasjertillegg_trekkfri");

/// A trip: km × (the vehicle's rate + one supplement per passenger),
/// split the same way as [`mileage_allowance`]. The supplement is per
/// passenger per km for the whole trip.
pub fn trip_allowance(
    km: i64,
    (rate, tax_free_rate): (i64, i64),
    (passenger_rate, passenger_tax_free_rate): (i64, i64),
    passengers: i64,
) -> MileageAllowance {
    mileage_allowance(
        km,
        rate + passengers * passenger_rate,
        tax_free_rate + passengers * passenger_tax_free_rate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_passengers_add_two_supplements_on_both_sides() {
        // 100 km by car with two colleagues, 2026: (5,30 + 2 × 1,00) kr
        // paid, (3,50 + 2 × 1,00) kr of it trekkfri.
        let k = trip_allowance(100, (530, 350), (100, 100), 2);
        assert_eq!(k.amount_ore, 73_000);
        assert_eq!(k.tax_free_ore, 55_000);
        assert_eq!(k.taxable_ore, 18_000);
    }

    #[test]
    fn every_vehicle_maps_to_a_pair_of_domains() {
        for v in [
            "bil",
            "bil_utslippsfri",
            "motorsykkel",
            "moped",
            "snoscooter",
            "bat",
        ] {
            let (state, tax_free) = vehicle_rate_domains(v).unwrap();
            assert_eq!(format!("{state}_trekkfri"), tax_free);
        }
        assert_eq!(vehicle_rate_domains("sykkel"), None);
    }

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
