//! Rettigheter, roller og den ene tilgangsvakten (#56, #59 —
//! docs/auth.md).
//!
//! Før #56 hadde hver modul sin egen `require_access` — 22 kopier i tre
//! ulike former. Den ble slått sammen til én vakt med tre nivåer. #59
//! byttet nivåene ut med et **vokabular av rettigheter**, fordi en
//! rangstige ikke kan uttrykke «en som bare fakturerer» eller «en
//! controller som ser alt bortsett fra lønn».
//!
//! Modellen:
//!
//! - Et endepunkt sier hvilken **rettighet** handlingen krever. Det
//!   hører til handlingen, ikke til personen, og endrer seg ikke når vi
//!   legger til en rolle.
//! - En **rolle er et sett rettigheter**, ikke et trinn. I dag er
//!   settene faste bunter i denne filen; #60 flytter dem til databasen
//!   så et selskap kan sette sammen sine egne.
//! - Vokabularet er en **enum i koden**, ikke fritekst. Et endepunkt
//!   kan ikke kreve en rettighet som ikke finnes, og kompilatoren
//!   finner alle stedene når en rettighet endrer navn.
//!
//! Navnene er norske, som resten av domenet (CLAUDE.md): `FAKTURA_LES`,
//! ikke `INVOICE_READ`.
//!
//! **Rettigheter er additive, aldri subtraktive.** Det finnes ingen
//! «alt unntatt X» — den regelen er umulig å resonnere om når roller
//! settes sammen.
//!
//! Autorisasjonsoppslaget ligger fortsatt i regnmed-db
//! (`company_access`) — tokenet beviser identitet, databasen avgjør
//! tilgang.

use uuid::Uuid;

use crate::AppState;
use crate::auth::ApiError;

/// Hva en handling krever av den som utfører den.
///
/// ## Omfang: egne data mot alles
///
/// Noen rettigheter finnes i par, `_EGNE`/`_ALLE`. En ansatt skal føre
/// sine egne timer uten å se kollegenes; en leder skal se begge deler.
/// Konvensjonen er:
///
/// - `_ALLE` **medfører** `_EGNE` (se [`Rett::medforer`]). Ellers måtte
///   hver bunt ta med begge, og en bunt som glemte `_EGNE` ville stengt
///   folk ute fra deres egne data.
/// - Et endepunkt som allerede filtrerer på personen krever `_EGNE`;
///   et som viser eller endrer andres krever `_ALLE`.
///
/// Denne dimensjonen er bestemt nå, ikke senere, nettopp fordi den er
/// dyr å legge til i ettertid: hver «egen»-variant ville blitt et nytt
/// navn, og lagrede roller måtte migreres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rett {
    // Hovedbok, bilag og perioder
    BilagLes,
    VedleggSkriv,
    BilagLastOpp,
    BilagBokfor,
    PeriodeLaas,

    // Rapporter
    RapportLes,
    MvaOrdningAdmin,

    // Faktura og salg
    FakturaLes,
    FakturaSkriv,
    FakturaSend,
    FakturamalLes,
    FakturamalSkriv,
    TilbudLes,
    TilbudSkriv,
    PurringLes,
    PurringSkriv,

    // Reskontro og kontakter
    ReskontroLes,
    ReskontroSkriv,

    // Bank, betaling og valuta
    BankLes,
    BankAvstem,
    OcrLes,
    OcrImport,
    BetalingLes,
    BetalingOpprett,
    BetalingGodkjenn,
    BetalingOppgjor,
    ValutaLes,
    ValutaSkriv,

    // Produkter og lager
    ProduktLes,
    ProduktSkriv,
    LagerLes,
    LagerSkriv,

    // Anlegg
    AnleggLes,
    AnleggSkriv,

    // Timer — omfang gjelder her i dag
    TimerLesEgne,
    TimerLesAlle,
    TimerSkrivEgne,
    TimerSkrivAlle,
    TimerFakturer,
    TimerLaas,

    // Utlegg
    UtleggLes,
    UtleggSkrivEgne,
    UtleggGodkjenn,
    UtleggUtbetal,

    // Lønn
    LonnLes,
    LonnsslippLes,
    LonnSkriv,
    LonnKjor,

    // Budsjett
    BudsjettLes,
    BudsjettSkriv,

    // Dimensjoner
    DimensjonLes,
    DimensjonSkriv,

    // Aksjeeierbok
    AksjebokLes,
    AksjebokSkriv,

    // Attestering
    AttesteringLes,
    AttesteringUtfor,
    AttesteringAdmin,

    // Innboks per e-post
    EpostInnLes,
    EpostInnAdmin,

    // Forankring
    ForankringLes,

    // Selskap, oppdrag, integrasjoner, migrering
    SelskapLes,
    SelskapAdmin,
    MedlemAdmin,
    KontaktSkriv,
    OppdragLes,
    OppdragAdmin,
    IntegrasjonLes,
    IntegrasjonAdmin,
    MigreringAdmin,
}

impl Rett {
    /// Det kanoniske navnet. Dette er strengen #60 lagrer i
    /// `role_right`, så den kan ikke endres uten en migrasjon.
    pub fn slug(self) -> &'static str {
        use Rett::*;
        match self {
            BilagLes => "BILAG_LES",
            VedleggSkriv => "VEDLEGG_SKRIV",
            BilagLastOpp => "BILAG_LAST_OPP",
            BilagBokfor => "BILAG_BOKFOR",
            PeriodeLaas => "PERIODE_LAAS",
            RapportLes => "RAPPORT_LES",
            MvaOrdningAdmin => "MVA_ORDNING_ADMIN",
            FakturaLes => "FAKTURA_LES",
            FakturaSkriv => "FAKTURA_SKRIV",
            FakturaSend => "FAKTURA_SEND",
            FakturamalLes => "FAKTURAMAL_LES",
            FakturamalSkriv => "FAKTURAMAL_SKRIV",
            TilbudLes => "TILBUD_LES",
            TilbudSkriv => "TILBUD_SKRIV",
            PurringLes => "PURRING_LES",
            PurringSkriv => "PURRING_SKRIV",
            ReskontroLes => "RESKONTRO_LES",
            ReskontroSkriv => "RESKONTRO_SKRIV",
            BankLes => "BANK_LES",
            BankAvstem => "BANK_AVSTEM",
            OcrLes => "OCR_LES",
            OcrImport => "OCR_IMPORT",
            BetalingLes => "BETALING_LES",
            BetalingOpprett => "BETALING_OPPRETT",
            BetalingGodkjenn => "BETALING_GODKJENN",
            BetalingOppgjor => "BETALING_OPPGJOR",
            ValutaLes => "VALUTA_LES",
            ValutaSkriv => "VALUTA_SKRIV",
            ProduktLes => "PRODUKT_LES",
            ProduktSkriv => "PRODUKT_SKRIV",
            LagerLes => "LAGER_LES",
            LagerSkriv => "LAGER_SKRIV",
            AnleggLes => "ANLEGG_LES",
            AnleggSkriv => "ANLEGG_SKRIV",
            TimerLesEgne => "TIMER_LES_EGNE",
            TimerLesAlle => "TIMER_LES_ALLE",
            TimerSkrivEgne => "TIMER_SKRIV_EGNE",
            TimerSkrivAlle => "TIMER_SKRIV_ALLE",
            TimerFakturer => "TIMER_FAKTURER",
            TimerLaas => "TIMER_LAAS",
            UtleggLes => "UTLEGG_LES",
            UtleggSkrivEgne => "UTLEGG_SKRIV_EGNE",
            UtleggGodkjenn => "UTLEGG_GODKJENN",
            UtleggUtbetal => "UTLEGG_UTBETAL",
            LonnLes => "LONN_LES",
            LonnsslippLes => "LONNSSLIPP_LES",
            LonnSkriv => "LONN_SKRIV",
            LonnKjor => "LONN_KJOR",
            BudsjettLes => "BUDSJETT_LES",
            BudsjettSkriv => "BUDSJETT_SKRIV",
            DimensjonLes => "DIMENSJON_LES",
            DimensjonSkriv => "DIMENSJON_SKRIV",
            AksjebokLes => "AKSJEBOK_LES",
            AksjebokSkriv => "AKSJEBOK_SKRIV",
            AttesteringLes => "ATTESTERING_LES",
            AttesteringUtfor => "ATTESTERING_UTFOR",
            AttesteringAdmin => "ATTESTERING_ADMIN",
            EpostInnLes => "EPOST_INN_LES",
            EpostInnAdmin => "EPOST_INN_ADMIN",
            ForankringLes => "FORANKRING_LES",
            SelskapLes => "SELSKAP_LES",
            SelskapAdmin => "SELSKAP_ADMIN",
            MedlemAdmin => "MEDLEM_ADMIN",
            KontaktSkriv => "KONTAKT_SKRIV",
            OppdragLes => "OPPDRAG_LES",
            OppdragAdmin => "OPPDRAG_ADMIN",
            IntegrasjonLes => "INTEGRASJON_LES",
            IntegrasjonAdmin => "INTEGRASJON_ADMIN",
            MigreringAdmin => "MIGRERING_ADMIN",
        }
    }

    /// Hva denne rettigheten også gir. `_ALLE` gir `_EGNE`: den som ser
    /// alles timer ser selvsagt sine egne, og uten regelen måtte hver
    /// bunt huske begge.
    pub fn medforer(self) -> &'static [Rett] {
        match self {
            Rett::TimerLesAlle => &[Rett::TimerLesEgne],
            Rett::TimerSkrivAlle => &[Rett::TimerSkrivEgne, Rett::TimerLesEgne],
            _ => &[],
        }
    }
}

/// En persons rolle i ett selskap, slik `company_access` løser den.
///
/// Rollene er foreløpig faste bunter. #60 gjør dem til rader i
/// databasen slik at et selskap kan lage sine egne — men vokabularet og
/// vakten er de samme, så den endringen rører ikke endepunktene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rolle {
    Les,
    Bokforing,
    Admin,
}

/// Det enhver med tilgang til selskapet får. Revisor lever her: lesing
/// av alt, endring av ingenting.
const LES_BUNT: &[Rett] = &[
    Rett::BilagLes,
    Rett::RapportLes,
    Rett::FakturaLes,
    Rett::FakturamalLes,
    Rett::TilbudLes,
    Rett::PurringLes,
    Rett::ReskontroLes,
    Rett::BankLes,
    Rett::OcrLes,
    Rett::BetalingLes,
    Rett::ValutaLes,
    Rett::ProduktLes,
    Rett::LagerLes,
    Rett::AnleggLes,
    Rett::TimerLesEgne,
    Rett::UtleggLes,
    Rett::LonnLes,
    // MERK: lønnsslippen er lesbar for enhver med tilgang, også i dag.
    // Det er en kjent svakhet (#55) — den står her fordi #59 ikke skal
    // endre oppførsel, ikke fordi den hører hjemme i lesebunten.
    Rett::LonnsslippLes,
    Rett::BudsjettLes,
    Rett::DimensjonLes,
    Rett::AksjebokLes,
    Rett::AttesteringLes,
    Rett::EpostInnLes,
    Rett::ForankringLes,
    Rett::SelskapLes,
    Rett::OppdragLes,
    Rett::IntegrasjonLes,
];

/// Det bokføring legger til: alt som endrer hovedboken eller ender der.
const BOKFORING_BUNT: &[Rett] = &[
    Rett::VedleggSkriv,
    Rett::BilagLastOpp,
    Rett::BilagBokfor,
    Rett::PeriodeLaas,
    Rett::FakturaSkriv,
    Rett::FakturaSend,
    Rett::FakturamalSkriv,
    Rett::TilbudSkriv,
    Rett::PurringSkriv,
    Rett::ReskontroSkriv,
    Rett::BankAvstem,
    Rett::OcrImport,
    Rett::BetalingOpprett,
    Rett::BetalingGodkjenn,
    Rett::BetalingOppgjor,
    Rett::ValutaSkriv,
    Rett::ProduktSkriv,
    Rett::LagerSkriv,
    Rett::AnleggSkriv,
    Rett::TimerSkrivEgne,
    Rett::TimerFakturer,
    Rett::UtleggSkrivEgne,
    Rett::UtleggGodkjenn,
    Rett::UtleggUtbetal,
    Rett::LonnSkriv,
    Rett::LonnKjor,
    Rett::BudsjettSkriv,
    Rett::DimensjonSkriv,
    Rett::AksjebokSkriv,
    Rett::AttesteringUtfor,
    Rett::KontaktSkriv,
];

/// Det admin legger til: å styre selskapet og hvem som slipper til.
const ADMIN_BUNT: &[Rett] = &[
    Rett::MvaOrdningAdmin,
    Rett::TimerLesAlle,
    Rett::TimerSkrivAlle,
    Rett::TimerLaas,
    Rett::AttesteringAdmin,
    Rett::EpostInnAdmin,
    Rett::SelskapAdmin,
    // Å styre hvem som slipper inn. Den som har denne kan gi seg
    // selv alt annet, så den hører hjemme hos admin og ingen andre.
    Rett::MedlemAdmin,
    Rett::OppdragAdmin,
    Rett::IntegrasjonAdmin,
    Rett::MigreringAdmin,
];

impl Rolle {
    /// Ukjente verdier blir den svakeste rollen, ikke en feil.
    ///
    /// Databasen har en check-constraint på lovlige roller, så dette
    /// skal ikke kunne skje — men skulle det skje, er «minst tilgang»
    /// det trygge svaret. Å feile åpent her ville gjort en datafeil om
    /// til en tilgangseskalering.
    pub fn fra_db(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            "bokforing" => Self::Bokforing,
            _ => Self::Les,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Bokforing => "bokforing",
            Self::Les => "les",
        }
    }

    pub fn er_admin(self) -> bool {
        self == Self::Admin
    }

    /// Buntene rollen er satt sammen av. De er nøstet i dag — bokføring
    /// er lesing pluss noe, admin er bokføring pluss noe — men det er
    /// en egenskap ved disse tre bunter, ikke ved modellen. En
    /// egendefinert rolle (#60) trenger ikke være nøstet i det hele
    /// tatt.
    fn bunter(self) -> &'static [&'static [Rett]] {
        match self {
            Self::Les => &[LES_BUNT],
            Self::Bokforing => &[LES_BUNT, BOKFORING_BUNT],
            Self::Admin => &[LES_BUNT, BOKFORING_BUNT, ADMIN_BUNT],
        }
    }

    /// Om rollen har rettigheten, direkte eller fordi en annen
    /// rettighet medfører den.
    pub fn har(self, rett: Rett) -> bool {
        self.bunter()
            .iter()
            .flat_map(|b| b.iter())
            .any(|r| *r == rett || r.medforer().contains(&rett))
    }

    /// Alle rettigheter rollen faktisk gir, medregnet det som medføres.
    /// Portalen bruker dette til å vise hva en rolle betyr (#61).
    pub fn rettigheter(self) -> Vec<Rett> {
        let mut ut: Vec<Rett> = self
            .bunter()
            .iter()
            .flat_map(|b| b.iter())
            .flat_map(|r| std::iter::once(*r).chain(r.medforer().iter().copied()))
            .collect();
        ut.sort();
        ut.dedup();
        ut
    }
}

/// Slår opp personens rolle i selskapet og krever at den har `rett`.
/// Returnerer rollen, så en handler som trenger å vite mer (typisk:
/// «er dette en admin?») slipper et nytt oppslag.
///
/// **Ukjent selskap og manglende tilgang gir begge 404.** En som ikke
/// har tilgang skal ikke få vite om selskapet finnes; det er samme
/// regel som ellers i API-et (docs/auth.md).
pub async fn krev(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    rett: Rett,
) -> Result<Rolle, ApiError> {
    let rolle = regnmed_db::company_access(&state.pool, person_id, company_id)
        .await?
        .map(|s| Rolle::fra_db(&s))
        .ok_or(ApiError::NotFound)?;
    if !rolle.har(rett) {
        return Err(ApiError::Forbidden(manglende(rett)));
    }
    Ok(rolle)
}

/// Feilmeldingen navngir rettigheten som mangler. Den som får 403 skal
/// kunne si til sin admin hva han trenger, uten at vi må lete i loggen.
fn manglende(rett: Rett) -> &'static str {
    // ApiError::Forbidden bærer &'static str, så slug-en brukes direkte.
    rett.slug()
}

/// Enhetstester for vokabularet.
///
/// **Disse kan ikke fange at en rettighet ligger i FEIL bunt.** De
/// utleder fasiten sin fra buntene, så flyttes `PRODUKT_SKRIV` fra
/// bokføring til lesing, består de fortsatt — prøvd, med vilje. Det er
/// `tests/tilgang.rs` som er sperren der: den spør en ekte server med
/// en ekte rolle og ser hva som slipper gjennom.
///
/// Det disse testene fanger er det andre: en rettighet i to bunter, en
/// duplisert slug, en bunt som glemte noe, en implikasjon som ikke
/// virker.
#[cfg(test)]
mod tests {
    use super::*;

    /// Hele vokabularet, som fasit for de andre testene. Legges en
    /// rettighet til uten å havne i en bunt, faller den ut her.
    fn alle_rettigheter() -> Vec<Rett> {
        let mut v: Vec<Rett> = LES_BUNT
            .iter()
            .chain(BOKFORING_BUNT)
            .chain(ADMIN_BUNT)
            .copied()
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Buntene skal DELE vokabularet, ikke overlappe. En rettighet i to
    /// bunter er enten en skrivefeil eller en rettighet som ikke betyr
    /// det den ser ut som.
    #[test]
    fn buntene_overlapper_ikke() {
        let mut sett = std::collections::BTreeSet::new();
        for bunt in [LES_BUNT, BOKFORING_BUNT, ADMIN_BUNT] {
            for r in bunt {
                assert!(sett.insert(*r), "{} er i mer enn én bunt", r.slug());
            }
        }
    }

    /// Slug-ene er det #60 lagrer i databasen. To rettigheter med samme
    /// navn ville gjort en lagret rolle tvetydig.
    #[test]
    fn slugene_er_unike() {
        let mut sett = std::collections::BTreeSet::new();
        for r in alle_rettigheter() {
            assert!(sett.insert(r.slug()), "duplisert slug {}", r.slug());
        }
    }

    /// Rangstigen fra før #59 skal fortsatt gjelde for de innebygde
    /// rollene: admin har alt bokføring har, som har alt lesing har.
    /// Dette er ikke et krav til modellen — en egendefinert rolle
    /// trenger ikke være nøstet — men det er et krav til DISSE tre, og
    /// det er det som gjør #59 atferdsbevarende.
    #[test]
    fn de_innebygde_rollene_er_nostet() {
        for r in alle_rettigheter() {
            if Rolle::Les.har(r) {
                assert!(Rolle::Bokforing.har(r), "bokforing mangler {}", r.slug());
            }
            if Rolle::Bokforing.har(r) {
                assert!(Rolle::Admin.har(r), "admin mangler {}", r.slug());
            }
        }
    }

    /// Oversettelsen fra de gamle nivåene: alt i lesebunten skal alle
    /// tre ha, alt i bokføringsbunten skal les IKKE ha, og alt i
    /// adminbunten skal bare admin ha. Endres en rettighet til feil
    /// bunt, er det her det merkes.
    #[test]
    fn buntene_gir_noyaktig_de_gamle_nivaene() {
        for r in LES_BUNT {
            assert!(Rolle::Les.har(*r), "les mangler {}", r.slug());
        }
        for r in BOKFORING_BUNT {
            assert!(!Rolle::Les.har(*r), "les skulle ikke hatt {}", r.slug());
            assert!(Rolle::Bokforing.har(*r), "bokforing mangler {}", r.slug());
        }
        for r in ADMIN_BUNT {
            assert!(!Rolle::Les.har(*r), "les skulle ikke hatt {}", r.slug());
            assert!(
                !Rolle::Bokforing.har(*r),
                "bokforing skulle ikke hatt {}",
                r.slug()
            );
            assert!(Rolle::Admin.har(*r), "admin mangler {}", r.slug());
        }
    }

    /// `_ALLE` medfører `_EGNE`. Uten regelen ville en admin som har
    /// TIMER_SKRIV_ALLE ikke kunne føre sine egne timer med mindre
    /// bunten husket begge — og den slags «husk begge» er nettopp det
    /// som glipper.
    #[test]
    fn alle_medforer_egne() {
        assert!(Rett::TimerLesAlle.medforer().contains(&Rett::TimerLesEgne));
        assert!(
            Rett::TimerSkrivAlle
                .medforer()
                .contains(&Rett::TimerSkrivEgne)
        );
        // Admin har _ALLE og får dermed _EGNE uten å ha den i bunten.
        assert!(!ADMIN_BUNT.contains(&Rett::TimerSkrivEgne));
        assert!(Rolle::Admin.har(Rett::TimerSkrivEgne));
        // Og motsatt vei gjelder ikke: egne gir ikke alles.
        assert!(!Rolle::Bokforing.har(Rett::TimerSkrivAlle));
        assert!(Rolle::Bokforing.har(Rett::TimerSkrivEgne));
    }

    #[test]
    fn rollen_er_rundtur_mot_databasens_verdier() {
        for slug in ["admin", "bokforing", "les"] {
            assert_eq!(Rolle::fra_db(slug).slug(), slug);
        }
    }

    /// En rolleverdi vi ikke kjenner igjen skal gi MINST tilgang.
    /// Feiler dette, blir en datafeil til en tilgangseskalering.
    #[test]
    fn ukjent_rolle_faller_til_svakeste() {
        assert_eq!(Rolle::fra_db("superbruker"), Rolle::Les);
        assert_eq!(Rolle::fra_db(""), Rolle::Les);
        assert!(!Rolle::fra_db("superbruker").har(Rett::BilagBokfor));
    }

    /// Rettighetslisten portalen viser skal være komplett og sortert
    /// uten duplikater, også når noe medføres.
    #[test]
    fn rettighetslisten_er_komplett() {
        let admin = Rolle::Admin.rettigheter();
        assert_eq!(admin.len(), alle_rettigheter().len());
        let les = Rolle::Les.rettigheter();
        assert!(les.contains(&Rett::TimerLesEgne));
        assert!(!les.contains(&Rett::TimerLesAlle));
    }
}
