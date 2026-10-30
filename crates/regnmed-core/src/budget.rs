//! Budget and variance report (docs/budsjett.md, #41) — plain arithmetic
//! over numbers we already trust.
//!
//! The amounts here are in **presentation sign** (like the resultat
//! report: income positive, cost positive), because a budget is written
//! the way a human reads it. Actuals are converted from the hovedbok's
//! debit/credit with [`crate::regnskap::presentasjon_ore`] before they
//! meet the budget, so the comparison happens in one space.
//!
//! `variance = actual − budget` per account. The sign is interpreted by the
//! reader: for an income account a positive variance is better than
//! planned, for a cost account it is more expensive. The report does not
//! pretend to know what is "good" — it shows the numbers.

use crate::regnskap::class_of;

/// One account's year: twelve months of budget and twelve of actuals,
/// both presentation sign.
#[derive(Debug, Clone)]
pub struct AccountFigures {
    pub number: String,
    pub name: String,
    pub budget: [i64; 12],
    pub actual: [i64; 12],
}

#[derive(Debug, Clone)]
pub struct VarianceLine {
    pub number: String,
    pub name: String,
    /// Sum of months 1..=through_month.
    pub budget_ytd_ore: i64,
    pub actual_ytd_ore: i64,
    /// `actual_ytd − budget_ytd`.
    pub variance_ytd_ore: i64,
    /// The whole budgeted year, however far the ledger has come.
    pub budget_year_ore: i64,
    pub budget_months: [i64; 12],
    pub actual_months: [i64; 12],
}

#[derive(Debug)]
pub struct VarianceSection {
    pub heading: &'static str,
    pub lines: Vec<VarianceLine>,
    pub budget_ytd_ore: i64,
    pub actual_ytd_ore: i64,
    pub variance_ytd_ore: i64,
    pub budget_year_ore: i64,
}

#[derive(Debug)]
pub struct VarianceReport {
    /// Months 1..=through_month are counted as year-to-date.
    pub through_month: u32,
    pub sections: Vec<VarianceSection>,
    /// Income − costs, year to date.
    pub result_budget_ytd_ore: i64,
    pub result_actual_ytd_ore: i64,
    pub result_variance_ytd_ore: i64,
    pub result_budget_year_ore: i64,
    /// Per month: budgeted and actual result (inntekter − kostnader).
    pub result_budget_months: [i64; 12],
    pub result_actual_months: [i64; 12],
}

/// The NS 4102 sections, the same split as the resultat report.
const SECTIONS: [(&str, &[u32]); 5] = [
    ("Driftsinntekter", &[3]),
    ("Varekostnad", &[4]),
    ("Lønnskostnad", &[5]),
    ("Annen driftskostnad", &[6, 7]),
    ("Finansposter, skatt m.m.", &[8]),
];

/// Class 3 is income; 4–8 are costs. The result adds income and
/// subtracts costs — both are positive in presentation sign.
fn presentation_sign(number: &str) -> i64 {
    match class_of(number) {
        Some(3) => 1,
        _ => -1,
    }
}

/// Builds the variance report. `through_month` (1..=12) decides how far
/// year-to-date reaches — normally the current month for the running year,
/// 12 for a finished one. Accounts with nothing budgeted and nothing
/// booked are left out; an account that appears on only one side is
/// kept, with zeroes on the other (a cost nobody planned is exactly
/// what an avviksrapport is for).
pub fn variance(figures: &[AccountFigures], through_month: u32) -> VarianceReport {
    let through = through_month.clamp(1, 12) as usize;
    let year_to_date = |v: &[i64; 12]| v[..through].iter().sum::<i64>();

    let mut sections = Vec::with_capacity(SECTIONS.len());
    let mut result_budget_months = [0i64; 12];
    let mut result_actual_months = [0i64; 12];
    let mut result_budget_year = 0i64;

    for (heading, classes) in SECTIONS {
        let mut lines: Vec<VarianceLine> = figures
            .iter()
            .filter(|k| class_of(&k.number).is_some_and(|c| classes.contains(&c)))
            .filter(|k| k.budget.iter().any(|v| *v != 0) || k.actual.iter().any(|v| *v != 0))
            .map(|k| {
                let budget_ytd_ore = year_to_date(&k.budget);
                let actual_ytd_ore = year_to_date(&k.actual);
                VarianceLine {
                    number: k.number.clone(),
                    name: k.name.clone(),
                    budget_ytd_ore,
                    actual_ytd_ore,
                    variance_ytd_ore: actual_ytd_ore - budget_ytd_ore,
                    budget_year_ore: k.budget.iter().sum(),
                    budget_months: k.budget,
                    actual_months: k.actual,
                }
            })
            .collect();
        lines.sort_by(|a, b| a.number.cmp(&b.number));

        for line in &lines {
            let sign = presentation_sign(&line.number);
            for m in 0..12 {
                result_budget_months[m] += sign * line.budget_months[m];
                result_actual_months[m] += sign * line.actual_months[m];
            }
            result_budget_year += sign * line.budget_year_ore;
        }

        let budget_ytd_ore = lines.iter().map(|l| l.budget_ytd_ore).sum();
        let actual_ytd_ore = lines.iter().map(|l| l.actual_ytd_ore).sum();
        sections.push(VarianceSection {
            heading,
            budget_ytd_ore,
            actual_ytd_ore,
            variance_ytd_ore: actual_ytd_ore - budget_ytd_ore,
            budget_year_ore: lines.iter().map(|l| l.budget_year_ore).sum(),
            lines,
        });
    }

    let result_budget_ytd_ore = year_to_date(&result_budget_months);
    let result_actual_ytd_ore = year_to_date(&result_actual_months);
    VarianceReport {
        through_month: through as u32,
        sections,
        result_budget_ytd_ore,
        result_actual_ytd_ore,
        result_variance_ytd_ore: result_actual_ytd_ore - result_budget_ytd_ore,
        result_budget_year_ore: result_budget_year,
        result_budget_months,
        result_actual_months,
    }
}

/// "Build a budget from last year ±X %": scales an amount by basis
/// points (500 = +5 %, -1000 = −10 %), rounding half away from zero so
/// the suggestion never invents or loses an øre through truncation. Pure
/// integer arithmetic — no floats near money, ever.
pub fn scale_ore(amount_ore: i64, scale_bp: i64) -> i64 {
    let faktor = 10_000 + scale_bp;
    let produkt = (amount_ore as i128) * (faktor as i128);
    let half = 10_000i128 / 2;
    let rounded = if produkt >= 0 {
        (produkt + half) / 10_000
    } else {
        (produkt - half) / 10_000
    };
    rounded as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(number: &str, budget: [i64; 12], actual: [i64; 12]) -> AccountFigures {
        AccountFigures {
            number: number.into(),
            name: format!("Konto {number}"),
            budget,
            actual,
        }
    }

    fn evenly(per_month: i64) -> [i64; 12] {
        [per_month; 12]
    }

    #[test]
    fn year_to_date_stops_at_the_chosen_month() {
        let figures = vec![account("3000", evenly(100_00), evenly(90_00))];
        let report = variance(&figures, 3);
        let line = &report.sections[0].lines[0];
        assert_eq!(line.budget_ytd_ore, 300_00);
        assert_eq!(line.actual_ytd_ore, 270_00);
        assert_eq!(line.variance_ytd_ore, -30_00, "income shortfall");
        assert_eq!(line.budget_year_ore, 1_200_00, "the whole budgeted year");
    }

    #[test]
    fn the_result_adds_income_and_subtracts_cost() {
        let figures = vec![
            account("3000", evenly(100_00), evenly(120_00)),
            account("5000", evenly(40_00), evenly(45_00)),
            account("6300", evenly(10_00), evenly(10_00)),
        ];
        let report = variance(&figures, 12);
        // Budgeted result: (100 − 40 − 10) × 12.
        assert_eq!(report.result_budget_ytd_ore, 600_00);
        assert_eq!(report.result_actual_ytd_ore, 780_00);
        assert_eq!(report.result_variance_ytd_ore, 180_00);
        assert_eq!(report.result_budget_year_ore, 600_00);
        assert_eq!(report.result_actual_months[0], 65_00);
    }

    #[test]
    fn an_account_present_on_only_one_side_is_kept() {
        let figures = vec![
            // An unbudgeted cost — exactly what the report is for.
            account("7770", [0; 12], evenly(5_00)),
            // Budgeted, but nothing booked yet.
            account("6300", evenly(3_00), [0; 12]),
            // Neither plan nor reality: left out.
            account("6400", [0; 12], [0; 12]),
        ];
        let report = variance(&figures, 12);
        let annen = report
            .sections
            .iter()
            .find(|s| s.heading == "Annen driftskostnad")
            .unwrap();
        assert_eq!(annen.lines.len(), 2);
        assert_eq!(annen.lines[0].number, "6300");
        assert_eq!(annen.lines[0].actual_ytd_ore, 0);
        assert_eq!(annen.lines[1].variance_ytd_ore, 60_00);
    }

    #[test]
    fn the_sections_follow_ns_4102() {
        let figures = vec![
            account("3000", evenly(1_00), [0; 12]),
            account("4300", evenly(1_00), [0; 12]),
            account("5000", evenly(1_00), [0; 12]),
            account("6300", evenly(1_00), [0; 12]),
            account("7100", evenly(1_00), [0; 12]),
            account("8050", evenly(1_00), [0; 12]),
        ];
        let report = variance(&figures, 12);
        let headings: Vec<_> = report.sections.iter().map(|s| s.heading).collect();
        assert_eq!(
            headings,
            vec![
                "Driftsinntekter",
                "Varekostnad",
                "Lønnskostnad",
                "Annen driftskostnad",
                "Finansposter, skatt m.m.",
            ]
        );
        // 6 and 7 share a section.
        assert_eq!(report.sections[3].lines.len(), 2);
    }

    #[test]
    fn scaling_rounds_half_ore_away_from_zero() {
        assert_eq!(scale_ore(100_00, 0), 100_00);
        assert_eq!(scale_ore(100_00, 500), 105_00);
        assert_eq!(scale_ore(100_00, -1000), 90_00);
        // 1 øre + 5 % = 1,05 øre → 1; 1 øre + 50 % = 1,5 → 2 (away from zero).
        assert_eq!(scale_ore(1, 500), 1);
        assert_eq!(scale_ore(1, 5000), 2);
        assert_eq!(scale_ore(-1, 5000), -2);
    }
}
