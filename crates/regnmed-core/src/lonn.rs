//! Lønn: brutto → forskuddstrekk → netto, arbeidsgiveravgift og
//! feriepengeavsetning (docs/lonn.md, #46 første del).
//!
//! Alt er heltall øre. Ingen flyttall er innom, og hver avrunding skjer
//! halvt vekk fra null på ett bestemt sted, slik at en lønnskjøring gir
//! nøyaktig samme tall uansett hvor den beregnes.
//!
//! **Hva denne modulen IKKE gjør**, og hvorfor det er et valg og ikke en
//! forglemmelse:
//!
//! - **Tabelltrekk.** Trekktabellene er Skatteetatens datafiler, og uten
//!   dem finnes det ingen forsvarlig måte å regne tabelltrekk på. Vi
//!   nekter høylytt i stedet for å tilnærme — et for lavt forskuddstrekk
//!   er den ansattes restskatt.
//! - **Sone Ia.** Den reduserte satsen gjelder bare til fribeløpet er
//!   brukt opp, og fribeløpet er bagatellmessig støtte som også kan
//!   forbrukes av ting regnmed ikke ser. Å regne 10,6 % uten å kjenne
//!   hele bildet ville underrapportert avgift.
//!
//! Satsene selv er data i satsregisteret (docs/regelverk.md), ikke tall
//! i denne koden.

use chrono::NaiveDate;

/// Rounds a basis-point calculation half away from zero.
fn bp_av(belop_ore: i64, sats_bp: i64) -> i64 {
    let n = belop_ore as i128 * sats_bp as i128;
    let d = 10_000i128;
    let sign = if (n < 0) != (d < 0) { -1 } else { 1 };
    (sign * ((n.abs() + d / 2) / d)) as i64
}

/// Arbeidsgiveravgift-soner (Skattedirektoratets melding, årlig).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sone {
    I,
    Ia,
    II,
    III,
    IV,
    IVa,
    V,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LonnError {
    /// Tabelltrekk needs Skatteetatens published tables.
    TabelltrekkIkkeStottet(i32),
    /// Sone Ia's reduced rate is bounded by a yearly fribeløp.
    SoneIaKreverFribelopsberegning,
    /// The satsregister has no verified rate covering this date.
    ManglerSats(String, NaiveDate),
}

impl std::fmt::Display for LonnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TabelltrekkIkkeStottet(tabell) => write!(
                f,
                "tabelltrekk (tabell {tabell}) er ikke støttet: trekktabellene er \
                 Skatteetatens datafiler, og regnmed tilnærmer dem ikke — bruk \
                 prosenttrekk fra skattekortet inntil tabellene er vendored \
                 (se docs/lonn.md)"
            ),
            Self::SoneIaKreverFribelopsberegning => write!(
                f,
                "sone Ia er ikke støttet: den reduserte satsen gjelder bare til \
                 fribeløpet er brukt opp, og fribeløpet er bagatellmessig støtte \
                 som også forbrukes utenfor regnmed — å regne redusert sats uten \
                 hele bildet ville underrapportert avgift (se docs/lonn.md)"
            ),
            Self::ManglerSats(domene, dato) => write!(
                f,
                "satsregisteret har ingen verifisert «{domene}» som dekker {dato} \
                 — legg inn satsen med kilde før lønnskjøringen"
            ),
        }
    }
}

impl std::error::Error for LonnError {}

impl Sone {
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

    pub fn fra_slug(slug: &str) -> Option<Self> {
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
    pub fn sats_domene(self) -> String {
        format!("aga_sone_{}", self.slug().to_lowercase())
    }
}

/// How the skattekort says to withhold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trekk {
    /// Percentage in basis points (35 % = 3500).
    Prosent(i64),
    /// Table number — refused, see [`LonnError::TabelltrekkIkkeStottet`].
    Tabell(i32),
    /// Frikort or otherwise exempt.
    Ingen,
}

/// One employee's pay for one month.
#[derive(Debug, Clone)]
pub struct Lonnsgrunnlag {
    /// Ordinary pay for the month (fastlønn, timelønn, tillegg).
    pub brutto_ore: i64,
    /// Feriepenger paid out this month. Kept separate because they are
    /// **trekkfrie** — see [`beregn`].
    pub feriepenger_ore: i64,
    pub trekk: Trekk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lonnsberegning {
    pub brutto_ore: i64,
    pub feriepenger_ore: i64,
    /// Base the withholding was actually computed on.
    pub trekkgrunnlag_ore: i64,
    pub forskuddstrekk_ore: i64,
    pub netto_ore: i64,
    /// True when December's half withholding was applied.
    pub halv_trekk: bool,
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
pub fn beregn(grunnlag: &Lonnsgrunnlag, maned: u32) -> Result<Lonnsberegning, LonnError> {
    let sats_bp = match grunnlag.trekk {
        Trekk::Tabell(nr) => return Err(LonnError::TabelltrekkIkkeStottet(nr)),
        Trekk::Ingen => 0,
        Trekk::Prosent(bp) => bp,
    };

    // Feriepenger never carry trekk; only ordinary pay does.
    let trekkgrunnlag = grunnlag.brutto_ore;
    let halv_trekk = maned == 12;
    let effektiv_bp = if halv_trekk { sats_bp / 2 } else { sats_bp };
    let forskuddstrekk = bp_av(trekkgrunnlag, effektiv_bp);

    Ok(Lonnsberegning {
        brutto_ore: grunnlag.brutto_ore,
        feriepenger_ore: grunnlag.feriepenger_ore,
        trekkgrunnlag_ore: trekkgrunnlag,
        forskuddstrekk_ore: forskuddstrekk,
        netto_ore: grunnlag.brutto_ore + grunnlag.feriepenger_ore - forskuddstrekk,
        halv_trekk,
    })
}

/// Arbeidsgiveravgift on a basis, at the zone's rate for the date.
///
/// `sats_bp` comes from the satsregister — this function never knows a
/// rate itself. Sone Ia is refused; see [`LonnError`].
pub fn arbeidsgiveravgift(grunnlag_ore: i64, sone: Sone, sats_bp: i64) -> Result<i64, LonnError> {
    if sone == Sone::Ia {
        return Err(LonnError::SoneIaKreverFribelopsberegning);
    }
    Ok(bp_av(grunnlag_ore, sats_bp))
}

/// Pay for logged hours: minutes at an hourly rate, in øre.
///
/// Minutes rather than hours because that is how the timesheet stores
/// them (docs/timer.md) — converting to fractional hours first would
/// introduce a rounding step that serves no one. One division, rounded
/// half away from zero, and the result is exact for whole hours.
pub fn timelonn(minutter: i64, timesats_ore: i64) -> i64 {
    let n = minutter as i128 * timesats_ore as i128;
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
pub fn aga_avsetning_mal(skyldige_feriepenger_ore: i64, sats_bp: i64) -> i64 {
    bp_av(skyldige_feriepenger_ore.max(0), sats_bp)
}

/// Feriepengeavsetning for the year's earnings (ferieloven §10).
///
/// The rate is data: 10,2 % by law, 12,5 % from the year the employee
/// turns 60 (§10 nr. 3 adds 2,3 prosentpoeng), and tariff agreements
/// commonly raise it to 12 % / 14,3 % for five weeks' holiday. Which
/// one applies is the caller's decision, recorded per employee.
pub fn feriepengeavsetning(grunnlag_ore: i64, sats_bp: i64) -> i64 {
    bp_av(grunnlag_ore, sats_bp)
}

/// What a month's payroll costs and owes, per employee, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lonnssum {
    pub brutto_ore: i64,
    pub feriepenger_utbetalt_ore: i64,
    pub forskuddstrekk_ore: i64,
    pub netto_ore: i64,
    pub feriepengeavsetning_ore: i64,
    /// aga on this month's pay.
    pub aga_ore: i64,
    /// Change in the accrued aga on unpaid feriepenger — what this run
    /// posted to reach [`aga_avsetning_mal`]. Negative when feriepenger
    /// were paid out and the accrual is drawn back down.
    pub aga_feriepenger_ore: i64,
}

impl Lonnssum {
    pub fn total_aga_ore(&self) -> i64 {
        self.aga_ore + self.aga_feriepenger_ore
    }

    /// What the run costs the employer.
    ///
    /// **Feriepenger paid out are not part of it.** They were expensed
    /// when they were earned and now only draw down the liability;
    /// counting them here would expense them twice. What the month
    /// genuinely costs is the ordinary pay plus everything accrued on
    /// it — the new feriepenger obligation and both avgifter.
    pub fn lonnskostnad_ore(&self) -> i64 {
        self.brutto_ore + self.feriepengeavsetning_ore + self.total_aga_ore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grunnlag(brutto: i64, trekk: Trekk) -> Lonnsgrunnlag {
        Lonnsgrunnlag {
            brutto_ore: brutto,
            feriepenger_ore: 0,
            trekk,
        }
    }

    #[test]
    fn prosenttrekk_er_ren_prosent_av_brutto() {
        // 50 000 kr brutto, 35 % trekk.
        let b = beregn(&grunnlag(5_000_000, Trekk::Prosent(3500)), 3).unwrap();
        assert_eq!(b.forskuddstrekk_ore, 1_750_000);
        assert_eq!(b.netto_ore, 3_250_000);
        assert!(!b.halv_trekk);
    }

    /// Feriepenger utbetales uten trekk — men de er fortsatt med i det
    /// den ansatte får.
    #[test]
    fn feriepenger_er_trekkfrie() {
        let g = Lonnsgrunnlag {
            brutto_ore: 1_000_000,
            feriepenger_ore: 4_000_000,
            trekk: Trekk::Prosent(3500),
        };
        let b = beregn(&g, 6).unwrap();
        assert_eq!(b.trekkgrunnlag_ore, 1_000_000, "bare ordinær lønn");
        assert_eq!(b.forskuddstrekk_ore, 350_000);
        assert_eq!(b.netto_ore, 1_000_000 + 4_000_000 - 350_000);
    }

    /// Halv skatt i desember. Satsen på skattekortet er beregnet over
    /// 10,5 måneder nettopp for at dette skal gå opp.
    #[test]
    fn desember_har_halvt_trekk() {
        let b = beregn(&grunnlag(5_000_000, Trekk::Prosent(3500)), 12).unwrap();
        assert!(b.halv_trekk);
        assert_eq!(b.forskuddstrekk_ore, 875_000);
        // November er en helt vanlig måned.
        let nov = beregn(&grunnlag(5_000_000, Trekk::Prosent(3500)), 11).unwrap();
        assert_eq!(nov.forskuddstrekk_ore, 1_750_000);
    }

    #[test]
    fn frikort_gir_ingen_trekk() {
        let b = beregn(&grunnlag(1_000_000, Trekk::Ingen), 5).unwrap();
        assert_eq!(b.forskuddstrekk_ore, 0);
        assert_eq!(b.netto_ore, 1_000_000);
    }

    /// Den ærlige nektelsen: uten Skatteetatens tabeller regner vi ikke
    /// tabelltrekk, vi sier fra.
    #[test]
    fn tabelltrekk_nektes_hoylytt() {
        let feil = beregn(&grunnlag(5_000_000, Trekk::Tabell(7100)), 3).unwrap_err();
        assert_eq!(feil, LonnError::TabelltrekkIkkeStottet(7100));
        assert!(feil.to_string().contains("tilnærmer dem ikke"), "{feil}");
    }

    #[test]
    fn aga_er_sats_av_grunnlaget() {
        // Sone I: 14,1 % av 50 000 kr.
        assert_eq!(
            arbeidsgiveravgift(5_000_000, Sone::I, 1410).unwrap(),
            705_000
        );
        // Sone V er nullsats — og det er et svar, ikke en manglende sats.
        assert_eq!(arbeidsgiveravgift(5_000_000, Sone::V, 0).unwrap(), 0);
    }

    #[test]
    fn sone_ia_nektes_fordi_fribelopet_ikke_kan_ses_herfra() {
        let feil = arbeidsgiveravgift(5_000_000, Sone::Ia, 1060).unwrap_err();
        assert_eq!(feil, LonnError::SoneIaKreverFribelopsberegning);
        assert!(feil.to_string().contains("fribeløpet"), "{feil}");
    }

    #[test]
    fn feriepenger_etter_ferieloven_og_tariff() {
        // §10: 10,2 % av grunnlaget.
        assert_eq!(feriepengeavsetning(50_000_000, 1020), 5_100_000);
        // Fra året man fyller 60: +2,3 prosentpoeng.
        assert_eq!(feriepengeavsetning(50_000_000, 1250), 6_250_000);
        // Tariff, fem uker.
        assert_eq!(feriepengeavsetning(50_000_000, 1200), 6_000_000);
    }

    #[test]
    fn timelonn_regnes_fra_minutter() {
        // 160 timer à 450 kr.
        assert_eq!(timelonn(160 * 60, 45_000), 7_200_000);
        // Halvtimer er eksakte.
        assert_eq!(timelonn(30, 45_000), 22_500);
        // Et skjevt minuttall runder halvt vekk fra null, én gang.
        // 7 min à 450 kr = 52,50 kr = 5250 øre.
        assert_eq!(timelonn(7, 45_000), 5_250);
        // 1 min à 100,01 kr → 166,68333… øre → 167.
        assert_eq!(timelonn(1, 10_001), 167);
        assert_eq!(timelonn(0, 45_000), 0);
    }

    #[test]
    fn avrunding_er_halvt_vekk_fra_null_og_deterministisk() {
        // 1234,55 kr * 10,2 % = 125,9241 -> 125,92
        assert_eq!(feriepengeavsetning(123_455, 1020), 12_592);
        // Nøyaktig halvparten runder opp i absoluttverdi.
        assert_eq!(bp_av(50_000, 1), 5);
        assert_eq!(bp_av(-50_000, 1), -5);
        // Samme input gir samme svar, alltid.
        assert_eq!(bp_av(123_455, 1020), bp_av(123_455, 1020));
    }

    #[test]
    fn sone_slug_er_rundtur() {
        for sone in [
            Sone::I,
            Sone::Ia,
            Sone::II,
            Sone::III,
            Sone::IV,
            Sone::IVa,
            Sone::V,
        ] {
            assert_eq!(Sone::fra_slug(sone.slug()), Some(sone));
        }
        assert_eq!(Sone::fra_slug("VI"), None);
        assert_eq!(Sone::I.sats_domene(), "aga_sone_i");
        assert_eq!(Sone::IVa.sats_domene(), "aga_sone_iva");
    }

    #[test]
    fn lonnssum_summerer_kostnad_og_avgift() {
        let sum = Lonnssum {
            brutto_ore: 5_000_000,
            feriepenger_utbetalt_ore: 1_000_000,
            forskuddstrekk_ore: 1_750_000,
            netto_ore: 4_250_000,
            feriepengeavsetning_ore: 510_000,
            aga_ore: 705_000,
            aga_feriepenger_ore: 71_910,
        };
        assert_eq!(sum.total_aga_ore(), 776_910);
        // Kostnaden er ordinær lønn + det som påløper på den. De
        // utbetalte feriepengene er IKKE med: de ble kostnadsført i
        // opptjeningsåret, og å telle dem her ville kostnadsført dem
        // to ganger.
        assert_eq!(
            sum.lonnskostnad_ore(),
            5_000_000 + 510_000 + 705_000 + 71_910
        );
    }

    /// Avsetningen er et MÅL, ikke en strøm av tillegg — derfor kan den
    /// ikke drive fra hverandre.
    #[test]
    fn aga_avsetning_er_sats_av_skyldige_feriepenger() {
        // 51 000 kr skyldige feriepenger, sone I 14,1 %.
        assert_eq!(aga_avsetning_mal(5_100_000, 1410), 719_100);
        // Ingen gjeld, ingen avsetning.
        assert_eq!(aga_avsetning_mal(0, 1410), 0);
        // Sone V er nullsats hele veien.
        assert_eq!(aga_avsetning_mal(5_100_000, 0), 0);
    }

    /// Betales det ut mer feriepenger enn lønnshistorikken har avsatt,
    /// stammer gjelden et annet sted fra — og da avsettes ingenting.
    /// Negativ avgift finnes ikke, og å bokføre den ville gjort et hull
    /// i regnskapet om til en inntekt.
    #[test]
    fn negativ_gjeld_gir_ingen_avsetning_ikke_negativ_avgift() {
        assert_eq!(aga_avsetning_mal(-3_694_000, 1410), 0);
    }

    /// Livsløpet til én feriepengekrone: avsettes, ligger, utbetales.
    /// Differansen hver kjøring bokfører er målet minus det som alt står
    /// — og summen over livsløpet er null.
    #[test]
    fn avsetningen_bygges_opp_og_trekkes_ned_til_null() {
        let sats = 1410;
        // År 1: 51 000 kr feriepenger opptjenes, ingenting avsatt før.
        let mal1 = aga_avsetning_mal(5_100_000, sats);
        let delta1 = mal1 - 0;
        assert_eq!(delta1, 719_100);

        // År 2, halvparten utbetales: gjelden er nå 25 500 kr.
        let mal2 = aga_avsetning_mal(2_550_000, sats);
        let delta2 = mal2 - mal1;
        assert!(delta2 < 0, "utbetaling trekker avsetningen ned");

        // Resten utbetales: gjelden er null, og det samme er avsetningen.
        let mal3 = aga_avsetning_mal(0, sats);
        let delta3 = mal3 - mal2;
        assert_eq!(mal3, 0);
        assert_eq!(delta1 + delta2 + delta3, 0, "livsløpet summerer til null");
    }

    /// En satsendring mellom opptjening og utbetaling korrigerer seg
    /// selv ved neste kjøring i stedet for å bli liggende som en rest.
    #[test]
    fn satsendring_korrigeres_ved_neste_kjoring() {
        let avsatt = aga_avsetning_mal(5_100_000, 1410);
        // Satsen settes ned året etter; gjelden er den samme.
        let mal = aga_avsetning_mal(5_100_000, 1400);
        let delta = mal - avsatt;
        assert_eq!(delta, -5_100, "differansen er nøyaktig satsendringen");
        assert_eq!(avsatt + delta, mal, "og etterpå står avsetningen riktig");
    }
}
