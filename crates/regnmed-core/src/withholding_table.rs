//! Tabelltrekk: forskuddstrekk by Skatteetaten's withholding tables
//! (docs/lonn.md «Tabelltrekk»).
//!
//! A port of Skatteetaten's own reference routine,
//! `github.com/Skatteetaten/trekktabell` (Java, `trekkrutine2026`), which
//! IS the definition of the tables: Skatteetaten generates the published
//! table file from it. Verified against that file
//! (`alleTabelleneIEnFilTilSkatteetatenNo.txt`, 2026, 1 063 799 rows) row
//! for row; a deterministic sample is vendored in docs/lonn/ for CI.
//!
//! **This module computes with `f64`, deliberately.** regnmed never uses
//! floats for money, but here the authority's routine is defined in IEEE
//! doubles, and the only way to land on Skatteetaten's kroner is to do
//! the same operations in the same order and round the same way
//! (`java_round` is Java's `Math.round`). The inputs and outputs are whole
//! kroner; no float ever reaches the ledger.
//!
//! The constants change every year. A year without constants here fails
//! loudly ([`TableError::UnsupportedYear`]) rather than reusing last
//! year's — that is a yearly regelverksrevisjon item.

/// The pay period a table is computed for. regnmed's payroll is monthly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Month,
    TwoWeeks,
    Week,
    FourDays,
    ThreeDays,
    TwoDays,
    OneDay,
}

impl Period {
    /// The period digit in Skatteetaten's table file.
    pub fn code(self) -> u8 {
        match self {
            Self::Month => 1,
            Self::TwoWeeks => 2,
            Self::Week => 3,
            Self::FourDays => 4,
            Self::ThreeDays => 5,
            Self::TwoDays => 6,
            Self::OneDay => 7,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        [
            Self::Month,
            Self::TwoWeeks,
            Self::Week,
            Self::FourDays,
            Self::ThreeDays,
            Self::TwoDays,
            Self::OneDay,
        ]
        .into_iter()
        .find(|p| p.code() == code)
    }

    /// (inntektsperiode, trekkperiode, inntektsperiode standardfradrag,
    /// trekkperiode standardfradrag, avrunding) -- `Periode.java`, with the
    /// same expressions so the doubles come out bit-identical.
    fn params(self) -> (f64, f64, f64, f64, i64) {
        match self {
            Self::Month => (12.12, 10.5, 12.0, 10.5, 100),
            Self::TwoWeeks => (26.26, 23.0, 26.0, 23.0, 50),
            Self::Week => (52.52, 46.0, 52.0, 46.0, 20),
            Self::FourDays => (60.60, (60.0 * 46.0) / 52.0, 91.0, (91.0 * 10.5) / 12.0, 20),
            Self::ThreeDays => (
                80.80,
                (80.0 * 46.0) / 52.0,
                122.0,
                (122.0 * 10.5) / 12.0,
                20,
            ),
            Self::TwoDays => (
                121.20,
                (120.0 * 46.0) / 52.0,
                183.0,
                (183.0 * 10.5) / 12.0,
                20,
            ),
            Self::OneDay => (
                242.40,
                (240.0 * 46.0) / 52.0,
                365.0,
                (365.0 * 10.5) / 12.0,
                20,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ordinary,
    StandardDeduction,
    Seafarer,
    Finnmark,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocialSecurity {
    High,
    Low,
    None,
}

/// One table number's parameters (`Tabellnummer.java`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Table {
    kind: Kind,
    /// Positive: tabellfradrag; negative: tabelltillegg.
    table_deduction: i64,
    finnmark: bool,
    social_security: SocialSecurity,
    /// Withholding spread over 12 months instead of 10,5 -- no half
    /// December, trekk on feriepenger.
    pub twelve_months: bool,
}

impl Table {
    pub fn lookup(number: i32) -> Option<Self> {
        use Kind::*;
        use SocialSecurity::{High, Low, None as NoSs};
        let t = |kind, table_deduction, finnmark, social_security, twelve_months| Table {
            kind,
            table_deduction,
            finnmark,
            social_security,
            twelve_months,
        };
        Some(match number {
            // 8000-8400: tabellfradrag in steps of 10 000; 9010-9400:
            // tabelltillegg in steps of 10 000.
            8000..=8400 if number % 10 == 0 => {
                t(Ordinary, (number as i64 - 8000) * 1000, false, High, false)
            }
            9010..=9400 if number % 10 == 0 => {
                t(Ordinary, -(number as i64 - 9000) * 1000, false, High, false)
            }
            7300 => t(StandardDeduction, 0, false, High, false),
            7350 => t(StandardDeduction, 0, false, High, true),
            7500 => t(StandardDeduction, 0, false, NoSs, true),
            7550 => t(StandardDeduction, 0, false, NoSs, false),
            7700 => t(StandardDeduction, 0, false, Low, true),
            6300 => t(Finnmark, 0, true, High, false),
            6350 => t(Finnmark, 0, true, High, true),
            6500 => t(Finnmark, 0, true, NoSs, true),
            6550 => t(Finnmark, 0, true, NoSs, false),
            6700 => t(Finnmark, 0, true, Low, true),
            100 => t(Seafarer, 0, false, NoSs, true),
            101 => t(Seafarer, 0, false, High, true),
            7150 => t(Special, 0, false, High, true),
            7160 => t(Special, 0, false, NoSs, true),
            7170 => t(Special, 0, false, NoSs, false),
            _ => return None,
        })
    }

    fn standard_deduction(&self) -> bool {
        matches!(
            self.kind,
            Kind::StandardDeduction | Kind::Seafarer | Kind::Finnmark
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    UnknownTable(i32),
    UnsupportedYear(i32),
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTable(n) => {
                write!(f, "tabell {n} finnes ikke i Skatteetatens trekktabeller")
            }
            Self::UnsupportedYear(y) => write!(
                f,
                "trekktabellene for {y} er ikke lagt inn i regnmed ennå — de kommer med den \
                 årlige regelverksrevisjonen (docs/lonn.md)"
            ),
        }
    }
}

/// `Konstanter.java` for one income year.
struct Constants {
    personal_allowance: i64,
    personal_allowance_finnmark: i64,
    steps: [i64; 5],
    step_percent: [f64; 5],
    common_tax: f64,
    common_tax_finnmark: f64,
    municipal_tax: f64,
    ss_exempt: i64,
    ss_percent: f64,
    ss_low: f64,
    ss_high: f64,
    max_minimum_deduction: i64,
    minimum_deduction_percent: f64,
    max_minimum_deduction_seafarer: i64,
    minimum_deduction_percent_seafarer: f64,
    max_standard_deduction: i64,
    standard_deduction_percent: f64,
    max_seafarer_deduction: i64,
    seafarer_percent: f64,
}

const Y2026: Constants = Constants {
    personal_allowance: 114_540,
    personal_allowance_finnmark: 159_540,
    steps: [226_100, 318_300, 725_050, 980_100, 1_467_200],
    step_percent: [1.7, 4.0, 13.7, 16.8, 17.8],
    common_tax: 8.25,
    common_tax_finnmark: 4.75,
    municipal_tax: 13.75,
    ss_exempt: 99_650,
    ss_percent: 25.0,
    ss_low: 5.1,
    ss_high: 7.6,
    max_minimum_deduction: 84_216,
    minimum_deduction_percent: 40.48,
    max_minimum_deduction_seafarer: 95_700,
    minimum_deduction_percent_seafarer: 46.0,
    max_standard_deduction: 40_000,
    standard_deduction_percent: 10.0,
    max_seafarer_deduction: 86_300,
    seafarer_percent: 30.0,
};

fn constants(year: i32) -> Result<&'static Constants, TableError> {
    match year {
        2026 => Ok(&Y2026),
        _ => Err(TableError::UnsupportedYear(year)),
    }
}

/// Java's `Math.round(double)`: nearest, ties towards positive infinity.
fn java_round(x: f64) -> i64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor as i64 + 1
    } else {
        floor as i64
    }
}

/// Forskuddstrekk in whole kroner for a trekkgrunnlag in whole kroner
/// (`Trekkrutine.beregnTabelltrekk`).
pub fn withholding_kr(
    year: i32,
    number: i32,
    period: Period,
    basis_kr: i64,
) -> Result<i64, TableError> {
    let c = constants(year)?;
    let table = Table::lookup(number).ok_or(TableError::UnknownTable(number))?;
    let (income_period, withholding_period, income_period_std, withholding_period_std, step) =
        period.params();
    let ordinary = table.kind == Kind::Ordinary;

    let rounded_basis = (basis_kr / step * step) + (step / 2);
    let income_period = if ordinary {
        income_period
    } else {
        income_period_std
    };
    let person_income = java_round(rounded_basis as f64 * income_period);

    // finnAlminneligInntektAar: all longs.
    let class_deduction = if table.finnmark {
        c.personal_allowance_finnmark
    } else {
        c.personal_allowance
    };
    let ordinary_income = person_income
        - minimum_deduction(c, &table, person_income)
        - table.table_deduction
        - standard_deduction(c, &table, person_income)
        - seafarer_deduction(c, &table, person_income)
        - class_deduction;
    let ordinary_income = ordinary_income as f64;
    let person_income_f = person_income as f64;

    let municipal = if ordinary_income > 0.0 {
        java_round(ordinary_income * c.municipal_tax / 100.0)
    } else {
        0
    };
    let common = if ordinary_income < 0.0 {
        0
    } else {
        let rate = if table.kind == Kind::Finnmark {
            c.common_tax_finnmark
        } else {
            c.common_tax
        };
        java_round(ordinary_income * rate / 100.0)
    };
    let tax = municipal
        + common
        + step_tax(c, person_income_f)
        + social_security(c, &table, person_income_f);

    let divisor = if ordinary {
        withholding_period
    } else if table.twelve_months {
        income_period_std
    } else {
        withholding_period_std
    };
    let exact = tax as f64 / divisor;
    let withholding = if table.kind == Kind::Seafarer {
        exact.floor() as i64
    } else {
        java_round(exact)
    };
    Ok(withholding.min(basis_kr))
}

fn minimum_deduction(c: &Constants, table: &Table, income: i64) -> i64 {
    let (percent, max) = if table.kind == Kind::Seafarer {
        (
            c.minimum_deduction_percent_seafarer,
            c.max_minimum_deduction_seafarer,
        )
    } else {
        (c.minimum_deduction_percent, c.max_minimum_deduction)
    };
    // The minimum and lønnsfradrag floors are 0 in 2026, so only the cap
    // and the income itself bind.
    java_round((income as f64 * percent) / 100.0)
        .min(max)
        .max(0)
        .min(income)
}

fn standard_deduction(c: &Constants, table: &Table, income: i64) -> i64 {
    if !table.standard_deduction() {
        return 0;
    }
    java_round((income as f64 * c.standard_deduction_percent) / 100.0).min(c.max_standard_deduction)
}

fn seafarer_deduction(c: &Constants, table: &Table, income: i64) -> i64 {
    if table.kind != Kind::Seafarer {
        return 0;
    }
    java_round((income as f64 * c.seafarer_percent) / 100.0).min(c.max_seafarer_deduction)
}

/// Trinnskatt, summed in the same order as `Skatteberegning.java`.
fn step_tax(c: &Constants, income: f64) -> i64 {
    let s = c.steps.map(|v| v as f64);
    let p = c.step_percent;
    let band = |i: usize| (s[i + 1] - s[i]) * p[i] / 100.0;
    if income < s[0] {
        0
    } else if income < s[1] {
        java_round((income - s[0]) * p[0] / 100.0)
    } else if income < s[2] {
        java_round(band(0) + (income - s[1]) * p[1] / 100.0)
    } else if income < s[3] {
        java_round(band(0) + band(1) + (income - s[2]) * p[2] / 100.0)
    } else if income < s[4] {
        java_round(band(0) + band(1) + band(2) + (income - s[3]) * p[3] / 100.0)
    } else {
        java_round(band(0) + band(1) + band(2) + band(3) + (income - s[4]) * p[4] / 100.0)
    }
}

fn social_security(c: &Constants, table: &Table, income: f64) -> i64 {
    let exempt = c.ss_exempt as f64;
    if income < exempt || table.social_security == SocialSecurity::None {
        return 0;
    }
    let rate = if table.social_security == SocialSecurity::Low {
        c.ss_low
    } else {
        c.ss_high
    };
    // The opptrappingsgrense: below it, 25 % of what exceeds the exempt
    // amount; above it, the full rate on everything.
    let limit = java_round((exempt * c.ss_percent) / (c.ss_percent - rate)) as f64;
    if income > limit {
        java_round(income * rate / 100.0)
    } else {
        java_round((income - exempt) * c.ss_percent / 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row of Skatteetaten's file: table(4) period(1) type(1)
    /// trekkgrunnlag(6) trekk(6).
    fn check_row(line: &str) {
        let n: i32 = line[0..4].parse().unwrap();
        let period = Period::from_code(line[4..5].parse().unwrap()).unwrap();
        let basis: i64 = line[6..12].parse().unwrap();
        let expected: i64 = line[12..18].parse().unwrap();
        assert_eq!(
            withholding_kr(2026, n, period, basis),
            Ok(expected),
            "tabell {n} periode {period:?} grunnlag {basis}"
        );
    }

    /// Every 211th row of Skatteetaten's 2026 file, plus its first and
    /// last row (docs/lonn/trekktabell-2026-utvalg.txt).
    #[test]
    fn matches_skatteetatens_published_tables() {
        let sample = include_str!("../../../docs/lonn/trekktabell-2026-utvalg.txt");
        let mut rows = 0;
        for line in sample.lines().filter(|l| !l.is_empty()) {
            check_row(line);
            rows += 1;
        }
        assert!(rows > 5000, "{rows}");
    }

    /// The whole file, when it is at hand. Never in CI (21 MB):
    ///
    ///     TREKKTABELL_FILE=…/alleTabelleneIEnFilTilSkatteetatenNo.txt \
    ///       cargo test -p regnmed-core withholding_table -- --nocapture
    #[test]
    fn matches_the_whole_published_file() {
        let Ok(path) = std::env::var("TREKKTABELL_FILE") else {
            return;
        };
        let file = std::fs::read_to_string(path).unwrap();
        let mut rows = 0;
        for line in file.lines().filter(|l| !l.trim().is_empty()) {
            check_row(line.trim_end());
            rows += 1;
        }
        println!("{rows} rows match");
        assert_eq!(rows, 1_063_799);
    }

    #[test]
    fn the_reference_routines_own_checks() {
        // TrekkrutineTest: 0 at 0, the trygdeavgift limits, and never more
        // than the basis.
        assert_eq!(withholding_kr(2026, 8000, Period::Month, 0), Ok(0));
        let c = &Y2026;
        let exempt = c.ss_exempt as f64;
        assert_eq!(java_round((exempt * 25.0) / (25.0 - 5.1)), 125_188);
        assert_eq!(java_round((exempt * 25.0) / (25.0 - 7.6)), 143_175);
        for basis in (10..1000).step_by(166) {
            assert!(withholding_kr(2026, 9400, Period::OneDay, basis).unwrap() <= basis);
        }
    }

    #[test]
    fn unknown_tables_and_years_are_refused() {
        assert_eq!(
            withholding_kr(2026, 8005, Period::Month, 50_000),
            Err(TableError::UnknownTable(8005))
        );
        assert_eq!(
            withholding_kr(2025, 8000, Period::Month, 50_000),
            Err(TableError::UnsupportedYear(2025))
        );
    }

    #[test]
    fn java_round_ties_go_up() {
        assert_eq!(java_round(2.5), 3);
        assert_eq!(java_round(-2.5), -2);
        assert_eq!(java_round(2.4999999999999996), 2);
    }
}
