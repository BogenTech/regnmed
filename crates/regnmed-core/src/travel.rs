//! Diett on business travel, pure side (docs/utlegg.md «Reise og diett»).
//!
//! A trip is cut into døgn of 24 hours counted from departure. Each døgn
//! gets a kost allowance, and a night in private lodging in Norway an
//! ulegitimert nattillegg. Two rate sets meet on every line:
//!
//! - the TREKKFRI rates (satsforskriften 2026, FOR-2025-11-07-2216 §§ 2-3):
//!   what may be paid without forskuddstrekk and arbeidsgiveravgift, the
//!   same in Norway and abroad;
//! - what the company PAYS: either those same rates ([`Scheme::TaxFree`]),
//!   or the state's (særavtale reise innenlands § 9-10 and utenfor Norge
//!   § 8, [`Scheme::State`]), whose excess over the trekkfri rate is
//!   trekkpliktig and goes through payroll.
//!
//! All amounts are integer øre; the rates come from the dated satsregister
//! and the state's country table, never from here. A foreign rate arrives
//! already converted to NOK for the døgn it applies to.

use chrono::NaiveDateTime;

/// Whose rates the company pays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Skatteetaten's trekkfri rates: nothing is trekkpliktig.
    TaxFree,
    /// The state's rates; the excess over the trekkfri rate is trekkpliktig.
    State,
}

impl Scheme {
    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "trekkfri" => Some(Self::TaxFree),
            "statens" => Some(Self::State),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TaxFree => "trekkfri",
            Self::State => "statens",
        }
    }
}

/// Where the traveller slept — it decides the trekkfri kost rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lodging {
    /// Hotel or the like.
    Hotel,
    /// Hybel without cooking facilities, pensjonat or brakke.
    Pension,
    /// Hybel with cooking facilities, or a private overnight stay.
    Private,
}

impl Lodging {
    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "hotell" => Some(Self::Hotel),
            "pensjonat" => Some(Self::Pension),
            "privat" => Some(Self::Private),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Hotel => "hotell",
            Self::Pension => "pensjonat",
            Self::Private => "privat",
        }
    }
}

/// Meals someone else paid for (the hotel breakfast, a dinner at the
/// conference) — each reduces the kost allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Meals {
    pub breakfast: bool,
    pub lunch: bool,
    pub dinner: bool,
}

/// Where a døgn was spent: the country where the traveller was longest
/// (særavtale utenfor Norge § 8 nr. 5, Norway included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Norway,
    /// The state's kost rate for the country or city, in NOK øre on the
    /// døgn's date.
    Abroad {
        state_rate_ore: i64,
    },
}

/// What the traveller says about one døgn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayInput {
    pub place: Place,
    /// The night in this døgn; None for the last, part døgn and a day trip.
    pub lodging: Option<Lodging>,
    pub meals: Meals,
}

/// Satsforskriften §§ 2-3, øre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxFreeRates {
    pub hotel: i64,
    pub pension: i64,
    pub private: i64,
    pub day_6_12: i64,
    pub day_over_12: i64,
    pub night: i64,
}

impl TaxFreeRates {
    fn kost(&self, lodging: Lodging) -> i64 {
        match lodging {
            Lodging::Hotel => self.hotel,
            Lodging::Pension => self.pension,
            Lodging::Private => self.private,
        }
    }
}

impl TaxFreeRates {
    /// The register's rates on `date`; Err names the first domain it
    /// lacks.
    pub fn on(rates: &[crate::rates::RatePeriod], date: chrono::NaiveDate) -> Result<Self, String> {
        let v = |d: &str| crate::rates::value_on(rates, d, date).ok_or_else(|| d.to_string());
        Ok(Self {
            hotel: v("diett_hotell_trekkfri")?,
            pension: v("diett_pensjonat_trekkfri")?,
            private: v("diett_privat_trekkfri")?,
            day_6_12: v("diett_dag_6_12_trekkfri")?,
            day_over_12: v("diett_dag_over_12_trekkfri")?,
            night: v("nattillegg_trekkfri")?,
        })
    }
}

impl StateRates {
    pub fn on(rates: &[crate::rates::RatePeriod], date: chrono::NaiveDate) -> Result<Self, String> {
        let v = |d: &str| crate::rates::value_on(rates, d, date).ok_or_else(|| d.to_string());
        Ok(Self {
            day_6_12: v("diett_dag_6_12")?,
            day_over_12: v("diett_dag_over_12")?,
            overnight: v("diett_overnatting")?,
            night: v("nattillegg")?,
        })
    }
}

/// Særavtale reise innenlands §§ 9-10, øre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRates {
    pub day_6_12: i64,
    pub day_over_12: i64,
    pub overnight: i64,
    pub night: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayKind {
    /// A full døgn with a night in it (or the only døgn of a short trip
    /// with an overnight stay).
    Overnight,
    /// What is left after the last full døgn, 6 hours or more.
    Remainder,
    /// A trip without an overnight stay.
    DayTrip,
}

/// One døgn of a trip, as paid and as reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayLine {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub kind: DayKind,
    /// The lodging the trekkfri rate follows: the night's own, and for
    /// the remainder the last night's.
    pub lodging: Option<Lodging>,
    pub place: Place,
    pub meals: Meals,
    pub kost_paid_ore: i64,
    pub kost_tax_free_ore: i64,
    pub night_paid_ore: i64,
    pub night_tax_free_ore: i64,
}

impl DayLine {
    pub fn paid_ore(&self) -> i64 {
        self.kost_paid_ore + self.night_paid_ore
    }

    pub fn tax_free_ore(&self) -> i64 {
        self.kost_tax_free_ore + self.night_tax_free_ore
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TravelAllowance {
    pub lines: Vec<DayLine>,
    pub amount_ore: i64,
    pub tax_free_ore: i64,
    pub taxable_ore: i64,
}

/// Why a trip gives no allowance as entered — said the way the traveller
/// reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TravelError {
    ReturnBeforeDeparture,
    UnderSixHours,
    DayTripOverADay,
    NoNightOnDayTrip,
    OverTwentyEightDays,
    DayCount { expected: usize, got: usize },
    NoLodging { day: usize },
}

impl std::fmt::Display for TravelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReturnBeforeDeparture => write!(f, "hjemkomst må være etter avreise"),
            Self::UnderSixHours => write!(f, "en reise under 6 timer gir ingen diett"),
            Self::DayTripOverADay => write!(
                f,
                "en reise uten overnatting kan ikke vare over et døgn — oppgi overnattingene"
            ),
            Self::NoNightOnDayTrip => write!(f, "en dagreise har ingen overnatting"),
            Self::OverTwentyEightDays => write!(
                f,
                "reiser over 28 døgn på samme sted følger egne regler, som regnmed ikke har \
                 bygget ennå — del opp eller før kravet utenom"
            ),
            Self::DayCount { expected, got } => write!(
                f,
                "reisen varer {expected} døgn, men {got} døgn er fylt ut — lagre på nytt"
            ),
            Self::NoLodging { day } => write!(f, "velg hvor du overnattet i døgn {day}"),
        }
    }
}

impl std::error::Error for TravelError {}

const HOUR: i64 = 60;
const DAY: i64 = 24 * HOUR;

/// The trip's døgn: 24 hours from departure each. With an overnight stay
/// a remainder of 6 hours or more after the last full døgn is a døgn of
/// its own, and less than 6 hours gives nothing (satsforskriften § 2,
/// særavtalene § 8/§ 9). Without one, the trip is one day trip. Under 6
/// hours gives nothing either way.
pub fn day_count(
    departure: NaiveDateTime,
    return_at: NaiveDateTime,
    overnight: bool,
) -> Result<usize, TravelError> {
    let minutes = (return_at - departure).num_minutes();
    if minutes <= 0 {
        return Err(TravelError::ReturnBeforeDeparture);
    }
    if minutes < 6 * HOUR {
        return Err(TravelError::UnderSixHours);
    }
    if !overnight {
        if minutes > DAY {
            return Err(TravelError::DayTripOverADay);
        }
        return Ok(1);
    }
    let full = (minutes / DAY) as usize;
    let rest = minutes % DAY;
    let count = if full == 0 {
        1
    } else {
        full + usize::from(rest >= 6 * HOUR)
    };
    if count > 28 {
        return Err(TravelError::OverTwentyEightDays);
    }
    Ok(count)
}

/// Rounds øre to the nearest whole krone (satsforskriften: «rundes av til
/// nærmeste hele krone»). Never negative here.
fn whole_krone(ore: i64) -> i64 {
    (ore + 50) / 100 * 100
}

/// The kost after the meals someone else paid: 20 % for breakfast, 30 %
/// for lunch, 50 % for dinner of the rate, each reduction rounded to whole
/// kroner, never below zero. `breakfast_counts` is false where the state
/// pays ulegitimert nattillegg (særavtale innenlands § 9 nr. 4).
fn after_meals(rate: i64, meals: Meals, breakfast_counts: bool) -> i64 {
    let mut reduction = 0;
    if meals.breakfast && breakfast_counts {
        reduction += whole_krone(rate * 20 / 100);
    }
    if meals.lunch {
        reduction += whole_krone(rate * 30 / 100);
    }
    if meals.dinner {
        reduction += whole_krone(rate * 50 / 100);
    }
    (rate - reduction).max(0)
}

/// The allowance for a whole trip.
pub fn allowance(
    departure: NaiveDateTime,
    return_at: NaiveDateTime,
    overnight: bool,
    days: &[DayInput],
    scheme: Scheme,
    tax_free: &TaxFreeRates,
    state: &StateRates,
) -> Result<TravelAllowance, TravelError> {
    let count = day_count(departure, return_at, overnight)?;
    if days.len() != count {
        return Err(TravelError::DayCount {
            expected: count,
            got: days.len(),
        });
    }
    let total = (return_at - departure).num_minutes();
    let full = (total / DAY) as usize;
    let mut lines = Vec::with_capacity(count);
    let mut last_lodging = None;
    for (i, day) in days.iter().enumerate() {
        let start = departure + chrono::Duration::minutes(i as i64 * DAY);
        let end = (start + chrono::Duration::minutes(DAY)).min(return_at);
        let minutes = (end - start).num_minutes();
        // The part døgn after the last full one may hold a night too
        // (out in the evening, home the morning after next): with a
        // lodging it is a døgn with an overnight stay like the others.
        let kind = if !overnight {
            DayKind::DayTrip
        } else if i < full || full == 0 || day.lodging.is_some() {
            DayKind::Overnight
        } else {
            DayKind::Remainder
        };
        let lodging = match kind {
            DayKind::Overnight => {
                let l = day.lodging.ok_or(TravelError::NoLodging { day: i + 1 })?;
                last_lodging = Some(l);
                Some(l)
            }
            DayKind::Remainder => last_lodging,
            DayKind::DayTrip => {
                if day.lodging.is_some() {
                    return Err(TravelError::NoNightOnDayTrip);
                }
                None
            }
        };

        // Trekkfri: the lodging's rate for a døgn with a night or the part
        // døgn after it; the hour band for a day trip. Same abroad.
        let tax_free_base = match (kind, lodging) {
            (DayKind::DayTrip, _) if minutes <= 12 * HOUR => tax_free.day_6_12,
            (DayKind::DayTrip, _) => tax_free.day_over_12,
            (_, Some(l)) => tax_free.kost(l),
            (_, None) => unreachable!("an overnight trip has a lodging"),
        };
        let kost_tax_free = after_meals(tax_free_base, day.meals, true);

        // The nattillegg: the state's, for a private night in Norway.
        let night = scheme == Scheme::State
            && kind == DayKind::Overnight
            && day.place == Place::Norway
            && lodging == Some(Lodging::Private);
        let (night_paid, night_tax_free) = if night {
            // Over the trekkfri amount the WHOLE nattillegg is trekkpliktig
            // (Skatteetaten, «Trekkfritt ulegitimert nattillegg»).
            let free = if state.night <= tax_free.night {
                state.night
            } else {
                0
            };
            (state.night, free)
        } else {
            (0, 0)
        };

        let kost_paid = match scheme {
            Scheme::TaxFree => kost_tax_free,
            Scheme::State => {
                let base = match (day.place, kind) {
                    // «Over 12 timer med overnatting» (innenlands § 9 nr. 1
                    // a); a shorter one falls in the 6-12 band.
                    (Place::Norway, DayKind::Overnight) if minutes > 12 * HOUR => state.overnight,
                    // The part døgn and a day trip by the hour bands
                    // (innenlands § 9 nr. 1 b-c).
                    (Place::Norway, _) if minutes <= 12 * HOUR => state.day_6_12,
                    (Place::Norway, _) => state.day_over_12,
                    (Place::Abroad { state_rate_ore }, DayKind::Overnight) => state_rate_ore,
                    // Half the country's rate from 6 to under 12 hours,
                    // all of it from 12 (utenfor Norge § 8 nr. 2 b-c).
                    (Place::Abroad { state_rate_ore }, _) if minutes < 12 * HOUR => {
                        (state_rate_ore + 1) / 2
                    }
                    (Place::Abroad { state_rate_ore }, _) => state_rate_ore,
                };
                after_meals(base, day.meals, !night)
            }
        };

        lines.push(DayLine {
            start,
            end,
            kind,
            lodging,
            place: day.place,
            meals: day.meals,
            kost_paid_ore: kost_paid,
            kost_tax_free_ore: kost_tax_free.min(kost_paid),
            night_paid_ore: night_paid,
            night_tax_free_ore: night_tax_free,
        });
    }
    let amount: i64 = lines.iter().map(DayLine::paid_ore).sum();
    let tax_free_total: i64 = lines.iter().map(DayLine::tax_free_ore).sum();
    Ok(TravelAllowance {
        lines,
        amount_ore: amount,
        tax_free_ore: tax_free_total,
        taxable_ore: amount - tax_free_total,
    })
}

/// The a-melding lines a paid trip reports: per beskrivelse, the amount
/// and the number of døgn (`antall`). Skatteetaten's inntektskombinasjoner
/// R1 2026, all `utgiftsgodtgjoerelse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Report {
    KostHotel,
    KostPension,
    KostPrivate,
    KostDayTrip,
    Night,
    /// Kost above the trekkfri rate: trekkpliktig, with avgift; no antall.
    KostTaxable,
    /// A nattillegg above the trekkfri amount, whole.
    OtherTaxable,
}

impl Report {
    /// Trekkpliktig and avgiftspliktig (Skatteetaten: «reiseKost»,
    /// «reiseAnnet» trigger both).
    pub fn is_taxable(self) -> bool {
        matches!(self, Self::KostTaxable | Self::OtherTaxable)
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.slug() == s)
    }

    pub const ALL: [Self; 7] = [
        Self::KostHotel,
        Self::KostPension,
        Self::KostPrivate,
        Self::KostDayTrip,
        Self::Night,
        Self::KostTaxable,
        Self::OtherTaxable,
    ];

    /// The name stored on payroll rows.
    pub fn slug(self) -> &'static str {
        match self {
            Self::KostHotel => "kost_hotell",
            Self::KostPension => "kost_pensjonat",
            Self::KostPrivate => "kost_privat",
            Self::KostDayTrip => "kost_dagreise",
            Self::Night => "nattillegg",
            Self::KostTaxable => "kost_trekkpliktig",
            Self::OtherTaxable => "annet_trekkpliktig",
        }
    }
}

/// (report, amount, døgn) per line of the trip, zero amounts left out.
pub fn report_lines(lines: &[DayLine]) -> Vec<(Report, i64, i64)> {
    let mut out = Vec::new();
    for l in lines {
        let kost_report = match (l.kind, l.lodging) {
            (DayKind::DayTrip, _) => Report::KostDayTrip,
            (_, Some(Lodging::Hotel)) => Report::KostHotel,
            (_, Some(Lodging::Pension)) => Report::KostPension,
            (_, Some(Lodging::Private) | None) => Report::KostPrivate,
        };
        out.push((kost_report, l.kost_tax_free_ore, 1));
        out.push((
            Report::KostTaxable,
            l.kost_paid_ore - l.kost_tax_free_ore,
            0,
        ));
        out.push((Report::Night, l.night_tax_free_ore, 1));
        out.push((
            Report::OtherTaxable,
            l.night_paid_ore - l.night_tax_free_ore,
            0,
        ));
    }
    out.retain(|(_, amount, _)| *amount > 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    // 2026: satsforskriften §§ 2-3, særavtale innenlands §§ 9-10.
    const TAX_FREE: TaxFreeRates = TaxFreeRates {
        hotel: 69_300,
        pension: 40_000,
        private: 10_700,
        day_6_12: 20_000,
        day_over_12: 40_000,
        night: 45_200,
    };
    const STATE: StateRates = StateRates {
        day_6_12: 39_700,
        day_over_12: 73_600,
        overnight: 101_200,
        night: 45_200,
    };

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn norway(lodging: Option<Lodging>) -> DayInput {
        DayInput {
            place: Place::Norway,
            lodging,
            meals: Meals::default(),
        }
    }

    #[test]
    fn a_remainder_of_six_hours_is_a_dogn_and_under_six_is_not() {
        // Two full døgn and 6 h.
        assert_eq!(day_count(at(2, 8, 0), at(4, 14, 0), true), Ok(3));
        // Two full døgn and 5 h 59 min.
        assert_eq!(day_count(at(2, 8, 0), at(4, 13, 59), true), Ok(2));
        // Over the evening and home before lunch: one døgn.
        assert_eq!(day_count(at(2, 18, 0), at(3, 10, 0), true), Ok(1));
        // Out at 19, home at 7 after a hotel night: still a døgn with a
        // night (satsforskriften § 2 sets no minimum).
        assert_eq!(day_count(at(2, 19, 0), at(3, 7, 0), true), Ok(1));
        assert_eq!(
            day_count(at(2, 22, 0), at(3, 3, 0), true),
            Err(TravelError::UnderSixHours)
        );
        assert_eq!(
            day_count(at(2, 8, 0), at(2, 13, 59), false),
            Err(TravelError::UnderSixHours)
        );
        assert_eq!(
            day_count(at(2, 8, 0), at(3, 9, 0), false),
            Err(TravelError::DayTripOverADay)
        );
        assert_eq!(
            day_count(at(2, 8, 0), at(1, 9, 0), false),
            Err(TravelError::ReturnBeforeDeparture)
        );
        assert_eq!(
            day_count(at(1, 8, 0), at(30, 9, 0), true),
            Err(TravelError::OverTwentyEightDays)
        );
    }

    #[test]
    fn two_hotel_nights_paid_at_the_trekkfri_rates_are_all_trekkfri() {
        // Hotel with breakfast in the price both nights: 693 − 139 (20 %
        // of 693 = 138,60, rounded) = 554 kr, satsforskriften's own figure.
        // The part døgn follows the last night's rate, no meals.
        let breakfast = Meals {
            breakfast: true,
            ..Meals::default()
        };
        let days = [
            DayInput {
                meals: breakfast,
                ..norway(Some(Lodging::Hotel))
            },
            DayInput {
                meals: breakfast,
                ..norway(Some(Lodging::Hotel))
            },
            norway(None),
        ];
        let a = allowance(
            at(2, 8, 0),
            at(4, 15, 0),
            true,
            &days,
            Scheme::TaxFree,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(a.lines[0].kost_paid_ore, 55_400);
        assert_eq!(a.lines[2].kind, DayKind::Remainder);
        assert_eq!(a.lines[2].lodging, Some(Lodging::Hotel));
        assert_eq!(a.lines[2].kost_paid_ore, 69_300);
        assert_eq!(a.amount_ore, 55_400 * 2 + 69_300);
        assert_eq!(a.taxable_ore, 0);
    }

    #[test]
    fn the_states_rates_leave_a_trekkpliktig_excess() {
        // One hotel night, home the next afternoon after 7 hours more:
        // the state pays 1 012 for the døgn and 397 for the part døgn;
        // trekkfri is 693 for both.
        let days = [norway(Some(Lodging::Hotel)), norway(None)];
        let a = allowance(
            at(2, 8, 0),
            at(3, 15, 0),
            true,
            &days,
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(a.lines[0].kost_paid_ore, 101_200);
        assert_eq!(a.lines[1].kost_paid_ore, 39_700);
        assert_eq!(a.amount_ore, 140_900);
        // The part døgn is paid 397 but 693 would be trekkfri: never more
        // trekkfri than paid.
        assert_eq!(a.tax_free_ore, 69_300 + 39_700);
        assert_eq!(a.taxable_ore, 101_200 - 69_300);
    }

    #[test]
    fn a_private_night_gets_the_nattillegg_and_keeps_the_breakfast() {
        // State, private overnight in Norway, breakfast served by the
        // host: the nattillegg is paid and breakfast is NOT deducted from
        // the state's kost (innenlands § 9 nr. 4) — but it is from the
        // trekkfri rate, which knows no such exception.
        let day = DayInput {
            meals: Meals {
                breakfast: true,
                ..Meals::default()
            },
            ..norway(Some(Lodging::Private))
        };
        let a = allowance(
            at(2, 8, 0),
            at(3, 8, 0),
            true,
            &[day],
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        let l = a.lines[0];
        assert_eq!(l.night_paid_ore, 45_200);
        assert_eq!(l.night_tax_free_ore, 45_200);
        assert_eq!(l.kost_paid_ore, 101_200);
        // 107 − 21 (20 % of 107 = 21,40) = 86.
        assert_eq!(l.kost_tax_free_ore, 8_600);
        assert_eq!(a.taxable_ore, 101_200 - 8_600);
        // Paying the trekkfri rates, there is no nattillegg at all.
        let t = allowance(
            at(2, 8, 0),
            at(3, 8, 0),
            true,
            &[day],
            Scheme::TaxFree,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(t.lines[0].night_paid_ore, 0);
        assert_eq!(t.amount_ore, 8_600);
    }

    #[test]
    fn a_nattillegg_above_the_trekkfri_amount_is_trekkpliktig_whole() {
        let high = StateRates {
            night: 50_000,
            ..STATE
        };
        let a = allowance(
            at(2, 8, 0),
            at(3, 8, 0),
            true,
            &[norway(Some(Lodging::Private))],
            Scheme::State,
            &TAX_FREE,
            &high,
        )
        .unwrap();
        assert_eq!(a.lines[0].night_tax_free_ore, 0);
        let report = report_lines(&a.lines);
        assert!(report.contains(&(Report::OtherTaxable, 50_000, 0)));
        assert!(!report.iter().any(|(r, ..)| *r == Report::Night));
    }

    #[test]
    fn day_trips_follow_the_hour_bands() {
        let one = |from, to, scheme| {
            allowance(
                at(2, from, 0),
                at(2, to, 0),
                false,
                &[norway(None)],
                scheme,
                &TAX_FREE,
                &STATE,
            )
            .unwrap()
        };
        // Exactly 12 hours is still the 6-12 band, in both sets.
        assert_eq!(one(7, 19, Scheme::TaxFree).amount_ore, 20_000);
        assert_eq!(one(7, 19, Scheme::State).amount_ore, 39_700);
        let long = one(7, 20, Scheme::State);
        assert_eq!(long.amount_ore, 73_600);
        assert_eq!(long.tax_free_ore, 40_000);
        assert_eq!(
            report_lines(&long.lines),
            vec![
                (Report::KostDayTrip, 40_000, 1),
                (Report::KostTaxable, 33_600, 0)
            ]
        );
    }

    #[test]
    fn abroad_the_state_pays_the_country_rate_and_trekkfri_stays_flat() {
        // Berlin, 87 EUR at 11,75 = 1 022,25 kr per døgn. Two nights at a
        // hotel, then 8 hours: half the rate. Dinner at the conference on
        // the first day.
        let berlin = Place::Abroad {
            state_rate_ore: 102_225,
        };
        let days = [
            DayInput {
                place: berlin,
                lodging: Some(Lodging::Hotel),
                meals: Meals {
                    dinner: true,
                    ..Meals::default()
                },
            },
            DayInput {
                place: berlin,
                lodging: Some(Lodging::Hotel),
                meals: Meals::default(),
            },
            DayInput {
                place: berlin,
                lodging: None,
                meals: Meals::default(),
            },
        ];
        let a = allowance(
            at(2, 6, 0),
            at(4, 14, 0),
            true,
            &days,
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        // 50 % of 1 022,25 = 511,125 → 511 kr reduction.
        assert_eq!(a.lines[0].kost_paid_ore, 102_225 - 51_100);
        // 693 − 347 (346,50 rounded half up).
        assert_eq!(a.lines[0].kost_tax_free_ore, 34_600);
        assert_eq!(a.lines[1].kost_paid_ore, 102_225);
        assert_eq!(a.lines[2].kost_paid_ore, 51_113);
        // No nattillegg abroad, whatever the lodging.
        assert!(a.lines.iter().all(|l| l.night_paid_ore == 0));
        assert_eq!(a.tax_free_ore, 34_600 + 69_300 + 51_113);
    }

    #[test]
    fn a_night_in_the_part_dogn_counts_as_a_night() {
        // Out at 18, home at 10 two days later: 40 hours, two private
        // nights. The part døgn holds the second night, so the state pays
        // the overnight rate and the nattillegg for it too.
        let days = [
            norway(Some(Lodging::Private)),
            norway(Some(Lodging::Private)),
        ];
        let a = allowance(
            at(2, 18, 0),
            at(4, 10, 0),
            true,
            &days,
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(a.lines[1].kind, DayKind::Overnight);
        assert_eq!(a.lines[1].kost_paid_ore, 101_200);
        assert_eq!(a.lines[1].night_paid_ore, 45_200);
        // Without saying so it stays a part døgn at the hour band.
        let b = allowance(
            at(2, 18, 0),
            at(4, 10, 0),
            true,
            &[norway(Some(Lodging::Private)), norway(None)],
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(b.lines[1].kind, DayKind::Remainder);
        assert_eq!(b.lines[1].kost_paid_ore, 73_600);
    }

    #[test]
    fn a_short_overnight_trip_falls_in_the_states_lower_band() {
        let a = allowance(
            at(2, 19, 0),
            at(3, 7, 0),
            true,
            &[norway(Some(Lodging::Hotel))],
            Scheme::State,
            &TAX_FREE,
            &STATE,
        )
        .unwrap();
        assert_eq!(a.lines[0].kost_paid_ore, 39_700);
        assert_eq!(
            a.lines[0].kost_tax_free_ore, 39_700,
            "693 trekkfri, capped at paid"
        );
        let err = allowance(
            at(2, 8, 0),
            at(2, 20, 0),
            false,
            &[norway(Some(Lodging::Hotel))],
            Scheme::State,
            &TAX_FREE,
            &STATE,
        );
        assert_eq!(err.unwrap_err(), TravelError::NoNightOnDayTrip);
    }

    #[test]
    fn all_three_meals_leave_nothing() {
        let all = Meals {
            breakfast: true,
            lunch: true,
            dinner: true,
        };
        assert_eq!(after_meals(69_300, all, true), 0);
        assert_eq!(after_meals(10_700, all, true), 0);
    }

    #[test]
    fn the_days_must_match_the_trip() {
        let err = allowance(
            at(2, 8, 0),
            at(4, 15, 0),
            true,
            &[norway(Some(Lodging::Hotel))],
            Scheme::TaxFree,
            &TAX_FREE,
            &STATE,
        );
        assert_eq!(
            err.unwrap_err(),
            TravelError::DayCount {
                expected: 3,
                got: 1
            }
        );
        let err = allowance(
            at(2, 8, 0),
            at(4, 15, 0),
            true,
            &[norway(Some(Lodging::Hotel)), norway(None), norway(None)],
            Scheme::TaxFree,
            &TAX_FREE,
            &STATE,
        );
        assert_eq!(err.unwrap_err(), TravelError::NoLodging { day: 2 });
    }
}
