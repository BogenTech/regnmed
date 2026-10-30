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
pub struct Movement {
    pub quantity_milli: i64,
    pub cost_ore: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stock {
    pub quantity_milli: i64,
    pub value_ore: i64,
}

impl Stock {
    /// Running average cost per unit, when there is stock to price.
    pub fn average_ore(&self) -> Option<i64> {
        if self.quantity_milli > 0 {
            Some(div_round(
                self.value_ore as i128 * 1000,
                self.quantity_milli as i128,
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
pub fn value_stock<'a>(movements: impl IntoIterator<Item = &'a Movement>) -> Stock {
    let mut status = Stock::default();
    for b in movements {
        if b.quantity_milli > 0 {
            status.value_ore += match b.cost_ore {
                Some(cost) => div_round(b.quantity_milli as i128 * cost as i128, 1000),
                None if status.quantity_milli > 0 => div_round(
                    status.value_ore as i128 * b.quantity_milli as i128,
                    status.quantity_milli as i128,
                ),
                None => 0,
            };
        } else if b.quantity_milli < 0 && status.quantity_milli > 0 {
            let out = (-b.quantity_milli).min(status.quantity_milli);
            status.value_ore -= div_round(
                status.value_ore as i128 * out as i128,
                status.quantity_milli as i128,
            );
        }
        status.quantity_milli += b.quantity_milli;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbound(quantity_milli: i64, cost: i64) -> Movement {
        Movement {
            quantity_milli,
            cost_ore: Some(cost),
        }
    }

    fn movement(quantity_milli: i64) -> Movement {
        Movement {
            quantity_milli,
            cost_ore: None,
        }
    }

    #[test]
    fn average_across_two_purchases() {
        // 10 units at 100 kr + 10 at 200 kr → average 150 kr; sell 5.
        let status = value_stock(&[
            inbound(10_000, 100_00),
            inbound(10_000, 200_00),
            movement(-5_000),
        ]);
        assert_eq!(status.quantity_milli, 15_000);
        assert_eq!(status.value_ore, 3_000_00 - 750_00);
        assert_eq!(status.average_ore(), Some(150_00));
    }

    #[test]
    fn emptying_exactly_leaves_zero_value() {
        let status = value_stock(&[inbound(3_000, 99_99), movement(-3_000)]);
        assert_eq!(status, Stock::default());
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        // 3 units at 100 kr = 300 kr; sell 1 → remove 100,00 exactly.
        // 3 units at 100,00 total (33,3333 kr average); sell 1 → 33,33.
        let status = value_stock(&[inbound(3_000, 33_33), movement(-1_000)]);
        assert_eq!(status.value_ore, 99_99 - 33_33);
        // Odd case: value 1,01 kr across 2 units, sell 1 → remove 0,51 (half up).
        let status = value_stock(&[inbound(1_000, 33), inbound(1_000, 68), movement(-1_000)]);
        assert_eq!(status.value_ore, 101 - 51);
    }

    #[test]
    fn an_inbound_without_cost_is_taken_at_the_average() {
        // Stocktake up / kreditnota return: in at the running average.
        let status = value_stock(&[inbound(10_000, 100_00), movement(-4_000), movement(2_000)]);
        assert_eq!(status.quantity_milli, 8_000);
        assert_eq!(status.value_ore, 800_00);
        // Into empty stock without a cost: value 0.
        let status = value_stock(&[movement(5_000)]);
        assert_eq!(status.value_ore, 0);
        assert_eq!(status.quantity_milli, 5_000);
    }

    #[test]
    fn overselling_yields_negative_stock_and_zero_value() {
        let status = value_stock(&[inbound(2_000, 100_00), movement(-3_000)]);
        assert_eq!(status.quantity_milli, -1_000);
        assert_eq!(status.value_ore, 0);
        // A sale from empty stock changes only the quantity.
        let status = value_stock(&[movement(-1_000)]);
        assert_eq!(status.quantity_milli, -1_000);
        assert_eq!(status.value_ore, 0);
    }

    #[test]
    fn fractional_quantities() {
        // 2,5 kg at 40 kr/kg = 100 kr.
        let status = value_stock(&[inbound(2_500, 40_00)]);
        assert_eq!(status.value_ore, 100_00);
        assert_eq!(status.average_ore(), Some(40_00));
    }
}
