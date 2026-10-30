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
pub const SHARE_CLASS_ORDINARY: &str = "01";

/// How a shareholder's holding changed.
///
/// One enum for all three posts, because the aksjeeierbok does not care
/// which form field a movement lands in — only the oppgave does, and it
/// asks each variant where it belongs via [`Transaksjonstype::post`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionType {
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
pub enum FormItem {
    /// Post 23: shares in tilgang (acquisitions).
    Acquisition,
    /// Post 24: shares in tilgang through omfordeling.
    AcquisitionReallocation,
    /// Post 25: shares in avgang.
    Disposal,
}

impl TransactionType {
    /// The Norwegian name, exactly as Skatteetatens rettledning writes
    /// it. This is what the aksjeeierbok and the portal show.
    pub fn name(self) -> &'static str {
        use TransactionType::*;
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
        use TransactionType::*;
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

    pub fn from_slug(slug: &str) -> Option<Self> {
        ALL.iter().copied().find(|t| t.slug() == slug)
    }

    pub fn post(self) -> FormItem {
        use TransactionType::*;
        match self {
            Fondsemisjon
            | Splitt
            | SkattefriFusjon
            | SkattefriFisjon
            | SammenslaingDelingAksjeklasse => FormItem::AcquisitionReallocation,
            Salg
            | Likvidasjon
            | PartiellLikvidasjonLikedelt
            | PartiellLikvidasjonSkjevdelt
            | InnlosningSkattefriFusjonFisjon
            | Spleis
            | SlettingEgneAksjer => FormItem::Disposal,
            _ => FormItem::Acquisition,
        }
    }

    /// Whether the movement adds shares to the shareholder.
    pub fn is_acquisition(self) -> bool {
        self.post() != FormItem::Disposal
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
    pub fn code(self) -> Option<&'static str> {
        use TransactionType::*;
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

pub const ALL: [TransactionType; 31] = {
    use TransactionType::*;
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
pub struct ShareEvent {
    pub date: NaiveDate,
    pub type_: TransactionType,
    pub quantity: i64,
    /// What the shareholder paid, when the company knows it. Only
    /// meaningful on tilgang; the oppgave calls it anskaffelsesverdi.
    pub consideration_ore: Option<i64>,
}

impl ShareEvent {
    /// Signed change to the holding.
    pub fn delta(&self) -> i64 {
        if self.type_.is_acquisition() {
            self.quantity
        } else {
            -self.quantity
        }
    }
}

/// A shareholder's movement through one year — exactly the four numbers
/// RF-1086 post 20 and posts 23-25 are built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct YearMovement {
    /// Holding at 31.12 the year before (post 20's "fjoråret").
    pub opening: i64,
    pub acquisitions: i64,
    pub disposals: i64,
    /// Holding at 31.12 of the year itself.
    pub closing: i64,
}

/// Holding as of `dato`, inclusive — the aksjeeierbok on a given day.
///
/// Order of the input does not matter: a sum is a sum. That is the
/// point of computing rather than storing.
pub fn holding<'a>(events: impl IntoIterator<Item = &'a ShareEvent>, date: NaiveDate) -> i64 {
    events
        .into_iter()
        .filter(|h| h.date <= date)
        .map(ShareEvent::delta)
        .sum()
}

/// Folds one shareholder's events into the year's movement.
pub fn year_movement<'a>(
    events: impl IntoIterator<Item = &'a ShareEvent> + Clone,
    year: i32,
) -> YearMovement {
    let mut b = YearMovement::default();
    for h in events {
        let event_year = crate::fiscal_year::of(h.date);
        if event_year < year {
            b.opening += h.delta();
        } else if event_year == year {
            if h.type_.is_acquisition() {
                b.acquisitions += h.quantity;
            } else {
                b.disposals += h.quantity;
            }
        }
    }
    b.closing = b.opening + b.acquisitions - b.disposals;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn h(date: NaiveDate, type_: TransactionType, quantity: i64) -> ShareEvent {
        ShareEvent {
            date,
            type_,
            quantity,
            consideration_ore: None,
        }
    }

    #[test]
    fn the_holding_is_the_sum_of_the_events_up_to_the_date() {
        let events = vec![
            h(d(2025, 1, 1), TransactionType::Stiftelse, 100),
            h(d(2026, 3, 1), TransactionType::Kjop, 50),
            h(d(2026, 9, 1), TransactionType::Salg, 30),
        ];
        assert_eq!(holding(&events, d(2024, 12, 31)), 0);
        assert_eq!(holding(&events, d(2025, 1, 1)), 100);
        assert_eq!(holding(&events, d(2026, 3, 1)), 150);
        assert_eq!(holding(&events, d(2026, 12, 31)), 120);
    }

    #[test]
    fn the_year_movement_separates_opening_from_the_years_movement() {
        let events = vec![
            h(d(2025, 1, 1), TransactionType::Stiftelse, 100),
            h(d(2026, 3, 1), TransactionType::Kjop, 50),
            h(d(2026, 9, 1), TransactionType::Salg, 30),
        ];
        let b = year_movement(&events, 2026);
        assert_eq!(b.opening, 100);
        assert_eq!(b.acquisitions, 50);
        assert_eq!(b.disposals, 30);
        assert_eq!(b.closing, 120);
        // Last year's closing is this year's opening — otherwise post 20 does not add up.
        assert_eq!(year_movement(&events, 2025).closing, b.opening);
    }

    #[test]
    fn the_direction_lives_in_the_type_not_in_the_sign() {
        assert_eq!(h(d(2026, 1, 1), TransactionType::Kjop, 10).delta(), 10);
        assert_eq!(h(d(2026, 1, 1), TransactionType::Salg, 10).delta(), -10);
        assert_eq!(h(d(2026, 1, 1), TransactionType::Splitt, 10).delta(), 10);
        assert_eq!(h(d(2026, 1, 1), TransactionType::Spleis, 10).delta(), -10);
    }

    #[test]
    fn slugs_round_trip_and_are_unique() {
        let mut sett = std::collections::HashSet::new();
        for t in ALL {
            assert!(sett.insert(t.slug()), "duplikat slug: {}", t.slug());
            assert_eq!(TransactionType::from_slug(t.slug()), Some(t));
        }
        assert_eq!(TransactionType::from_slug("finnes-ikke"), None);
    }

    /// Pins the honest limitation: we have verified the code only for
    /// stiftelse/nyemisjon, from Skatteetaten's own example. Should
    /// anyone add further codes, it must be because they were verified
    /// against an official source — not because they looked plausible.
    #[test]
    fn only_verified_codes_exist() {
        let med_kode: Vec<_> = ALL.iter().filter(|t| t.code().is_some()).collect();
        assert_eq!(med_kode.len(), 2, "{med_kode:?}");
        assert_eq!(TransactionType::Stiftelse.code(), Some("N"));
        assert_eq!(TransactionType::Nyemisjon.code(), Some("N"));
        assert_eq!(TransactionType::Kjop.code(), None);
    }
}
