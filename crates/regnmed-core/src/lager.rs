//! Simple inventory (docs/produkter.md, #39): stock and value as a pure
//! function over the movements — never stored, the same philosophy as
//! balances. Valued by the weighted-average method: purchases at
//! acquisition cost, withdrawals at the running average cost. All in
//! integers (milli-units and øre); rounding half away from zero per
//! movement, so the status is deterministic wherever it is computed.
//!
//! Edge-case rules (deliberately simple, documented here and in the
//! tests):
//! - An inbound movement without a cost (stocktake up, kreditnota return)
//!   is taken in at the running average cost; if stock is empty, at 0.
//! - A withdrawal beyond the stock removes the whole value and lets the
//!   quantity go negative — negative stock is a counting discrepancy that
//!   must be visible, not hidden.
//! - When stock empties exactly, the value is removed exactly (the
//!   proportion is 1), so no residual øre is left behind.

/// One inventory movement, chronological order is the caller's job.
/// Quantities are milli-units (1000 = one unit) matching invoice line
/// quantities; `kostpris_ore` is the acquisition cost PER UNIT and is
/// only meaningful on inbound movements.
#[derive(Debug, Clone, Copy)]
pub struct Bevegelse {
    pub antall_milli: i64,
    pub kostpris_ore: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LagerStatus {
    pub antall_milli: i64,
    pub verdi_ore: i64,
}

impl LagerStatus {
    /// Running average cost per unit, when there is stock to price.
    pub fn gjennomsnitt_ore(&self) -> Option<i64> {
        if self.antall_milli > 0 {
            Some(div_round(
                self.verdi_ore as i128 * 1000,
                self.antall_milli as i128,
            ))
        } else {
            None
        }
    }
}

/// Integer division rounded half away from zero.
fn div_round(n: i128, d: i128) -> i64 {
    debug_assert!(d != 0);
    let (n, d, sign) = if (n < 0) != (d < 0) {
        (n.abs(), d.abs(), -1)
    } else {
        (n.abs(), d.abs(), 1)
    };
    (sign * ((n + d / 2) / d)) as i64
}

/// Folds movements (in the order given) into beholdning + verdi.
pub fn verdsett<'a>(bevegelser: impl IntoIterator<Item = &'a Bevegelse>) -> LagerStatus {
    let mut status = LagerStatus::default();
    for b in bevegelser {
        if b.antall_milli > 0 {
            status.verdi_ore += match b.kostpris_ore {
                Some(kost) => div_round(b.antall_milli as i128 * kost as i128, 1000),
                None if status.antall_milli > 0 => div_round(
                    status.verdi_ore as i128 * b.antall_milli as i128,
                    status.antall_milli as i128,
                ),
                None => 0,
            };
        } else if b.antall_milli < 0 && status.antall_milli > 0 {
            let ut = (-b.antall_milli).min(status.antall_milli);
            status.verdi_ore -= div_round(
                status.verdi_ore as i128 * ut as i128,
                status.antall_milli as i128,
            );
        }
        status.antall_milli += b.antall_milli;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inn(antall_milli: i64, kost: i64) -> Bevegelse {
        Bevegelse {
            antall_milli,
            kostpris_ore: Some(kost),
        }
    }

    fn bev(antall_milli: i64) -> Bevegelse {
        Bevegelse {
            antall_milli,
            kostpris_ore: None,
        }
    }

    #[test]
    fn average_across_two_purchases() {
        // 10 units at 100 kr + 10 at 200 kr → average 150 kr; sell 5.
        let status = verdsett(&[inn(10_000, 100_00), inn(10_000, 200_00), bev(-5_000)]);
        assert_eq!(status.antall_milli, 15_000);
        assert_eq!(status.verdi_ore, 3_000_00 - 750_00);
        assert_eq!(status.gjennomsnitt_ore(), Some(150_00));
    }

    #[test]
    fn emptying_exactly_leaves_zero_value() {
        let status = verdsett(&[inn(3_000, 99_99), bev(-3_000)]);
        assert_eq!(status, LagerStatus::default());
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        // 3 units at 100 kr = 300 kr; sell 1 → remove 100,00 exactly.
        // 3 units at 100,00 total (33,3333 kr average); sell 1 → 33,33.
        let status = verdsett(&[inn(3_000, 33_33), bev(-1_000)]);
        assert_eq!(status.verdi_ore, 99_99 - 33_33);
        // Odd case: value 1,01 kr across 2 units, sell 1 → remove 0,51 (half up).
        let status = verdsett(&[inn(1_000, 33), inn(1_000, 68), bev(-1_000)]);
        assert_eq!(status.verdi_ore, 101 - 51);
    }

    #[test]
    fn an_inbound_without_cost_is_taken_at_the_average() {
        // Stocktake up / kreditnota return: in at the running average.
        let status = verdsett(&[inn(10_000, 100_00), bev(-4_000), bev(2_000)]);
        assert_eq!(status.antall_milli, 8_000);
        assert_eq!(status.verdi_ore, 800_00);
        // Into empty stock without a cost: value 0.
        let status = verdsett(&[bev(5_000)]);
        assert_eq!(status.verdi_ore, 0);
        assert_eq!(status.antall_milli, 5_000);
    }

    #[test]
    fn overselling_yields_negative_stock_and_zero_value() {
        let status = verdsett(&[inn(2_000, 100_00), bev(-3_000)]);
        assert_eq!(status.antall_milli, -1_000);
        assert_eq!(status.verdi_ore, 0);
        // A sale from empty stock changes only the quantity.
        let status = verdsett(&[bev(-1_000)]);
        assert_eq!(status.antall_milli, -1_000);
        assert_eq!(status.verdi_ore, 0);
    }

    #[test]
    fn fractional_quantities() {
        // 2,5 kg at 40 kr/kg = 100 kr.
        let status = verdsett(&[inn(2_500, 40_00)]);
        assert_eq!(status.verdi_ore, 100_00);
        assert_eq!(status.gjennomsnitt_ore(), Some(40_00));
    }
}
