//! The fiscal year — the assumption, in one place (docs/regelverk.md,
//! #52).
//!
//! **regnmed assumes the fiscal year is the calendar year.** That is the
//! main rule in regnskapsloven §1-7 and the situation of virtually the
//! whole target group. An avvikende regnskapsår is permitted in defined
//! cases (seasonal businesses, groups with a foreign parent), and then
//! the assumption does not hold.
//!
//! The assumption has not been removed, but it has been **gathered and
//! named**: the day a customer with an avvikende regnskapsår turns up,
//! this is where the definition changes, and docs/regelverk.md lists
//! exactly which other places must follow. A `.year()` scattered through
//! the code is an assumption nobody can find again; a named function is a
//! decision somebody can revisit.
//!
//! Note what does NOT belong here: **mva terminer are calendar-anchored
//! regardless** (mval. §15-1 cf. sktfvf. §8-3). They do not follow the
//! fiscal year, and must never take their period from here.

use chrono::{Datelike, NaiveDate};

/// The fiscal year a date belongs to.
///
/// Today: the calendar year. This is what gap-free voucher numbering is
/// keyed by (`voucher_counter(journal_id, fiscal_year)`), so changing
/// it changes which counter a voucher draws its number from — never do
/// it for a company that already has vouchers in the affected period.
pub fn regnskapsar(dato: NaiveDate) -> i32 {
    dato.year()
}

/// First and last day of a fiscal year, inclusive — the period a
/// year-based report covers.
pub fn regnskapsar_periode(ar: i32) -> Option<(NaiveDate, NaiveDate)> {
    Some((
        NaiveDate::from_ymd_opt(ar, 1, 1)?,
        NaiveDate::from_ymd_opt(ar, 12, 31)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the assumption. If this test is ever changed, it must be
    /// because someone decided to support avvikende regnskapsår — not
    /// because a date happened to fall on the wrong side of something.
    #[test]
    fn the_fiscal_year_is_the_calendar_year() {
        let dato = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(regnskapsar(dato(2026, 1, 1)), 2026);
        assert_eq!(regnskapsar(dato(2026, 12, 31)), 2026);
        // New Year's Eve and New Year's Day belong to different years,
        // and to different voucher-number runs.
        assert_eq!(regnskapsar(dato(2027, 1, 1)), 2027);
    }

    #[test]
    fn the_period_covers_the_whole_year() {
        let (start, slutt) = regnskapsar_periode(2026).unwrap();
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(slutt, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        assert_eq!(regnskapsar(start), 2026);
        assert_eq!(regnskapsar(slutt), 2026);
        // A leap year has its extra day inside the period.
        let (_, slutt) = regnskapsar_periode(2028).unwrap();
        assert_eq!(slutt.ordinal(), 366);
    }
}
