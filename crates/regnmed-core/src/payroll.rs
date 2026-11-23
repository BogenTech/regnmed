//! Lønn: brutto → forskuddstrekk → netto, arbeidsgiveravgift and
//! feriepengeavsetning (docs/lonn.md, first part of #46).
//!
//! Everything is integer øre. No float is ever involved, and every
//! rounding happens half away from zero at one defined place, so a
//! payroll run yields exactly the same numbers wherever it is computed.
//!
//! **What this module does NOT do**, and why that is a choice rather than
//! an oversight:
//!
//! - **12-month withholding tables.** Tabelltrekk itself is Skatteetaten's
//!   own routine (`withholding_table`, verified against the published file
//!   row for row); the tables spread over 12 months are refused, because
//!   they withhold in December and on feriepenger and `calculate` does not.
//! - **Sone Ia.** The reduced rate applies only until the fribeløp is
//!   used up, and that fribeløp is de minimis aid which can also be
//!   consumed by things regnmed cannot see. Computing 10,6 % without the
//!   whole picture would under-report avgift.
//!
//! The rates themselves are data in the satsregister
//! (docs/regelverk.md), not numbers in this code.

use chrono::NaiveDate;

/// Rounds a basis-point calculation half away from zero.
pub(crate) fn bp_of(amount_ore: i64, rate_bp: i64) -> i64 {
    let n = amount_ore as i128 * rate_bp as i128;
    let d = 10_000i128;
    let sign = if (n < 0) != (d < 0) { -1 } else { 1 };
    (sign * ((n.abs() + d / 2) / d)) as i64
}

/// Arbeidsgiveravgift-soner (Skattedirektoratets melding, årlig).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    I,
    Ia,
    II,
    III,
    IV,
    IVa,
    V,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PayrollError {
    /// Unknown table number, or a year whose tables are not in regnmed.
    Table(crate::withholding_table::TableError),
    /// Sone Ia's reduced rate is bounded by a yearly fribeløp.
    ZoneIaRequiresExemptionCalculation,
    /// The satsregister has no verified rate covering this date.
    MissingRate(String, NaiveDate),
}

impl std::fmt::Display for PayrollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table(e) => write!(f, "{e}"),
            Self::ZoneIaRequiresExemptionCalculation => write!(
                f,
                "sone Ia er ikke støttet: den reduserte satsen gjelder bare til \
                 fribeløpet er brukt opp, og fribeløpet er bagatellmessig støtte \
                 som også forbrukes utenfor regnmed — å regne redusert sats uten \
                 hele bildet ville underrapportert avgift (se docs/lonn.md)"
            ),
            Self::MissingRate(domain, date) => write!(
                f,
                "satsregisteret har ingen verifisert «{domain}» som dekker {date} \
                 — legg inn satsen med kilde før lønnskjøringen"
            ),
        }
    }
}

impl std::error::Error for PayrollError {}

impl Zone {
    pub fn slug(self) -> &'static str {
        match self {
            Self::I => "I",
            Self::Ia => "Ia",
            Self::II => "II",
            Self::III => "III",
            Self::IV => "IV",
            Self::IVa => "IVa",
            Self::V => "V",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        [
            Self::I,
            Self::Ia,
            Self::II,
            Self::III,
            Self::IV,
            Self::IVa,
            Self::V,
        ]
        .into_iter()
        .find(|s| s.slug() == slug)
    }

    /// The satsregister domain carrying this zone's rate.
    pub fn rate_domain(self) -> String {
        format!("aga_sone_{}", self.slug().to_lowercase())
    }
}

/// How the skattekort says to withhold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withholding {
    /// Percentage in basis points (35 % = 3500), spread over 10,5 months:
    /// half in December, nothing on feriepenger.
    Percent(i64),
    /// Percentage spread over 12 months (kildeskatt, among others): the
    /// same rate every month, ALSO on feriepenger and in December
    /// (skattebetalingsloven § 5-7 annet ledd; Skatteetaten: «Dette gjeld
    /// også i feriepengane og i lønna før jul»).
    PercentTwelveMonths(i64),
    /// Table number, computed by `withholding_table`; the table itself
    /// says whether it is a 10,5- or a 12-month table.
    Table(i32),
    /// Frikort with a beløpsgrense for this employer: no trekk until the
    /// year's trekkpliktig pay reaches the limit, then 50 % of the excess.
    TaxFreeCard { limit_ore: i64 },
    /// Frikort without a limit, or otherwise exempt.
    None,
}

/// One employee's pay for one month.
#[derive(Debug, Clone)]
pub struct PayrollBasis {
    /// Ordinary pay for the month (fastlønn, timelønn, tillegg).
    pub gross_ore: i64,
    /// Feriepenger paid out this month. Kept separate because they are
    /// **trekkfrie** — see [`beregn`].
    pub holiday_pay_ore: i64,
    pub withholding: Withholding,
    /// Trekkpliktig pay this employer already paid the employee earlier
    /// in the income year (ordinary pay + feriepenger). Only a frikort
    /// reads it.
    pub paid_earlier_this_year_ore: i64,
    /// Kjøregodtgjørelse paid with this run: the part above the trekkfri
    /// rate is pay (trekkpliktig, triggers aga, earns no feriepenger),
    /// the rest is a reimbursement the employee simply gets.
    pub taxable_allowance_ore: i64,
    pub tax_free_allowance_ore: i64,
    /// The average monthly earnings December's half trekk reaches, from
    /// [`december_average`]. None withholds December in full.
    pub december_average_ore: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayrollCalculation {
    pub gross_ore: i64,
    pub holiday_pay_ore: i64,
    /// Base the withholding was actually computed on.
    pub withholding_basis_ore: i64,
    pub withholding_ore: i64,
    /// Kjøregodtgjørelse paid with the run, trekkfri and trekkpliktig.
    pub allowance_ore: i64,
    pub taxable_allowance_ore: i64,
    pub net_ore: i64,
    /// True when December's half withholding was applied, to all of the
    /// basis or to the part within the average (`method` then names it).
    pub half_withholding: bool,
    /// How the trekk was computed, for the payslip («35,00 %»,
    /// «tabell 8000», «frikort»). Norwegian: the employee reads it.
    pub method: String,
}

/// Forskuddstrekk for one month.
///
/// Two rules that look like generosity but are just timing — the
/// skattekort percentage is calculated over 10,5 months precisely so
/// these two hold, which is why applying them is correct rather than
/// double counting:
///
/// - **Feriepenger are trekkfrie** in the year they are paid.
/// - **December is half trekk** (skattebetalingsloven; since 2016 the
///   employer may instead take it in the second half of November —
///   regnmed does December, the common choice, and says so). Only pay
///   within the employee's average earnings is halved; the rest takes
///   full trekk (forskriften § 5-7-2 (4), [`december_average`]).
///
/// The trekk is rounded DOWN to whole kroner: the a-melding reports
/// forskuddstrekk in whole kroner (`amelding::whole_kroner` refuses
/// øre), and a run is insert-only, so a trekk in øre would leave a month
/// that can never be delivered. Down, so the employee is never withheld
/// more than the card says; the øre are settled by the skatteoppgjør.
pub fn calculate(
    basis: &PayrollBasis,
    year: i32,
    month: u32,
) -> Result<PayrollCalculation, PayrollError> {
    use crate::withholding_table::{Period, withholding_kr};
    let twelve_months = counts_all_holiday_pay(&basis.withholding)?;
    // The taxable allowance is trekkpliktig whatever the card: it is
    // not feriepenger, which are what the 10,5 months leave out.
    let trekkgrunnlag = basis.taxable_allowance_ore
        + if twelve_months {
            basis.gross_ore + basis.holiday_pay_ore
        } else {
            basis.gross_ore
        };
    let december_average = basis
        .december_average_ore
        .filter(|_| month == 12 && !twelve_months && basis.withholding != Withholding::None);
    let half_withholding = december_average.is_some();
    let within_average = december_average.map_or(0, |average| trekkgrunnlag.min(average));
    // Says how much was halved when it was not all of it.
    let halved_part = if half_withholding && within_average < trekkgrunnlag {
        format!(", halvt på {} kr", crate::payslip::kr(within_average))
    } else {
        String::new()
    };
    let (withholding, method) = match basis.withholding {
        Withholding::None => (0, "ingen trekk".to_string()),
        Withholding::Percent(bp) | Withholding::PercentTwelveMonths(bp) => {
            let months = if twelve_months { " (12 måneder)" } else { "" };
            (
                if half_withholding {
                    // Rounded up: an odd rate must not withhold less than half.
                    bp_of(within_average, bp - bp / 2) + bp_of(trekkgrunnlag - within_average, bp)
                } else {
                    bp_of(trekkgrunnlag, bp)
                },
                format!("{},{:02} %{months}{halved_part}", bp / 100, bp % 100),
            )
        }
        Withholding::Table(number) => {
            // The tables are in whole kroner; øre below a krone never
            // reach the next 100-krone step anyway.
            let lookup = |ore: i64| {
                withholding_kr(year, number, Period::Month, ore.max(0) / 100)
                    .map(|kr| kr * 100)
                    .map_err(PayrollError::Table)
            };
            let full = lookup(trekkgrunnlag)?;
            // The exemption is half the table's trekk on the pay within the
            // average -- exact in øre, since the table gives whole kroner.
            let w = if half_withholding {
                full - lookup(within_average)? / 2
            } else {
                full
            };
            (w, format!("tabell {number:04}{halved_part}"))
        }
        Withholding::TaxFreeCard { limit_ore } => {
            let left = (limit_ore - basis.paid_earlier_this_year_ore).max(0);
            let over = (trekkgrunnlag - left).max(0);
            if over == 0 {
                (0, "frikort".to_string())
            } else {
                let kr = |ore: i64| format!("{},{:02} kr", ore / 100, ore % 100);
                (
                    bp_of(over, FRIKORT_EXCESS_BP),
                    format!("frikort, 50 % av {} over beløpsgrensen", kr(over)),
                )
            }
        }
    };
    let withholding = withholding - withholding.rem_euclid(100);
    Ok(PayrollCalculation {
        gross_ore: basis.gross_ore,
        holiday_pay_ore: basis.holiday_pay_ore,
        withholding_basis_ore: trekkgrunnlag,
        withholding_ore: withholding,
        allowance_ore: basis.taxable_allowance_ore + basis.tax_free_allowance_ore,
        taxable_allowance_ore: basis.taxable_allowance_ore,
        net_ore: basis.gross_ore
            + basis.holiday_pay_ore
            + basis.taxable_allowance_ore
            + basis.tax_free_allowance_ore
            - withholding,
        half_withholding,
        method,
    })
}

/// Whether the card withholds on all holiday pay. A card spread over 10,5
/// months is what makes feriepenger trekkfrie and December half; a 12-month
/// card and a frikort count every krone.
pub fn counts_all_holiday_pay(withholding: &Withholding) -> Result<bool, PayrollError> {
    use crate::withholding_table::{Table, TableError};
    Ok(match *withholding {
        Withholding::PercentTwelveMonths(_) | Withholding::TaxFreeCard { .. } => true,
        Withholding::Table(number) => {
            Table::lookup(number)
                .ok_or(PayrollError::Table(TableError::UnknownTable(number)))?
                .twelve_months
        }
        Withholding::Percent(_) | Withholding::None => false,
    })
}

/// Why a December pay gets no half trekk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoDecemberHalf {
    /// § 5-7-2 (2) exempts two specific weeks, not half a monthly run.
    HourlyPay,
    /// The run cannot tell whether the amount is monthly pay.
    UnknownPayKind,
    /// No earlier pay to average, as § 5-7-2 (4) requires.
    NoEarlierPay,
    /// This payment year already carries one half: the card allows for one.
    AlreadyGiven,
}

/// The average monthly earnings December's half trekk reaches, or why it
/// does not apply. Where § 5-7-2 is silent this takes the side that cannot
/// leave the employee with restskatt (docs/lonn.md).
///
/// `earlier_ore` and `earlier_months` cover the year's earlier pay periods.
/// Rounded down, so the exemption never exceeds the average.
pub fn december_average(
    pay_kind: Option<&str>,
    earlier_ore: i64,
    earlier_months: i64,
    already_given: bool,
) -> Result<i64, NoDecemberHalf> {
    match pay_kind {
        Some("fixed") => {}
        Some(_) => return Err(NoDecemberHalf::HourlyPay),
        None => return Err(NoDecemberHalf::UnknownPayKind),
    }
    if already_given {
        return Err(NoDecemberHalf::AlreadyGiven);
    }
    if earlier_months <= 0 {
        return Err(NoDecemberHalf::NoEarlierPay);
    }
    Ok(earlier_ore / earlier_months)
}

/// Trekk on pay above a frikort's beløpsgrense (Skatteetaten: «trekke 50
/// prosent skatt av lønna» over grensen). Not halved in December and
/// taken on feriepenger too: where the sources are silent regnmed takes
/// the side that cannot leave the employee with restskatt.
const FRIKORT_EXCESS_BP: i64 = 5000;

/// Arbeidsgiveravgift on a basis, at the zone's rate for the date, in
/// whole kroner.
///
/// `rate_bp` comes from the satsregister — this function never knows a
/// rate itself. Sone Ia is refused; see [`PayrollError`].
///
/// Call it ONCE on the run's summed basis, not per employee: the claim
/// is grunnlag × sats over the whole basis (F185 in the a-melding), and
/// per-employee rounding would drift from it by up to a krone each.
pub fn employer_contribution(
    basis_ore: i64,
    zone: Zone,
    rate_bp: i64,
) -> Result<i64, PayrollError> {
    if zone == Zone::Ia {
        return Err(PayrollError::ZoneIaRequiresExemptionCalculation);
    }
    Ok(contribution_whole_kroner(basis_ore, rate_bp))
}

/// Basis × rate rounded DOWN to whole kroner, in øre: «Ved beregning av
/// skatte- og avgiftskrav avrundes beløpet ned til nærmeste hele krone»
/// (skattebetalingsforskriften § 10-4-4). Skatteetaten's own a-melding
/// guidance does the same: 12 250 kr × 14,1 % = 1 727,25 is reported as
/// 1 727. The a-melding wants whole kroner, so this is also what makes a
/// run reportable at all.
pub(crate) fn contribution_whole_kroner(basis_ore: i64, rate_bp: i64) -> i64 {
    // Integer division truncates toward zero: down for any real basis,
    // and a (never reported) negative one is not rounded up into a claim.
    (basis_ore as i128 * rate_bp as i128 / 1_000_000 * 100) as i64
}

/// Pay for logged hours: minutes at an hourly rate, in øre.
///
/// Minutes rather than hours because that is how the timesheet stores
/// them (docs/timer.md) — converting to fractional hours first would
/// introduce a rounding step that serves no one. One division, rounded
/// half away from zero, and the result is exact for whole hours.
pub fn hourly_pay(minutes: i64, hourly_rate_ore: i64) -> i64 {
    let n = minutes as i128 * hourly_rate_ore as i128;
    let d = 60i128;
    let sign = if (n < 0) != (d < 0) { -1 } else { 1 };
    (sign * ((n.abs() + d / 2) / d)) as i64
}

/// What påløpt arbeidsgiveravgift on unpaid feriepenger *should* be.
///
/// Feriepenger earned this year are paid next year, and the aga on them
/// falls due then — but the obligation arises with the earning, so the
/// cost belongs in the year it was earned.
///
/// This returns the **target balance**, not a movement: the accrual is
/// always `sats × skyldige feriepenger`, and a payroll run books the
/// difference between that and what is already accrued. Modelling it as
/// a target rather than a stream of increments is what makes it
/// self-correcting — a rate change, a payout, or feriepenger that
/// entered through an opening balance all resolve on the next run
/// instead of leaving a residue nobody can explain.
///
/// The rate is the current one, deliberately: the employer will pay the
/// avgift at the rate in force when the feriepenger are paid out, so the
/// current rate is the best estimate of the obligation, not an
/// approximation of an older one.
/// A negative skyldig — more feriepenger paid out than the payroll
/// history ever set aside, which happens when the liability came from an
/// opening balance — accrues **nothing** rather than a negative avgift.
/// There is no such thing as owing negative arbeidsgiveravgift, and
/// booking one would turn a bookkeeping gap into income.
pub fn employer_contribution_accrual_target(holiday_pay_owed_ore: i64, rate_bp: i64) -> i64 {
    bp_of(holiday_pay_owed_ore.max(0), rate_bp)
}

/// Feriepengeavsetning for the year's earnings (ferieloven §10).
///
/// The rate is data: 10,2 % by law, 12,5 % from the earning year before
/// the holiday year the employee turns 60 (§10 nr. 3 adds 2,3
/// prosentpoeng), and tariff agreements commonly raise it to 12 % / 14,3 %
/// for five weeks' holiday. Which one applies is the caller's decision,
/// recorded per employee. Above 6 G the caller subtracts
/// [`over_60_cap_cut`].
pub fn holiday_pay_accrual(basis_ore: i64, rate_bp: i64) -> i64 {
    bp_of(basis_ore, rate_bp)
}

/// The satsregister domain holding folketrygdens grunnbeløp, in øre.
pub const BASIC_AMOUNT_DOMAIN: &str = "folketrygdens_grunnbelop";

/// Ferieloven § 10 nr. 3 første ledd.
const OVER_60_ADDITION_BP: i64 = 230;

/// Whether the employee's rate carries the statutory over-60 addition in
/// this earning year. Someone turning 60 in holiday year Y earns it in Y-1,
/// the year they turn 59. Any other rate is an agreement whose parts we
/// cannot see, so the cap is not applied to it.
pub fn has_over_60_addition(rate_bp: i64, birth_year: i32, earning_year: i32) -> bool {
    RATES_WITH_OVER_60_ADDITION.contains(&rate_bp) && earning_year - birth_year >= 59
}

/// The ordinary rates plus the 2,3 points.
pub const RATES_WITH_OVER_60_ADDITION: [i64; 2] = [1250, 1430];

/// The date whose G caps the addition: 31 December of the earning year, or
/// the leaving date for someone who leaves during it (§ 10 nr. 3 andre ledd).
pub fn basic_amount_date(earning_year: i32, employed_to: Option<NaiveDate>) -> NaiveDate {
    use chrono::Datelike;
    employed_to
        .filter(|d| d.year() == earning_year)
        .or_else(|| NaiveDate::from_ymd_opt(earning_year, 12, 31))
        .expect("31 December exists in every year a payment date can have")
}

/// How much of the addition the 6 G cap removes from the earning year's
/// basis so far. Only the 2,3 points stop at 6 G: the ordinary rate under
/// § 10 nr. 2 or a tariff agreement still runs on all pay.
pub fn over_60_cap_cut(earning_year_basis_ore: i64, basic_amount_ore: i64) -> i64 {
    bp_of(
        (earning_year_basis_ore - 6 * basic_amount_ore).max(0),
        OVER_60_ADDITION_BP,
    )
}

/// The ordinary rates: 10,2 % by law and 12 % on tariff. Any other rate is an
/// agreement whose parts we cannot see.
pub const RATES_WITHOUT_OVER_60_ADDITION: [i64; 2] = [1020, 1200];

/// The rate the employee should be on when an ordinary rate is missing the
/// addition in this earning year (ferieloven § 10 nr. 3).
pub fn over_60_rate_due(rate_bp: i64, birth_year: i32, earning_year: i32) -> Option<i64> {
    (earning_year - birth_year >= 59 && RATES_WITHOUT_OVER_60_ADDITION.contains(&rate_bp))
        .then_some(rate_bp + OVER_60_ADDITION_BP)
}

/// The addition owed on pay accrued at an ordinary rate earlier in the
/// earning year. The right covers the whole year, so a raise catches it up.
pub fn over_60_catch_up(basis_without_addition_ore: i64) -> i64 {
    bp_of(basis_without_addition_ore, OVER_60_ADDITION_BP)
}

/// What a month's payroll costs and owes, per employee, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PayrollTotals {
    pub gross_ore: i64,
    pub holiday_pay_paid_ore: i64,
    pub withholding_ore: i64,
    /// Kjøregodtgjørelse paid with the run. Not a cost here: it was
    /// expensed when the claim was approved, and the run only settles
    /// the debt to the employee.
    pub allowance_ore: i64,
    pub net_ore: i64,
    pub holiday_pay_accrual_ore: i64,
    /// aga on this month's pay.
    pub employer_contribution_ore: i64,
    /// Change in the accrued aga on unpaid feriepenger — what this run
    /// posted to reach [`aga_avsetning_mal`]. Negative when feriepenger
    /// were paid out and the accrual is drawn back down.
    pub employer_contribution_holiday_pay_ore: i64,
}

impl PayrollTotals {
    pub fn total_employer_contribution_ore(&self) -> i64 {
        self.employer_contribution_ore + self.employer_contribution_holiday_pay_ore
    }

    /// What the run costs the employer.
    ///
    /// **Feriepenger paid out are not part of it.** They were expensed
    /// when they were earned and now only draw down the liability;
    /// counting them here would expense them twice. What the month
    /// genuinely costs is the ordinary pay plus everything accrued on
    /// it — the new feriepenger obligation and both avgifter.
    pub fn payroll_cost_ore(&self) -> i64 {
        self.gross_ore + self.holiday_pay_accrual_ore + self.total_employer_contribution_ore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basis(gross: i64, withholding: Withholding) -> PayrollBasis {
        PayrollBasis {
            gross_ore: gross,
            holiday_pay_ore: 0,
            withholding,
            paid_earlier_this_year_ore: 0,
            taxable_allowance_ore: 0,
            tax_free_allowance_ore: 0,
            december_average_ore: None,
        }
    }

    #[test]
    fn percentage_withholding_is_plain_percent_of_gross() {
        // 50 000 kr brutto, 35 % trekk.
        let b = calculate(&basis(5_000_000, Withholding::Percent(3500)), 2026, 3).unwrap();
        assert_eq!(b.withholding_ore, 1_750_000);
        assert_eq!(b.net_ore, 3_250_000);
        assert!(!b.half_withholding);
    }

    /// Feriepenger are paid without withholding — but they are still
    /// part of what the employee receives.
    #[test]
    fn holiday_pay_carries_no_withholding() {
        let g = PayrollBasis {
            gross_ore: 1_000_000,
            holiday_pay_ore: 4_000_000,
            withholding: Withholding::Percent(3500),
            paid_earlier_this_year_ore: 0,
            taxable_allowance_ore: 0,
            tax_free_allowance_ore: 0,
            december_average_ore: None,
        };
        let b = calculate(&g, 2026, 6).unwrap();
        assert_eq!(b.withholding_basis_ore, 1_000_000, "ordinary pay only");
        assert_eq!(b.withholding_ore, 350_000);
        assert_eq!(b.net_ore, 1_000_000 + 4_000_000 - 350_000);
    }

    /// Half tax in December. The skattekort percentage is calculated
    /// over 10,5 months precisely so that this works out.
    #[test]
    fn december_withholds_half() {
        let december = PayrollBasis {
            december_average_ore: Some(5_000_000),
            ..basis(5_000_000, Withholding::Percent(3500))
        };
        let b = calculate(&december, 2026, 12).unwrap();
        assert!(b.half_withholding);
        assert_eq!(b.withholding_ore, 875_000);
        assert_eq!(b.method, "35,00 %", "nothing to explain when all is halved");
        // November is an entirely ordinary month.
        let nov = calculate(&december, 2026, 11).unwrap();
        assert_eq!(nov.withholding_ore, 1_750_000);
        assert!(!nov.half_withholding);
        // Pay below the average is halved in full.
        let low = PayrollBasis {
            gross_ore: 3_000_000,
            ..december
        };
        assert_eq!(calculate(&low, 2026, 12).unwrap().withholding_ore, 525_000);
        // An odd rate rounds the halved rate up: 16,67 % of 30 000 kr.
        let odd = PayrollBasis {
            withholding: Withholding::Percent(3333),
            ..low
        };
        assert_eq!(calculate(&odd, 2026, 12).unwrap().withholding_ore, 500_100);
    }

    /// § 5-7-2 (4): a bonus is exempt only so far as the total stays within
    /// average earnings. 50 000 + 100 000 kr at 35 % used to withhold 26 250.
    #[test]
    fn december_halves_only_pay_within_the_average() {
        let bonus = PayrollBasis {
            december_average_ore: Some(5_000_000),
            ..basis(15_000_000, Withholding::Percent(3500))
        };
        let b = calculate(&bonus, 2026, 12).unwrap();
        assert!(b.half_withholding);
        assert_eq!(b.withholding_ore, 875_000 + 3_500_000);
        assert_eq!(
            b.method,
            format!("35,00 %, halvt på {} kr", crate::payslip::kr(5_000_000))
        );

        // On a table the exemption is half the trekk on the average.
        let table = PayrollBasis {
            withholding: Withholding::Table(8000),
            ..bonus
        };
        let kr = |pay| {
            crate::withholding_table::withholding_kr(
                2026,
                8000,
                crate::withholding_table::Period::Month,
                pay,
            )
            .unwrap()
                * 100
        };
        let t = calculate(&table, 2026, 12).unwrap();
        let expected = kr(150_000) - kr(50_000) / 2;
        assert_eq!(t.withholding_ore, expected - expected.rem_euclid(100));
    }

    #[test]
    fn december_without_an_average_withholds_in_full() {
        let b = calculate(&basis(5_000_000, Withholding::Percent(3500)), 2026, 12).unwrap();
        assert_eq!(b.withholding_ore, 1_750_000);
        assert!(!b.half_withholding);
    }

    /// No trekk is not a half trekk, so the payslip must not say it was.
    #[test]
    fn a_card_without_withholding_is_not_marked_as_half() {
        let none = PayrollBasis {
            december_average_ore: Some(5_000_000),
            ..basis(5_000_000, Withholding::None)
        };
        assert!(!calculate(&none, 2026, 12).unwrap().half_withholding);
    }

    #[test]
    fn who_gets_the_december_half() {
        use NoDecemberHalf::*;
        let fixed = Some("fixed");
        // 11 months of 50 000 kr, plus 11 000 kr in bonus over the year.
        assert_eq!(
            december_average(fixed, 56_100_000, 11, false),
            Ok(5_100_000)
        );
        // Rounded down, so the exemption never exceeds the average.
        assert_eq!(december_average(fixed, 1_000, 3, false), Ok(333));
        assert_eq!(
            december_average(Some("hourly"), 56_100_000, 11, false),
            Err(HourlyPay)
        );
        assert_eq!(
            december_average(None, 56_100_000, 11, false),
            Err(UnknownPayKind)
        );
        assert_eq!(december_average(fixed, 0, 0, false), Err(NoEarlierPay));
        assert_eq!(
            december_average(fixed, 56_100_000, 11, true),
            Err(AlreadyGiven)
        );
    }

    #[test]
    fn frikort_withholds_nothing() {
        let b = calculate(&basis(1_000_000, Withholding::None), 2026, 5).unwrap();
        assert_eq!(b.withholding_ore, 0);
        assert_eq!(b.net_ore, 1_000_000);
    }

    /// Tabelltrekk is Skatteetaten's own routine (`withholding_table`),
    /// in kroner, halved in December like prosenttrekk.
    #[test]
    fn tabelltrekk_follows_skatteetatens_table() {
        // Tabell 8000, 50 000 kr i måneden: the published 2026 table.
        let kr = crate::withholding_table::withholding_kr(
            2026,
            8000,
            crate::withholding_table::Period::Month,
            50_000,
        )
        .unwrap();
        let b = calculate(&basis(5_000_000, Withholding::Table(8000)), 2026, 3).unwrap();
        assert_eq!(b.withholding_ore, kr * 100);
        assert_eq!(b.net_ore, 5_000_000 - kr * 100);
        let december = PayrollBasis {
            december_average_ore: Some(5_000_000),
            ..basis(5_000_000, Withholding::Table(8000))
        };
        let dec = calculate(&december, 2026, 12).unwrap();
        assert_eq!(dec.withholding_ore, (kr * 100 / 2) / 100 * 100);
        assert!(dec.half_withholding);
    }

    /// What the table routine cannot stand behind is refused, not guessed.
    #[test]
    fn unknown_tables_and_years_are_refused() {
        assert!(calculate(&basis(5_000_000, Withholding::Table(7100)), 2026, 3).is_err());
        let error = calculate(&basis(5_000_000, Withholding::Table(8000)), 2027, 3).unwrap_err();
        assert!(error.to_string().contains("2027"), "{error}");
    }

    /// A 12-month card (kildeskatt): the same rate every month, also on
    /// feriepenger and in December.
    #[test]
    fn twelve_month_percent_withholds_on_holiday_pay_and_in_full_in_december() {
        let june = PayrollBasis {
            gross_ore: 1_000_000,
            holiday_pay_ore: 4_000_000,
            withholding: Withholding::PercentTwelveMonths(2220),
            paid_earlier_this_year_ore: 0,
            taxable_allowance_ore: 0,
            tax_free_allowance_ore: 0,
            december_average_ore: None,
        };
        let b = calculate(&june, 2026, 6).unwrap();
        assert_eq!(b.withholding_basis_ore, 5_000_000, "feriepenger included");
        assert_eq!(b.withholding_ore, 1_110_000);
        assert_eq!(b.method, "22,20 % (12 måneder)");
        let dec = calculate(
            &basis(5_000_000, Withholding::PercentTwelveMonths(2220)),
            2026,
            12,
        )
        .unwrap();
        assert_eq!(dec.withholding_ore, 1_110_000, "not halved");
        assert!(!dec.half_withholding);
    }

    /// A 12-month table is computed like any other, without the December
    /// half and on feriepenger.
    #[test]
    fn twelve_month_tables_are_computed_in_full() {
        let kr = crate::withholding_table::withholding_kr(
            2026,
            7350,
            crate::withholding_table::Period::Month,
            50_000,
        )
        .unwrap();
        let dec = calculate(&basis(5_000_000, Withholding::Table(7350)), 2026, 12).unwrap();
        assert_eq!(dec.withholding_ore, kr * 100);
        assert_eq!(dec.method, "tabell 7350");
        // Feriepenger are in the trekkgrunnlag of a 12-month table:
        // 40 000 lønn + 10 000 feriepenger looks up the same 50 000.
        let mut holiday = basis(4_000_000, Withholding::Table(7350));
        holiday.holiday_pay_ore = 1_000_000;
        let holiday = calculate(&holiday, 2026, 6).unwrap();
        assert_eq!(holiday.withholding_basis_ore, 5_000_000);
        assert_eq!(holiday.withholding_ore, kr * 100);
    }

    /// Kjøregodtgjørelse paid with pay: the part above the trekkfri rate
    /// is taxed like pay, on a 10,5-month card too, while the trekkfri part
    /// only reaches the net.
    #[test]
    fn the_taxable_allowance_is_withheld_and_the_rest_only_paid() {
        let mut b = basis(4_000_000, Withholding::Percent(3000));
        b.taxable_allowance_ore = 21_600;
        b.tax_free_allowance_ore = 42_000;
        let c = calculate(&b, 2026, 3).unwrap();
        assert_eq!(c.withholding_basis_ore, 4_021_600);
        // 30 % of 40 216 kr = 12 064,80 kr, rounded down to whole kroner.
        assert_eq!(c.withholding_ore, 1_206_400);
        assert_eq!(c.allowance_ore, 63_600);
        assert_eq!(c.taxable_allowance_ore, 21_600);
        assert_eq!(c.net_ore, 4_000_000 + 63_600 - 1_206_400);

        // A month with only the allowance still withholds on its taxable part.
        let mut only = basis(0, Withholding::Percent(3000));
        only.taxable_allowance_ore = 21_600;
        only.tax_free_allowance_ore = 42_000;
        let only = calculate(&only, 2026, 3).unwrap();
        assert_eq!(
            only.withholding_ore, 6_400,
            "30 % of 216 kr = 64,80 kr → 64 kr"
        );
        assert_eq!(only.net_ore, 63_600 - 6_400);
    }

    /// The a-melding takes forskuddstrekk in whole kroner, so the trekk
    /// is rounded down to one -- for every method.
    #[test]
    fn the_trekk_is_whole_kroner_rounded_down() {
        // 35 % of 41 001 kr = 14 350,35 kr.
        let percent = calculate(&basis(4_100_100, Withholding::Percent(3500)), 2026, 3).unwrap();
        assert_eq!(percent.withholding_ore, 1_435_000);
        assert_eq!(percent.net_ore, 4_100_100 - 1_435_000);
        // 1 kr over a frikort's limit: 50 % is 0,50 kr, which is nothing.
        let frikort = PayrollBasis {
            gross_ore: 3_000_100,
            holiday_pay_ore: 0,
            withholding: Withholding::TaxFreeCard {
                limit_ore: 3_000_000,
            },
            paid_earlier_this_year_ore: 0,
            taxable_allowance_ore: 0,
            tax_free_allowance_ore: 0,
            december_average_ore: None,
        };
        assert_eq!(calculate(&frikort, 2026, 3).unwrap().withholding_ore, 0);
    }

    /// Frikort: nothing until the limit, then 50 % of what exceeds it --
    /// counting what was paid earlier in the year, feriepenger included.
    #[test]
    fn frikort_withholds_half_of_what_exceeds_the_limit() {
        let frikort = |earlier: i64, gross: i64, holiday: i64| PayrollBasis {
            gross_ore: gross,
            holiday_pay_ore: holiday,
            withholding: Withholding::TaxFreeCard {
                limit_ore: 10_000_000,
            },
            paid_earlier_this_year_ore: earlier,
            taxable_allowance_ore: 0,
            tax_free_allowance_ore: 0,
            december_average_ore: None,
        };
        let under = calculate(&frikort(5_000_000, 2_000_000, 0), 2026, 5).unwrap();
        assert_eq!(under.withholding_ore, 0);
        assert_eq!(under.method, "frikort");
        // 9 000 kr left of the limit; 20 000 + 5 000 feriepenger paid.
        let crossing = calculate(&frikort(9_100_000, 2_000_000, 500_000), 2026, 6).unwrap();
        assert_eq!(crossing.withholding_ore, (2_500_000 - 900_000) / 2);
        assert!(
            crossing.method.contains("over beløpsgrensen"),
            "{}",
            crossing.method
        );
        // Past the limit: half of everything, December included.
        let over = calculate(&frikort(12_000_000, 2_000_000, 0), 2026, 12).unwrap();
        assert_eq!(over.withholding_ore, 1_000_000);
        // A limit of 0 for this employer: 50 % from the first krone.
        let zero = PayrollBasis {
            withholding: Withholding::TaxFreeCard { limit_ore: 0 },
            ..frikort(0, 2_000_000, 0)
        };
        assert_eq!(
            calculate(&zero, 2026, 3).unwrap().withholding_ore,
            1_000_000
        );
    }

    #[test]
    fn employer_contribution_is_the_rate_times_the_basis() {
        // Sone I: 14,1 % of 50 000 kr.
        assert_eq!(
            employer_contribution(5_000_000, Zone::I, 1410).unwrap(),
            705_000
        );
        // Sone V is a zero rate — an answer, not a missing rate.
        assert_eq!(employer_contribution(5_000_000, Zone::V, 0).unwrap(), 0);
    }

    #[test]
    fn employer_contribution_is_rounded_down_to_whole_kroner() {
        // Skatteetaten's own example: 12 250 kr × 14,1 % = 1 727,25 -> 1 727.
        assert_eq!(
            employer_contribution(1_225_000, Zone::I, 1410).unwrap(),
            172_700
        );
        // Down even at ,99: 7 092,19 kr × 14,1 % = 999,9988 -> 999.
        assert_eq!(
            employer_contribution(709_219, Zone::I, 1410).unwrap(),
            99_900
        );
    }

    #[test]
    fn sone_ia_is_refused_because_the_fribelop_is_invisible_here() {
        let error = employer_contribution(5_000_000, Zone::Ia, 1060).unwrap_err();
        assert_eq!(error, PayrollError::ZoneIaRequiresExemptionCalculation);
        assert!(error.to_string().contains("fribeløpet"), "{error}");
    }

    #[test]
    fn holiday_pay_by_ferieloven_and_by_tariff() {
        // §10: 10,2 % of the grunnlag.
        assert_eq!(holiday_pay_accrual(50_000_000, 1020), 5_100_000);
        // From the earning year before turning 60: +2,3 percentage points.
        assert_eq!(holiday_pay_accrual(50_000_000, 1250), 6_250_000);
        // Tariff agreement, five weeks.
        assert_eq!(holiday_pay_accrual(50_000_000, 1200), 6_000_000);
    }

    #[test]
    fn the_over_60_addition_stops_at_6g_and_the_ordinary_rate_does_not() {
        const G: i64 = 13_654_900;
        const SIX_G: i64 = 6 * G; // 819 294 kr
        let accrual = |basis, rate| holiday_pay_accrual(basis, rate) - over_60_cap_cut(basis, G);

        // Below and at 6 G: the whole raised rate.
        assert_eq!(accrual(SIX_G - 100, 1250), bp_of(SIX_G - 100, 1250));
        assert_eq!(over_60_cap_cut(SIX_G, G), 0);
        assert_eq!(accrual(SIX_G, 1250), bp_of(SIX_G, 1250));
        // Above: 10,2 % of 1 000 000 kr + 2,3 % of 819 294 kr.
        assert_eq!(accrual(100_000_000, 1250), 10_200_000 + 1_884_376);
        // Tariff: 12 % of all pay + the same capped addition.
        assert_eq!(accrual(100_000_000, 1430), 12_000_000 + 1_884_376);
    }

    #[test]
    fn the_addition_starts_in_the_earning_year_before_the_60th_birthday_year() {
        // Turns 60 in holiday year 2027, so 2026 pay carries the addition.
        assert!(!has_over_60_addition(1250, 1967, 2025));
        assert!(has_over_60_addition(1250, 1967, 2026));
        assert!(has_over_60_addition(1430, 1967, 2026));
        // 12 % has no addition to cap, at any age.
        assert!(!has_over_60_addition(1200, 1950, 2026));
        assert!(!has_over_60_addition(1020, 1950, 2026));
    }

    #[test]
    fn an_ordinary_rate_is_due_the_addition_from_the_year_the_employee_turns_59() {
        assert_eq!(over_60_rate_due(1020, 1968, 2026), None);
        assert_eq!(over_60_rate_due(1020, 1967, 2026), Some(1250));
        assert_eq!(over_60_rate_due(1200, 1950, 2026), Some(1430));
        // Already carries it, or an agreement we cannot read.
        assert_eq!(over_60_rate_due(1250, 1967, 2026), None);
        assert_eq!(over_60_rate_due(1430, 1967, 2026), None);
        assert_eq!(over_60_rate_due(1300, 1967, 2026), None);
    }

    #[test]
    fn g_is_taken_at_year_end_or_on_leaving_during_the_earning_year() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(basic_amount_date(2026, None), d(2026, 12, 31));
        assert_eq!(
            basic_amount_date(2026, Some(d(2026, 3, 31))),
            d(2026, 3, 31)
        );
        // Left in another year: did not leave "i opptjeningsåret".
        assert_eq!(
            basic_amount_date(2026, Some(d(2025, 12, 31))),
            d(2026, 12, 31)
        );
        assert_eq!(
            basic_amount_date(2026, Some(d(2027, 1, 31))),
            d(2026, 12, 31)
        );
    }

    #[test]
    fn hourly_wage_is_computed_from_minutes() {
        // 160 hours at 450 kr.
        assert_eq!(hourly_pay(160 * 60, 45_000), 7_200_000);
        // Half hours are exact.
        assert_eq!(hourly_pay(30, 45_000), 22_500);
        // An awkward minute count rounds half away from zero, once.
        // 7 min at 450 kr = 52,50 kr = 5250 øre.
        assert_eq!(hourly_pay(7, 45_000), 5_250);
        // 1 min at 100,01 kr → 166,68333… øre → 167.
        assert_eq!(hourly_pay(1, 10_001), 167);
        assert_eq!(hourly_pay(0, 45_000), 0);
    }

    #[test]
    fn rounding_is_half_away_from_zero_and_deterministic() {
        // 1234,55 kr * 10,2 % = 125,9241 -> 125,92
        assert_eq!(holiday_pay_accrual(123_455, 1020), 12_592);
        // Exactly one half rounds up in absolute value.
        assert_eq!(bp_of(50_000, 1), 5);
        assert_eq!(bp_of(-50_000, 1), -5);
        // The same input gives the same answer, always.
        assert_eq!(bp_of(123_455, 1020), bp_of(123_455, 1020));
    }

    #[test]
    fn sone_slug_round_trips() {
        for zone in [
            Zone::I,
            Zone::Ia,
            Zone::II,
            Zone::III,
            Zone::IV,
            Zone::IVa,
            Zone::V,
        ] {
            assert_eq!(Zone::from_slug(zone.slug()), Some(zone));
        }
        assert_eq!(Zone::from_slug("VI"), None);
        assert_eq!(Zone::I.rate_domain(), "aga_sone_i");
        assert_eq!(Zone::IVa.rate_domain(), "aga_sone_iva");
    }

    #[test]
    fn lonnssum_sums_cost_and_avgift() {
        let sum = PayrollTotals {
            gross_ore: 5_000_000,
            holiday_pay_paid_ore: 1_000_000,
            withholding_ore: 1_750_000,
            allowance_ore: 0,
            net_ore: 4_250_000,
            holiday_pay_accrual_ore: 510_000,
            employer_contribution_ore: 705_000,
            employer_contribution_holiday_pay_ore: 71_910,
        };
        assert_eq!(sum.total_employer_contribution_ore(), 776_910);
        // The cost is ordinary pay plus what accrues on it. The
        // feriepenger paid out are NOT included: they were expensed in
        // the year they were earned, and counting them here would
        // expense them twice.
        assert_eq!(
            sum.payroll_cost_ore(),
            5_000_000 + 510_000 + 705_000 + 71_910
        );
    }

    /// The accrual is a TARGET, not a stream of increments — which is
    /// why it cannot drift.
    #[test]
    fn aga_accrual_is_the_rate_times_holiday_pay_owed() {
        // 51 000 kr of feriepenger owed, sone I at 14,1 %.
        assert_eq!(
            employer_contribution_accrual_target(5_100_000, 1410),
            719_100
        );
        // No liability, no accrual.
        assert_eq!(employer_contribution_accrual_target(0, 1410), 0);
        // Sone V is a zero rate all the way through.
        assert_eq!(employer_contribution_accrual_target(5_100_000, 0), 0);
    }

    /// If more feriepenger are paid out than the payroll history ever
    /// set aside, the liability came from somewhere else — and then
    /// nothing is accrued. A negative avgift does not exist, and booking
    /// one would turn a gap in the books into income.
    #[test]
    fn negative_liability_accrues_nothing_rather_than_a_negative_avgift() {
        assert_eq!(employer_contribution_accrual_target(-3_694_000, 1410), 0);
    }

    /// The life of one feriepenge krone: accrued, held, paid out. What
    /// each run books is the target minus what already stands — and the
    /// sum across the whole life is zero.
    #[test]
    fn the_accrual_builds_up_and_draws_back_down_to_zero() {
        let rate = 1410;
        // Year 1: 51 000 kr of feriepenger earned, nothing accrued before.
        let mal1 = employer_contribution_accrual_target(5_100_000, rate);
        let delta1 = mal1 - 0;
        assert_eq!(delta1, 719_100);

        // Year 2, half is paid out: the liability is now 25 500 kr.
        let mal2 = employer_contribution_accrual_target(2_550_000, rate);
        let delta2 = mal2 - mal1;
        assert!(delta2 < 0, "a payout draws the accrual down");

        // The rest is paid out: the liability is zero, and so is the accrual.
        let mal3 = employer_contribution_accrual_target(0, rate);
        let delta3 = mal3 - mal2;
        assert_eq!(mal3, 0);
        assert_eq!(delta1 + delta2 + delta3, 0, "the whole life sums to zero");
    }

    /// A rate change between earning and payout corrects itself on the
    /// next run instead of lingering as a residue.
    #[test]
    fn a_rate_change_corrects_itself_on_the_next_run() {
        let accrued = employer_contribution_accrual_target(5_100_000, 1410);
        // The rate is lowered the year after; the liability is unchanged.
        let mal = employer_contribution_accrual_target(5_100_000, 1400);
        let delta = mal - accrued;
        assert_eq!(delta, -5_100, "the difference is exactly the rate change");
        assert_eq!(accrued + delta, mal, "and afterwards the accrual is right");
    }
}
