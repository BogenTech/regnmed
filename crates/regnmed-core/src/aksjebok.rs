//! The aksjeeierbok (docs/aksjonaer.md, #43): the holding as a pure
//! function over the events, never stored — the same philosophy as
//! balances in the hovedbok.
//!
//! The aksjeeierbok is a **statutory register** in its own right
//! (aksjeloven §4-5): the board shall keep it, it shall be kept in a
//! sound manner, and it may be kept electronically. It has value long
//! before anyone files an aksjonærregisteroppgave. That is why this is
//! modelled as an insert-only event trail and not as a "number of shares"
//! column somebody overwrites: an ownership share as of today is a
//! claim, a sequence of events is proof.
//!
//! The transaction types below are taken from Skatteetaten's own
//! rettledning RF-1087 (post 23 tilgang, post 24 omfordeling, post 25
//! avgang). The names are authoritative. **The codes are not** — see
//! [`Transaksjonstype::kode`].

use chrono::NaiveDate;

/// The aksjetype code the oppgave is filed per. Ordinære aksjer are
/// `01`; regnmed files one oppgave per company and does not model
/// several aksjeklasser (a tracked v1 limitation, docs/aksjonaer.md).
///
/// Verified against Skatteetatens own published RF-1086 example.
pub const AKSJETYPE_ORDINAERE: &str = "01";

/// How a shareholder's holding changed.
///
/// One enum for all three posts, because the aksjeeierbok does not care
/// which form field a movement lands in — only the oppgave does, and it
/// asks each variant where it belongs via [`Transaksjonstype::post`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transaksjonstype {
    // Post 23 — tilgang (acquisitions).
    Kjop,
    ArvUtenKontinuitet,
    GaveUtenKontinuitet,
    ArvGaveMedKontinuitet,
    AvgiftspliktigArvGaveMedKontinuitet,
    Stiftelse,
    Nyemisjon,
    NyemisjonAnsattaksjer,
    NyemisjonKonverteringFordring,
    NyemisjonKonserninternOverforing,
    KonserninternOverforing,
    FusjonSkattepliktig,
    FisjonSkattepliktig,
    SkattefriOmdanning,
    OverforingMedKontinuitet,
    BytteAksjerUtenforNorge,
    FlyttingAvSelskap,
    FordelingEktefellerSkilsmisse,
    StiftelseNyemisjonMedInntektsfradrag,
    // Post 24 — tilgang through omfordeling.
    Fondsemisjon,
    Splitt,
    SkattefriFusjon,
    SkattefriFisjon,
    SammenslaingDelingAksjeklasse,
    // Post 25 — avgang.
    Salg,
    Likvidasjon,
    PartiellLikvidasjonLikedelt,
    PartiellLikvidasjonSkjevdelt,
    InnlosningSkattefriFusjonFisjon,
    Spleis,
    SlettingEgneAksjer,
}

/// Which RF-1086 post a transaction is reported under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Post {
    /// Post 23: shares in tilgang (acquisitions).
    Tilgang,
    /// Post 24: shares in tilgang through omfordeling.
    TilgangOmfordeling,
    /// Post 25: shares in avgang.
    Avgang,
}

impl Transaksjonstype {
    /// The Norwegian name, exactly as Skatteetatens rettledning writes
    /// it. This is what the aksjeeierbok and the portal show.
    pub fn navn(self) -> &'static str {
        use Transaksjonstype::*;
        match self {
            Kjop => "kjøp",
            ArvUtenKontinuitet => "arv uten skattemessig kontinuitet",
            GaveUtenKontinuitet => "gave uten skattemessig kontinuitet",
            ArvGaveMedKontinuitet => "arv/gave med skattemessig kontinuitet",
            AvgiftspliktigArvGaveMedKontinuitet => {
                "avgiftspliktig arv/gave med skattemessig kontinuitet"
            }
            Stiftelse => "stiftelse",
            Nyemisjon => "nyemisjon",
            NyemisjonAnsattaksjer => "nyemisjon ansattaksjer",
            NyemisjonKonverteringFordring => "nyemisjon ved konvertering av fordring",
            NyemisjonKonserninternOverforing => "nyemisjon ved konsernintern overføring",
            KonserninternOverforing => "konsernintern overføring",
            FusjonSkattepliktig => "fusjon (skattepliktig)",
            FisjonSkattepliktig => "fisjon (skattepliktig)",
            SkattefriOmdanning => "skattefri omdanning til aksjeselskap",
            OverforingMedKontinuitet => "overføring med skattemessig kontinuitet",
            BytteAksjerUtenforNorge => "bytte av aksjer til/fra selskap utenfor Norge",
            FlyttingAvSelskap => "flytting av selskap til og fra Norge",
            FordelingEktefellerSkilsmisse => "fordeling mellom ektefeller ved skilsmisse",
            StiftelseNyemisjonMedInntektsfradrag => "stiftelse/nyemisjon med inntektsfradrag",
            Fondsemisjon => "fondsemisjon",
            Splitt => "splitt",
            SkattefriFusjon => "skattefri fusjon",
            SkattefriFisjon => "skattefri fisjon",
            SammenslaingDelingAksjeklasse => "sammenslåing/deling av aksjeklasse",
            Salg => "salg",
            Likvidasjon => "likvidasjon",
            PartiellLikvidasjonLikedelt => "likedelt partiell likvidasjon",
            PartiellLikvidasjonSkjevdelt => "skjevdelt partiell likvidasjon",
            InnlosningSkattefriFusjonFisjon => "innløsning ifm. skattefri fusjon/fisjon",
            Spleis => "spleis",
            SlettingEgneAksjer => "sletting av egne (selskapets) aksjer",
        }
    }

    /// Stable machine name used in the API and the database. Chosen by
    /// us, unlike [`Transaksjonstype::kode`], and therefore safe to rely
    /// on.
    pub fn slug(self) -> &'static str {
        use Transaksjonstype::*;
        match self {
            Kjop => "kjop",
            ArvUtenKontinuitet => "arv_uten_kontinuitet",
            GaveUtenKontinuitet => "gave_uten_kontinuitet",
            ArvGaveMedKontinuitet => "arv_gave_med_kontinuitet",
            AvgiftspliktigArvGaveMedKontinuitet => "avgiftspliktig_arv_gave_med_kontinuitet",
            Stiftelse => "stiftelse",
            Nyemisjon => "nyemisjon",
            NyemisjonAnsattaksjer => "nyemisjon_ansattaksjer",
            NyemisjonKonverteringFordring => "nyemisjon_konvertering_fordring",
            NyemisjonKonserninternOverforing => "nyemisjon_konsernintern_overforing",
            KonserninternOverforing => "konsernintern_overforing",
            FusjonSkattepliktig => "fusjon_skattepliktig",
            FisjonSkattepliktig => "fisjon_skattepliktig",
            SkattefriOmdanning => "skattefri_omdanning",
            OverforingMedKontinuitet => "overforing_med_kontinuitet",
            BytteAksjerUtenforNorge => "bytte_aksjer_utenfor_norge",
            FlyttingAvSelskap => "flytting_av_selskap",
            FordelingEktefellerSkilsmisse => "fordeling_ektefeller_skilsmisse",
            StiftelseNyemisjonMedInntektsfradrag => "stiftelse_nyemisjon_med_inntektsfradrag",
            Fondsemisjon => "fondsemisjon",
            Splitt => "splitt",
            SkattefriFusjon => "skattefri_fusjon",
            SkattefriFisjon => "skattefri_fisjon",
            SammenslaingDelingAksjeklasse => "sammenslaing_deling_aksjeklasse",
            Salg => "salg",
            Likvidasjon => "likvidasjon",
            PartiellLikvidasjonLikedelt => "partiell_likvidasjon_likedelt",
            PartiellLikvidasjonSkjevdelt => "partiell_likvidasjon_skjevdelt",
            InnlosningSkattefriFusjonFisjon => "innlosning_skattefri_fusjon_fisjon",
            Spleis => "spleis",
            SlettingEgneAksjer => "sletting_egne_aksjer",
        }
    }

    pub fn fra_slug(slug: &str) -> Option<Self> {
        ALLE.iter().copied().find(|t| t.slug() == slug)
    }

    pub fn post(self) -> Post {
        use Transaksjonstype::*;
        match self {
            Fondsemisjon
            | Splitt
            | SkattefriFusjon
            | SkattefriFisjon
            | SammenslaingDelingAksjeklasse => Post::TilgangOmfordeling,
            Salg
            | Likvidasjon
            | PartiellLikvidasjonLikedelt
            | PartiellLikvidasjonSkjevdelt
            | InnlosningSkattefriFusjonFisjon
            | Spleis
            | SlettingEgneAksjer => Post::Avgang,
            _ => Post::Tilgang,
        }
    }

    /// Whether the movement adds shares to the shareholder.
    pub fn er_tilgang(self) -> bool {
        self.post() != Post::Avgang
    }

    /// The code RF-1086 expects in the `Transaksjonstype` field — or
    /// `None` when we have not verified it.
    ///
    /// **This is the honest gap in #43.** Skatteetaten does not publish
    /// this code list: it is not enumerated in either XSD (both fields
    /// are unconstrained `Tekst35`), and the rettledning names the
    /// transaction types without giving their codes. The list is
    /// distributed to sluttbrukersystemer through the SBS channel —
    /// Skatteetatens own release notes speak of "kodelister" for
    /// RF-1086 as a separate artifact.
    ///
    /// So we return `Some` only for codes taken from an official
    /// Skatteetaten artifact, and the renderer refuses to file anything
    /// else rather than guess. A wrong transaksjonstype is not a
    /// cosmetic error: it flows into the shareholder's own aksjeoppgave
    /// (RF-1088) and changes inngangsverdi and skjermingsgrunnlag. A
    /// loud refusal is recoverable; a silently mis-filed transaction is
    /// not.
    pub fn kode(self) -> Option<&'static str> {
        use Transaksjonstype::*;
        match self {
            // Verified: Skatteetatens published RF-1086 example files a
            // stiftelse/nyemisjon as "N", in both the hovedskjema's
            // AksjerNyutstedteStiftelseMvType and the underskjema's
            // AksjeErvervType.
            Stiftelse | Nyemisjon => Some("N"),
            _ => None,
        }
    }
}

pub const ALLE: [Transaksjonstype; 31] = {
    use Transaksjonstype::*;
    [
        Kjop,
        ArvUtenKontinuitet,
        GaveUtenKontinuitet,
        ArvGaveMedKontinuitet,
        AvgiftspliktigArvGaveMedKontinuitet,
        Stiftelse,
        Nyemisjon,
        NyemisjonAnsattaksjer,
        NyemisjonKonverteringFordring,
        NyemisjonKonserninternOverforing,
        KonserninternOverforing,
        FusjonSkattepliktig,
        FisjonSkattepliktig,
        SkattefriOmdanning,
        OverforingMedKontinuitet,
        BytteAksjerUtenforNorge,
        FlyttingAvSelskap,
        FordelingEktefellerSkilsmisse,
        StiftelseNyemisjonMedInntektsfradrag,
        Fondsemisjon,
        Splitt,
        SkattefriFusjon,
        SkattefriFisjon,
        SammenslaingDelingAksjeklasse,
        Salg,
        Likvidasjon,
        PartiellLikvidasjonLikedelt,
        PartiellLikvidasjonSkjevdelt,
        InnlosningSkattefriFusjonFisjon,
        Spleis,
        SlettingEgneAksjer,
    ]
};

/// One movement on one shareholder's holding.
///
/// `antall` is always POSITIVE — the direction lives in the type, so a
/// stored row can never contradict itself by claiming a negative
/// purchase. [`Hendelse::delta`] resolves the sign.
#[derive(Debug, Clone)]
pub struct Hendelse {
    pub dato: NaiveDate,
    pub type_: Transaksjonstype,
    pub antall: i64,
    /// What the shareholder paid, when the company knows it. Only
    /// meaningful on tilgang; the oppgave calls it anskaffelsesverdi.
    pub vederlag_ore: Option<i64>,
}

impl Hendelse {
    /// Signed change to the holding.
    pub fn delta(&self) -> i64 {
        if self.type_.er_tilgang() {
            self.antall
        } else {
            -self.antall
        }
    }
}

/// A shareholder's movement through one year — exactly the four numbers
/// RF-1086 post 20 and posts 23-25 are built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aarsbevegelse {
    /// Holding at 31.12 the year before (post 20's "fjoråret").
    pub inngaende: i64,
    pub tilgang: i64,
    pub avgang: i64,
    /// Holding at 31.12 of the year itself.
    pub utgaende: i64,
}

/// Holding as of `dato`, inclusive — the aksjeeierbok on a given day.
///
/// Order of the input does not matter: a sum is a sum. That is the
/// point of computing rather than storing.
pub fn beholdning<'a>(hendelser: impl IntoIterator<Item = &'a Hendelse>, dato: NaiveDate) -> i64 {
    hendelser
        .into_iter()
        .filter(|h| h.dato <= dato)
        .map(Hendelse::delta)
        .sum()
}

/// Folds one shareholder's events into the year's movement.
pub fn aarsbevegelse<'a>(
    hendelser: impl IntoIterator<Item = &'a Hendelse> + Clone,
    ar: i32,
) -> Aarsbevegelse {
    let mut b = Aarsbevegelse::default();
    for h in hendelser {
        let hendelse_ar = crate::regnskapsar::regnskapsar(h.dato);
        if hendelse_ar < ar {
            b.inngaende += h.delta();
        } else if hendelse_ar == ar {
            if h.type_.er_tilgang() {
                b.tilgang += h.antall;
            } else {
                b.avgang += h.antall;
            }
        }
    }
    b.utgaende = b.inngaende + b.tilgang - b.avgang;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn h(dato: NaiveDate, type_: Transaksjonstype, antall: i64) -> Hendelse {
        Hendelse {
            dato,
            type_,
            antall,
            vederlag_ore: None,
        }
    }

    #[test]
    fn the_holding_is_the_sum_of_the_events_up_to_the_date() {
        let hendelser = vec![
            h(d(2025, 1, 1), Transaksjonstype::Stiftelse, 100),
            h(d(2026, 3, 1), Transaksjonstype::Kjop, 50),
            h(d(2026, 9, 1), Transaksjonstype::Salg, 30),
        ];
        assert_eq!(beholdning(&hendelser, d(2024, 12, 31)), 0);
        assert_eq!(beholdning(&hendelser, d(2025, 1, 1)), 100);
        assert_eq!(beholdning(&hendelser, d(2026, 3, 1)), 150);
        assert_eq!(beholdning(&hendelser, d(2026, 12, 31)), 120);
    }

    #[test]
    fn the_year_movement_separates_opening_from_the_years_movement() {
        let hendelser = vec![
            h(d(2025, 1, 1), Transaksjonstype::Stiftelse, 100),
            h(d(2026, 3, 1), Transaksjonstype::Kjop, 50),
            h(d(2026, 9, 1), Transaksjonstype::Salg, 30),
        ];
        let b = aarsbevegelse(&hendelser, 2026);
        assert_eq!(b.inngaende, 100);
        assert_eq!(b.tilgang, 50);
        assert_eq!(b.avgang, 30);
        assert_eq!(b.utgaende, 120);
        // Last year's closing is this year's opening — otherwise post 20 does not add up.
        assert_eq!(aarsbevegelse(&hendelser, 2025).utgaende, b.inngaende);
    }

    #[test]
    fn the_direction_lives_in_the_type_not_in_the_sign() {
        assert_eq!(h(d(2026, 1, 1), Transaksjonstype::Kjop, 10).delta(), 10);
        assert_eq!(h(d(2026, 1, 1), Transaksjonstype::Salg, 10).delta(), -10);
        assert_eq!(h(d(2026, 1, 1), Transaksjonstype::Splitt, 10).delta(), 10);
        assert_eq!(h(d(2026, 1, 1), Transaksjonstype::Spleis, 10).delta(), -10);
    }

    #[test]
    fn slugs_round_trip_and_are_unique() {
        let mut sett = std::collections::HashSet::new();
        for t in ALLE {
            assert!(sett.insert(t.slug()), "duplikat slug: {}", t.slug());
            assert_eq!(Transaksjonstype::fra_slug(t.slug()), Some(t));
        }
        assert_eq!(Transaksjonstype::fra_slug("finnes-ikke"), None);
    }

    /// Pins the honest limitation: we have verified the code only for
    /// stiftelse/nyemisjon, from Skatteetaten's own example. Should
    /// anyone add further codes, it must be because they were verified
    /// against an official source — not because they looked plausible.
    #[test]
    fn only_verified_codes_exist() {
        let med_kode: Vec<_> = ALLE.iter().filter(|t| t.kode().is_some()).collect();
        assert_eq!(med_kode.len(), 2, "{med_kode:?}");
        assert_eq!(Transaksjonstype::Stiftelse.kode(), Some("N"));
        assert_eq!(Transaksjonstype::Nyemisjon.kode(), Some("N"));
        assert_eq!(Transaksjonstype::Kjop.kode(), None);
    }
}
