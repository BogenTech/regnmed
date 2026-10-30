//! Periodisering (#87): spreading a cost or an income over the months it
//! belongs to — rskl. §4-1 nr. 2 and 3, the earning and matching
//! principles.
//!
//! Pure and deterministic like the rest of this crate. This module owns
//! ONE thing: how a total is divided across the months. The posting, the
//! table and the run live in regnmed-db, following the pattern from the
//! avskrivninger (#40) and the repeterende fakturaer (#30).
//!
//! **Periodisering moves cost and income, NEVER merverdiavgift.** The
//! tax point follows the sales document (mval. §15-9): a year's rent
//! paid up front is deductible in full in the VAT period the invoice belongs
//! to, however the cost is spread in the resultat. Spread the tax along
//! with it and the mva-melding is wrong — silently wrong, because the
//! resultat looks more correct than before. So the functions here take a
//! NET amount, and the caller has already separated the tax out.

use chrono::NaiveDate;

/// Months from `from` through `to` inclusive, each given as
/// (year, month). 0 or less when the interval is empty or backwards —
/// the caller rejects that; this only counts.
pub fn month_count(from: (i32, u32), to: (i32, u32)) -> i32 {
    (to.0 - from.0) * 12 + to.1 as i32 - from.1 as i32 + 1
}

/// The amount for month `month_no` (1-based) of `count`.
///
/// Same contract as [`crate::anlegg::manedsbelop`], and that is the
/// whole point: every month but the last gets the rounded base amount,
/// and the last takes the remainder. The parts then sum EXACTLY to the
/// total — no floating point, no øre lost or invented.
///
/// Negative totals (an income in ledger signs) are handled by Rust's
/// integer division truncating toward zero: the base amount gets the
/// right sign, and the last month takes the remainder either way.
pub fn monthly_amount(total_ore: i64, count: i32, month_no: i32) -> i64 {
    debug_assert!(count > 0 && month_no >= 1 && month_no <= count);
    let base = total_ore / count as i64;
    if month_no < count {
        base
    } else {
        total_ore - base * (count as i64 - 1)
    }
}

/// Last day of the month — a periodiseringsbilag is dated month-end,
/// like the avskrivninger.
pub fn month_end(year: i32, month: u32) -> NaiveDate {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid date") - chrono::Days::new(1)
}

/// The month `step` months after (year, month), 0-based: `step = 0`
/// gives the month itself.
pub fn month_plus(from: (i32, u32), step: i32) -> (i32, u32) {
    let total = from.0 * 12 + from.1 as i32 - 1 + step;
    (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32)
}

/// The whole plan: one row per month with its amount and voucher date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthRow {
    pub year: i32,
    pub month: u32,
    /// The voucher date: last day of the month.
    pub date: NaiveDate,
    pub amount_ore: i64,
}

/// The plan from `from` through `to` inclusive. Empty when the interval
/// runs backwards — the caller rejects that with a message; this never
/// invents a month.
pub fn plan(total_ore: i64, from: (i32, u32), to: (i32, u32)) -> Vec<MonthRow> {
    let count = month_count(from, to);
    if count <= 0 {
        return Vec::new();
    }
    (1..=count)
        .map(|no| {
            let (year, month) = month_plus(from, no - 1);
            MonthRow {
                year,
                month,
                date: month_end(year, month),
                amount_ore: monthly_amount(total_ore, count, no),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn maneder_are_counted_inclusively_across_year_ends() {
        assert_eq!(month_count((2026, 1), (2026, 1)), 1);
        assert_eq!(month_count((2026, 1), (2026, 12)), 12);
        assert_eq!(month_count((2026, 11), (2027, 2)), 4);
        assert!(month_count((2026, 5), (2026, 4)) <= 0, "backwards");
    }

    #[test]
    fn month_arithmetic_wraps_both_ways() {
        assert_eq!(month_plus((2026, 1), 0), (2026, 1));
        assert_eq!(month_plus((2026, 12), 1), (2027, 1));
        assert_eq!(month_plus((2026, 3), 22), (2028, 1));
        // Negative steps are not used by `plan`, but the arithmetic must
        // not silently produce month 0 if anyone reaches for them.
        assert_eq!(month_plus((2026, 1), -1), (2025, 12));
    }

    #[test]
    fn month_end_handles_february_and_leap_years() {
        assert_eq!(month_end(2026, 1), d(2026, 1, 31));
        assert_eq!(month_end(2026, 2), d(2026, 2, 28));
        assert_eq!(month_end(2028, 2), d(2028, 2, 29), "leap year");
        assert_eq!(month_end(2026, 12), d(2026, 12, 31));
    }

    /// The property the whole feature rests on: the parts sum EXACTLY to
    /// the total. An øre invented or lost here would land in the
    /// resultat and never balance against the source bilag.
    #[test]
    fn the_parts_sum_exactly_to_the_total() {
        // 10 000,01 over 3 months is the classic case: 3333,33 × 3 is
        // one øre short, and the last month has to carry it.
        let p = plan(1_000_001, (2026, 1), (2026, 3));
        assert_eq!(p.iter().map(|r| r.amount_ore).sum::<i64>(), 1_000_001);
        assert_eq!(p[0].amount_ore, 333_333);
        assert_eq!(
            p[2].amount_ore, 333_335,
            "the last month takes the remainder"
        );

        // Exhaustive over awkward totals and lengths.
        for total in [
            1, 7, 99, 100, 101, -1, -7, -99, -101, 1_000_001, -1_000_001, 12_345_678,
        ] {
            for count in 1..=36 {
                let sum: i64 = (1..=count).map(|no| monthly_amount(total, count, no)).sum();
                assert_eq!(sum, total, "total {total} over {count} months");
            }
        }
    }

    #[test]
    fn a_plan_carries_month_ends_across_the_year_boundary() {
        let p = plan(120_000, (2026, 11), (2027, 2));
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].date, d(2026, 11, 30));
        assert_eq!(p[1].date, d(2026, 12, 31));
        assert_eq!(p[2].date, d(2027, 1, 31));
        assert_eq!(p[3].date, d(2027, 2, 28));
        assert!(p.iter().all(|r| r.amount_ore == 30_000));
    }

    /// An income periodisering is negative in ledger signs, and the
    /// remainder must not drift the other way.
    #[test]
    fn income_signs_survive_the_split() {
        let p = plan(-1_000_001, (2026, 1), (2026, 3));
        assert_eq!(p.iter().map(|r| r.amount_ore).sum::<i64>(), -1_000_001);
        assert!(p.iter().all(|r| r.amount_ore < 0), "{p:?}");
    }

    #[test]
    fn a_backwards_interval_yields_no_plan() {
        assert!(plan(100_000, (2026, 5), (2026, 4)).is_empty());
    }
}
