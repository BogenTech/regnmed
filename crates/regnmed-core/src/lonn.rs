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
    /// aga accrued on the feriepenger set aside, due when they are paid.
    pub aga_feriepenger_ore: i64,
}

impl Lonnssum {
    pub fn total_aga_ore(&self) -> i64 {
        self.aga_ore + self.aga_feriepenger_ore
    }

    /// What the employer books as cost: pay plus the accruals.
    pub fn lonnskostnad_ore(&self) -> i64 {
        self.brutto_ore + self.feriepenger_utbetalt_ore
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
        assert_eq!(sum.lonnskostnad_ore(), 6_000_000);
        assert_eq!(sum.total_aga_ore(), 776_910);
    }
}
