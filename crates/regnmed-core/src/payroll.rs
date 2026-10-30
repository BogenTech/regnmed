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
//! - **Tabelltrekk.** The withholding tables are Skatteetaten's data
//!   files, and without them there is no defensible way to compute
//!   tabelltrekk. We refuse loudly instead of approximating — withholding
//!   too little is the employee's restskatt.
//! - **Sone Ia.** The reduced rate applies only until the fribeløp is
//!   used up, and that fribeløp is de minimis aid which can also be
//!   consumed by things regnmed cannot see. Computing 10,6 % without the
//!   whole picture would under-report avgift.
//!
//! The rates themselves are data in the satsregister
//! (docs/regelverk.md), not numbers in this code.

use chrono::NaiveDate;

/// Rounds a basis-point calculation half away from zero.
fn bp_av(amount_ore: i64, rate_bp: i64) -> i64 {
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
    /// Tabelltrekk needs Skatteetatens published tables.
    TableWithholdingUnsupported(i32),
    /// Sone Ia's reduced rate is bounded by a yearly fribeløp.
    ZoneIaRequiresExemptionCalculation,
    /// The satsregister has no verified rate covering this date.
    MissingRate(String, NaiveDate),
}

impl std::fmt::Display for PayrollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TableWithholdingUnsupported(table) => write!(
                f,
                "tabelltrekk (tabell {table}) er ikke støttet: trekktabellene er \
                 Skatteetatens datafiler, og regnmed tilnærmer dem ikke — bruk \
                 prosenttrekk fra skattekortet inntil tabellene er vendored \
                 (se docs/lonn.md)"
            ),
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
    /// Percentage in basis points (35 % = 3500).
    Percent(i64),
    /// Table number — refused, see [`LonnError::TabelltrekkIkkeStottet`].
    Table(i32),
    /// Frikort or otherwise exempt.
    Ingen,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayrollCalculation {
    pub gross_ore: i64,
    pub holiday_pay_ore: i64,
    /// Base the withholding was actually computed on.
    pub withholding_basis_ore: i64,
    pub withholding_ore: i64,
    pub net_ore: i64,
    /// True when December's half withholding was applied.
    pub half_withholding: bool,
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
///   regnmed does December, the common choice, and says so).
///
/// Rounding is half away from zero, on the month's whole trekkgrunnlag.
pub fn calculate(basis: &PayrollBasis, month: u32) -> Result<PayrollCalculation, PayrollError> {
    let rate_bp = match basis.withholding {
        Withholding::Table(nr) => return Err(PayrollError::TableWithholdingUnsupported(nr)),
        Withholding::Ingen => 0,
        Withholding::Percent(bp) => bp,
    };

    // Feriepenger never carry trekk; only ordinary pay does.
    let trekkgrunnlag = basis.gross_ore;
    let half_withholding = month == 12;
    let effektiv_bp = if half_withholding {
        rate_bp / 2
    } else {
        rate_bp
    };
    let forskuddstrekk = bp_av(trekkgrunnlag, effektiv_bp);

    Ok(PayrollCalculation {
        gross_ore: basis.gross_ore,
        holiday_pay_ore: basis.holiday_pay_ore,
        withholding_basis_ore: trekkgrunnlag,
        withholding_ore: forskuddstrekk,
        net_ore: basis.gross_ore + basis.holiday_pay_ore - forskuddstrekk,
        half_withholding,
    })
}

/// Arbeidsgiveravgift on a basis, at the zone's rate for the date.
///
/// `sats_bp` comes from the satsregister — this function never knows a
/// rate itself. Sone Ia is refused; see [`LonnError`].
pub fn employer_contribution(
    basis_ore: i64,
    zone: Zone,
    rate_bp: i64,
) -> Result<i64, PayrollError> {
    if zone == Zone::Ia {
        return Err(PayrollError::ZoneIaRequiresExemptionCalculation);
    }
    Ok(bp_av(basis_ore, rate_bp))
}

/// Pay for logged hours: minutes at an hourly rate, in øre.
///
/// Minutes rather than hours because that is how the timesheet stores
/// them (docs/timer.md) — converting to fractional hours first would
/// introduce a rounding step that serves no one. One division, rounded
/// half away from zero, and the result is exact for whole hours.
pub fn hourly_pay(minutes: i64, timesats_ore: i64) -> i64 {
    let n = minutes as i128 * timesats_ore as i128;
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
    bp_av(holiday_pay_owed_ore.max(0), rate_bp)
}

/// Feriepengeavsetning for the year's earnings (ferieloven §10).
///
/// The rate is data: 10,2 % by law, 12,5 % from the year the employee
/// turns 60 (§10 nr. 3 adds 2,3 prosentpoeng), and tariff agreements
/// commonly raise it to 12 % / 14,3 % for five weeks' holiday. Which
/// one applies is the caller's decision, recorded per employee.
pub fn holiday_pay_accrual(basis_ore: i64, rate_bp: i64) -> i64 {
    bp_av(basis_ore, rate_bp)
}

/// What a month's payroll costs and owes, per employee, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PayrollTotals {
    pub gross_ore: i64,
    pub feriepenger_utbetalt_ore: i64,
    pub withholding_ore: i64,
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

    fn basis(brutto: i64, withholding: Withholding) -> PayrollBasis {
        PayrollBasis {
            gross_ore: brutto,
            holiday_pay_ore: 0,
            withholding,
        }
    }

    #[test]
    fn percentage_withholding_is_plain_percent_of_brutto() {
        // 50 000 kr brutto, 35 % trekk.
        let b = calculate(&basis(5_000_000, Withholding::Percent(3500)), 3).unwrap();
        assert_eq!(b.withholding_ore, 1_750_000);
        assert_eq!(b.net_ore, 3_250_000);
        assert!(!b.half_withholding);
    }

    /// Feriepenger are paid without withholding — but they are still
    /// part of what the employee receives.
    #[test]
    fn feriepenger_carry_no_withholding() {
        let g = PayrollBasis {
            gross_ore: 1_000_000,
            holiday_pay_ore: 4_000_000,
            withholding: Withholding::Percent(3500),
        };
        let b = calculate(&g, 6).unwrap();
        assert_eq!(b.withholding_basis_ore, 1_000_000, "ordinary pay only");
        assert_eq!(b.withholding_ore, 350_000);
        assert_eq!(b.net_ore, 1_000_000 + 4_000_000 - 350_000);
    }

    /// Half tax in December. The skattekort percentage is calculated
    /// over 10,5 months precisely so that this works out.
    #[test]
    fn december_withholds_half() {
        let b = calculate(&basis(5_000_000, Withholding::Percent(3500)), 12).unwrap();
        assert!(b.half_withholding);
        assert_eq!(b.withholding_ore, 875_000);
        // November is an entirely ordinary month.
        let nov = calculate(&basis(5_000_000, Withholding::Percent(3500)), 11).unwrap();
        assert_eq!(nov.withholding_ore, 1_750_000);
    }

    #[test]
    fn frikort_withholds_nothing() {
        let b = calculate(&basis(1_000_000, Withholding::Ingen), 5).unwrap();
        assert_eq!(b.withholding_ore, 0);
        assert_eq!(b.net_ore, 1_000_000);
    }

    /// The honest refusal: without Skatteetaten's tables we do not
    /// compute tabelltrekk, we say so.
    #[test]
    fn tabelltrekk_is_refused_loudly() {
        let feil = calculate(&basis(5_000_000, Withholding::Table(7100)), 3).unwrap_err();
        assert_eq!(feil, PayrollError::TableWithholdingUnsupported(7100));
        assert!(feil.to_string().contains("tilnærmer dem ikke"), "{feil}");
    }

    #[test]
    fn aga_is_the_rate_times_the_grunnlag() {
        // Sone I: 14,1 % of 50 000 kr.
        assert_eq!(
            employer_contribution(5_000_000, Zone::I, 1410).unwrap(),
            705_000
        );
        // Sone V is a zero rate — an answer, not a missing rate.
        assert_eq!(employer_contribution(5_000_000, Zone::V, 0).unwrap(), 0);
    }

    #[test]
    fn sone_ia_is_refused_because_the_fribelop_is_invisible_here() {
        let feil = employer_contribution(5_000_000, Zone::Ia, 1060).unwrap_err();
        assert_eq!(feil, PayrollError::ZoneIaRequiresExemptionCalculation);
        assert!(feil.to_string().contains("fribeløpet"), "{feil}");
    }

    #[test]
    fn feriepenger_by_ferieloven_and_by_tariff() {
        // §10: 10,2 % of the grunnlag.
        assert_eq!(holiday_pay_accrual(50_000_000, 1020), 5_100_000);
        // From the year the employee turns 60: +2,3 percentage points.
        assert_eq!(holiday_pay_accrual(50_000_000, 1250), 6_250_000);
        // Tariff agreement, five weeks.
        assert_eq!(holiday_pay_accrual(50_000_000, 1200), 6_000_000);
    }

    #[test]
    fn timelonn_is_computed_from_minutes() {
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
        assert_eq!(bp_av(50_000, 1), 5);
        assert_eq!(bp_av(-50_000, 1), -5);
        // The same input gives the same answer, always.
        assert_eq!(bp_av(123_455, 1020), bp_av(123_455, 1020));
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
            feriepenger_utbetalt_ore: 1_000_000,
            withholding_ore: 1_750_000,
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
    fn aga_accrual_is_the_rate_times_feriepenger_owed() {
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
        let sats = 1410;
        // Year 1: 51 000 kr of feriepenger earned, nothing accrued before.
        let mal1 = employer_contribution_accrual_target(5_100_000, sats);
        let delta1 = mal1 - 0;
        assert_eq!(delta1, 719_100);

        // Year 2, half is paid out: the liability is now 25 500 kr.
        let mal2 = employer_contribution_accrual_target(2_550_000, sats);
        let delta2 = mal2 - mal1;
        assert!(delta2 < 0, "a payout draws the accrual down");

        // The rest is paid out: the liability is zero, and so is the accrual.
        let mal3 = employer_contribution_accrual_target(0, sats);
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
