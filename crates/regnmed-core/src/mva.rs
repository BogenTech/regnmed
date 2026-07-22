//! Merverdiavgift: terminer, dated rates and integer-øre beregning.
//!
//! Pure and deterministic like everything in this crate. Rates arrive as
//! data (loaded from the `vat_rate` table by regnmed-db) — the rate valid
//! on the voucher date decides the beregning, never a "current rate".

use chrono::{Datelike, NaiveDate};

/// A standard two-month mva-termin (1 = januar–februar … 6 = november–
/// desember). Årstermin and other special schemes are mva-melding
/// concerns, not ledger concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Termin {
    pub year: i32,
    /// 1–6.
    pub number: u8,
}

impl Termin {
    pub fn of(date: NaiveDate) -> Termin {
        Termin {
            year: date.year(),
            number: ((date.month() + 1) / 2) as u8,
        }
    }

    pub fn new(year: i32, number: u8) -> Option<Termin> {
        (1..=6).contains(&number).then_some(Termin { year, number })
    }

    pub fn start(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, u32::from(self.number) * 2 - 1, 1)
            .expect("termin start is a valid date")
    }

    /// Last day of the termin's second month.
    pub fn end(self) -> NaiveDate {
        let next_month_start = if self.number == 6 {
            NaiveDate::from_ymd_opt(self.year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(self.year, u32::from(self.number) * 2 + 1, 1)
        };
        next_month_start.expect("valid date") - chrono::Days::new(1)
    }
}

impl std::fmt::Display for Termin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}. termin {}", self.number, self.year)
    }
}

/// One row of the dated rate table: `rate_class` charges `rate_bp`
/// (basis points, 25 % = 2500) from `valid_from` until superseded.
#[derive(Debug, Clone)]
pub struct RatePeriod {
    pub rate_class: String,
    pub valid_from: NaiveDate,
    pub rate_bp: i64,
}

/// The rate in force for a class on a date: the latest `valid_from` that
/// is not after the date. `None` before the table's history starts.
pub fn rate_on(rates: &[RatePeriod], rate_class: &str, date: NaiveDate) -> Option<i64> {
    rates
        .iter()
        .filter(|r| r.rate_class == rate_class && r.valid_from <= date)
        .max_by_key(|r| r.valid_from)
        .map(|r| r.rate_bp)
}

/// VAT in øre from a base (grunnlag) in øre, rounded half away from zero.
/// The result carries the base's sign, so ledger conventions (positive =
/// debit) survive the beregning.
pub fn vat_of_base(base_ore: i64, rate_bp: i64) -> i64 {
    let vat = (i128::from(base_ore.unsigned_abs()) * i128::from(rate_bp) + 5_000) / 10_000;
    i64::try_from(vat).expect("vat amount fits in i64") * base_ore.signum()
}

/// Splits a VAT-inclusive amount into (base, vat): base rounds half away
/// from zero, vat is the exact remainder so base + vat == gross always.
pub fn split_gross(gross_ore: i64, rate_bp: i64) -> (i64, i64) {
    let denominator = 10_000 + i128::from(rate_bp);
    let base = (i128::from(gross_ore.unsigned_abs()) * 10_000 + denominator / 2) / denominator;
    let base = i64::try_from(base).expect("base fits in i64") * gross_ore.signum();
    (base, gross_ore - base)
}

/// How a standard code participates in the mva-oppgjør.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Output VAT, payable (codes 3, 31, 32, 33).
    Utgaende,
    /// Input VAT, deductible (codes 1, 11, 12, 13, 14, 15).
    Inngaende,
    /// Import / reverse-charge basis codes (2x, 8x, 9x) — the two-sided
    /// treatment belongs to the mva-melding, not the ledger report.
    OmvendtAvgiftsplikt,
    /// No VAT effect (codes 0, 5, 51, 52, 6, 7).
    Ingen,
}

pub fn direction(code: &str) -> Direction {
    match code {
        "3" | "31" | "32" | "33" => Direction::Utgaende,
        "1" | "11" | "12" | "13" | "14" | "15" => Direction::Inngaende,
        "20" | "21" | "22" | "81" | "82" | "83" | "84" | "85" | "86" | "87" | "88" | "89"
        | "91" | "92" => Direction::OmvendtAvgiftsplikt,
        _ => Direction::Ingen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn termin_boundaries() {
        assert_eq!(Termin::of(date(2026, 1, 1)).number, 1);
        assert_eq!(Termin::of(date(2026, 2, 28)).number, 1);
        assert_eq!(Termin::of(date(2026, 3, 1)).number, 2);
        assert_eq!(Termin::of(date(2026, 12, 31)).number, 6);

        let t1 = Termin::new(2024, 1).unwrap();
        assert_eq!(t1.start(), date(2024, 1, 1));
        assert_eq!(t1.end(), date(2024, 2, 29), "leap year");
        let t6 = Termin::new(2026, 6).unwrap();
        assert_eq!(t6.start(), date(2026, 11, 1));
        assert_eq!(t6.end(), date(2026, 12, 31));

        assert!(Termin::new(2026, 0).is_none());
        assert!(Termin::new(2026, 7).is_none());
    }

    #[test]
    fn rate_lookup_respects_history() {
        let rates = vec![
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2016, 1, 1),
                rate_bp: 1000,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2018, 1, 1),
                rate_bp: 1200,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2020, 4, 1),
                rate_bp: 600,
            },
            RatePeriod {
                rate_class: "low".into(),
                valid_from: date(2021, 10, 1),
                rate_bp: 1200,
            },
        ];
        assert_eq!(rate_on(&rates, "low", date(2017, 6, 1)), Some(1000));
        assert_eq!(rate_on(&rates, "low", date(2019, 6, 1)), Some(1200));
        assert_eq!(rate_on(&rates, "low", date(2020, 6, 1)), Some(600));
        assert_eq!(rate_on(&rates, "low", date(2026, 1, 1)), Some(1200));
        assert_eq!(rate_on(&rates, "low", date(2015, 1, 1)), None);
        assert_eq!(rate_on(&rates, "regular", date(2026, 1, 1)), None);
    }

    #[test]
    fn beregning_is_integer_and_sign_preserving() {
        assert_eq!(vat_of_base(1_000_000, 2500), 250_000);
        assert_eq!(vat_of_base(-1_000_000, 2500), -250_000);
        assert_eq!(vat_of_base(2, 2500), 1, "0,5 øre rounds away from zero");
        assert_eq!(vat_of_base(1_000_000, 1111), 111_100, "råfisk 11,11 %");
    }

    #[test]
    fn split_gross_reconstructs_exactly() {
        assert_eq!(split_gross(1_250_000, 2500), (1_000_000, 250_000));
        assert_eq!(split_gross(-1_250_000, 2500), (-1_000_000, -250_000));
        for gross in [1, 99, 100, 12_345, 999_999_999] {
            let (base, vat) = split_gross(gross, 2500);
            assert_eq!(base + vat, gross, "base + vat must equal gross");
        }
    }

    #[test]
    fn directions_classify_the_standard_codes() {
        assert_eq!(direction("3"), Direction::Utgaende);
        assert_eq!(direction("1"), Direction::Inngaende);
        assert_eq!(direction("86"), Direction::OmvendtAvgiftsplikt);
        assert_eq!(direction("5"), Direction::Ingen);
        assert_eq!(direction("0"), Direction::Ingen);
    }
}
