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
    /// Selskapsvide timeoversikter (per prosjekt, ufakturert). Skilt fra
    /// `TimerLesAlle`, som er admins rett til å se og rette andres
    /// enkelttimer: en leser skal se totalene uten å kunne rette noe.
    TimerRapportLes,
    TimerSkrivEgne,
    TimerSkrivAlle,
    TimerFakturer,
    TimerLaas,

    // Utlegg
    UtleggLesEgne,
    UtleggLesAlle,
    UtleggSkrivEgne,
    UtleggGodkjenn,
    UtleggUtbetal,

    // Lønn
    LonnLes,
    LonnsslippLesEgen,
    LonnsslippLesAlle,
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

static ALLE_RETTIGHETER: [Rett; 72] = [
    Rett::BilagLes,
    Rett::VedleggSkriv,
    Rett::BilagLastOpp,
    Rett::BilagBokfor,
    Rett::PeriodeLaas,
    Rett::RapportLes,
    Rett::MvaOrdningAdmin,
    Rett::FakturaLes,
    Rett::FakturaSkriv,
    Rett::FakturaSend,
    Rett::FakturamalLes,
    Rett::FakturamalSkriv,
    Rett::TilbudLes,
    Rett::TilbudSkriv,
    Rett::PurringLes,
    Rett::PurringSkriv,
    Rett::ReskontroLes,
    Rett::ReskontroSkriv,
    Rett::BankLes,
    Rett::BankAvstem,
    Rett::OcrLes,
    Rett::OcrImport,
    Rett::BetalingLes,
    Rett::BetalingOpprett,
    Rett::BetalingGodkjenn,
    Rett::BetalingOppgjor,
    Rett::ValutaLes,
    Rett::ValutaSkriv,
    Rett::ProduktLes,
    Rett::ProduktSkriv,
    Rett::LagerLes,
    Rett::LagerSkriv,
    Rett::AnleggLes,
    Rett::AnleggSkriv,
    Rett::TimerLesEgne,
    Rett::TimerLesAlle,
    Rett::TimerRapportLes,
    Rett::TimerSkrivEgne,
    Rett::TimerSkrivAlle,
    Rett::TimerFakturer,
    Rett::TimerLaas,
    Rett::UtleggLesEgne,
    Rett::UtleggLesAlle,
    Rett::UtleggSkrivEgne,
    Rett::UtleggGodkjenn,
    Rett::UtleggUtbetal,
    Rett::LonnLes,
    Rett::LonnsslippLesEgen,
    Rett::LonnsslippLesAlle,
    Rett::LonnSkriv,
    Rett::LonnKjor,
    Rett::BudsjettLes,
    Rett::BudsjettSkriv,
    Rett::DimensjonLes,
    Rett::DimensjonSkriv,
    Rett::AksjebokLes,
    Rett::AksjebokSkriv,
    Rett::AttesteringLes,
    Rett::AttesteringUtfor,
    Rett::AttesteringAdmin,
    Rett::EpostInnLes,
    Rett::EpostInnAdmin,
    Rett::ForankringLes,
    Rett::SelskapLes,
    Rett::SelskapAdmin,
    Rett::MedlemAdmin,
    Rett::KontaktSkriv,
    Rett::OppdragLes,
    Rett::OppdragAdmin,
    Rett::IntegrasjonLes,
    Rett::IntegrasjonAdmin,
    Rett::MigreringAdmin,
];

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
            TimerRapportLes => "TIMER_RAPPORT_LES",
            TimerSkrivEgne => "TIMER_SKRIV_EGNE",
            TimerSkrivAlle => "TIMER_SKRIV_ALLE",
            TimerFakturer => "TIMER_FAKTURER",
            TimerLaas => "TIMER_LAAS",
            UtleggLesEgne => "UTLEGG_LES_EGNE",
            UtleggLesAlle => "UTLEGG_LES_ALLE",
            UtleggSkrivEgne => "UTLEGG_SKRIV_EGNE",
            UtleggGodkjenn => "UTLEGG_GODKJENN",
            UtleggUtbetal => "UTLEGG_UTBETAL",
            LonnLes => "LONN_LES",
            LonnsslippLesEgen => "LONNSSLIPP_LES_EGEN",
            LonnsslippLesAlle => "LONNSSLIPP_LES_ALLE",
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

    /// Hele vokabularet, i den rekkefølgen enumet er skrevet.
    pub const ALLE: &'static [Rett] = &ALLE_RETTIGHETER;

    /// Fører handlingen regnskapet (eller dataene rundt det) videre?
    ///
    /// Brukes av abonnementssperren (#65, docs/abonnement.md): et
    /// sperret abonnement stopper alt som ENDRER — som en låst periode —
    /// men aldri lesing eller eksport, og heller ikke styringen av
    /// selskapet: tilgang, oppdrag, integrasjoner og firmaopplysninger
    /// kan alltid ryddes i, ellers kunne et sperret selskap verken
    /// avvikle oppdrag eller slippe inn den som skal ordne opp.
    ///
    /// Matchen er uttømmende med vilje: en NY rettighet tvinges til å
    /// velge side her, den kan ikke havne utenfor sperren ved et uhell.
    pub fn endrer(self) -> bool {
        use Rett::*;
        match self {
            // Lesing — alltid åpen.
            BilagLes | RapportLes | FakturaLes | FakturamalLes | TilbudLes | PurringLes
            | ReskontroLes | BankLes | OcrLes | BetalingLes | ValutaLes | ProduktLes | LagerLes
            | AnleggLes | TimerLesEgne | TimerLesAlle | TimerRapportLes | UtleggLesEgne
            | UtleggLesAlle | LonnLes | LonnsslippLesEgen | LonnsslippLesAlle | BudsjettLes
            | DimensjonLes | AksjebokLes | AttesteringLes | EpostInnLes | ForankringLes
            | SelskapLes | OppdragLes | IntegrasjonLes => false,

            // Styring av selskapet — åpen også når abonnementet er
            // sperret (se over).
            SelskapAdmin | MedlemAdmin | OppdragAdmin | IntegrasjonAdmin | EpostInnAdmin => false,

            // Alt som fører regnskapet videre — sperres.
            VedleggSkriv | BilagLastOpp | BilagBokfor | PeriodeLaas | MvaOrdningAdmin
            | FakturaSkriv | FakturaSend | FakturamalSkriv | TilbudSkriv | PurringSkriv
            | ReskontroSkriv | KontaktSkriv | BankAvstem | OcrImport | BetalingOpprett
            | BetalingGodkjenn | BetalingOppgjor | ValutaSkriv | ProduktSkriv | LagerSkriv
            | AnleggSkriv | TimerSkrivEgne | TimerSkrivAlle | TimerFakturer | TimerLaas
            | UtleggSkrivEgne | UtleggGodkjenn | UtleggUtbetal | LonnSkriv | LonnKjor
            | BudsjettSkriv | DimensjonSkriv | AksjebokSkriv | AttesteringUtfor
            | AttesteringAdmin | MigreringAdmin => true,
        }
    }

    /// Slår et lagret navn tilbake til en rettighet.
    ///
    /// Ukjente navn gir `None` og blir **ignorert** der de brukes (#60):
    /// databasen kjenner ikke vokabularet, så en rolle kan ikke love en
    /// rettighet ingen håndhever. Rulles en versjon tilbake som ikke
    /// kjenner en ny rettighet, forsvinner den — den blir ikke til noe
    /// annet.
    pub fn fra_slug(s: &str) -> Option<Rett> {
        Rett::ALLE.iter().copied().find(|r| r.slug() == s)
    }

    /// Om rettigheten kan legges i en **egendefinert** rolle (#60).
    ///
    /// Nei for alt som styrer HVEM SOM HAR TILGANG. En rolle som kan
    /// endre tilganger kan gi seg selv alt annet, og da er resten av
    /// avgrensningen bare pynt. De rettighetene blir værende hos admin,
    /// som er en rolle et selskap ikke kan skrive om.
    pub fn kan_delegeres(self) -> bool {
        !matches!(
            self,
            Rett::MedlemAdmin | Rett::OppdragAdmin | Rett::IntegrasjonAdmin | Rett::SelskapAdmin
        )
    }

    /// Området rettigheten hører til — samme inndeling som portalens
    /// meny, så et rutenett over rettigheter kan leses av noen som
    /// kjenner produktet og ikke koden.
    pub fn gruppe(self) -> &'static str {
        use Rett::*;
        match self {
            BilagLes => "Bilag",
            VedleggSkriv => "Bilag",
            BilagLastOpp => "Bilag",
            BilagBokfor => "Bilag",
            PeriodeLaas => "Bilag",
            RapportLes => "Rapporter",
            MvaOrdningAdmin => "Rapporter",
            FakturaLes => "Faktura",
            FakturaSkriv => "Faktura",
            FakturaSend => "Faktura",
            FakturamalLes => "Faktura",
            FakturamalSkriv => "Faktura",
            TilbudLes => "Faktura",
            TilbudSkriv => "Faktura",
            PurringLes => "Faktura",
            PurringSkriv => "Faktura",
            ReskontroLes => "Reskontro",
            ReskontroSkriv => "Reskontro",
            BankLes => "Bank",
            BankAvstem => "Bank",
            OcrLes => "Bank",
            OcrImport => "Bank",
            BetalingLes => "Betaling",
            BetalingOpprett => "Betaling",
            BetalingGodkjenn => "Betaling",
            BetalingOppgjor => "Betaling",
            ValutaLes => "Bank",
            ValutaSkriv => "Bank",
            ProduktLes => "Produkter",
            ProduktSkriv => "Produkter",
            LagerLes => "Produkter",
            LagerSkriv => "Produkter",
            AnleggLes => "Anlegg",
            AnleggSkriv => "Anlegg",
            TimerLesEgne => "Timer",
            TimerLesAlle => "Timer",
            TimerRapportLes => "Timer",
            TimerSkrivEgne => "Timer",
            TimerSkrivAlle => "Timer",
            TimerFakturer => "Timer",
            TimerLaas => "Timer",
            UtleggLesEgne => "Utlegg",
            UtleggLesAlle => "Utlegg",
            UtleggSkrivEgne => "Utlegg",
            UtleggGodkjenn => "Utlegg",
            UtleggUtbetal => "Utlegg",
            LonnLes => "Lønn",
            LonnsslippLesEgen => "Lønn",
            LonnsslippLesAlle => "Lønn",
            LonnSkriv => "Lønn",
            LonnKjor => "Lønn",
            BudsjettLes => "Rapporter",
            BudsjettSkriv => "Rapporter",
            DimensjonLes => "Dimensjoner",
            DimensjonSkriv => "Dimensjoner",
            AksjebokLes => "Aksjonærer",
            AksjebokSkriv => "Aksjonærer",
            AttesteringLes => "Attestering",
            AttesteringUtfor => "Attestering",
            AttesteringAdmin => "Attestering",
            EpostInnLes => "Bilag",
            EpostInnAdmin => "Bilag",
            ForankringLes => "Rapporter",
            SelskapLes => "Selskap",
            SelskapAdmin => "Selskap",
            MedlemAdmin => "Selskap",
            KontaktSkriv => "Reskontro",
            OppdragLes => "Selskap",
            OppdragAdmin => "Selskap",
            IntegrasjonLes => "Selskap",
            IntegrasjonAdmin => "Selskap",
            MigreringAdmin => "Selskap",
        }
    }

    /// Hva rettigheten lar deg gjøre, på norsk.
    ///
    /// `TIMER_LES_ALLE` er for oss; «Se alles timer» er for den som skal
    /// sette sammen en rolle. Teksten hører hjemme her og ikke i
    /// portalen: da finnes det bare én liste, og en ny rettighet kan
    /// ikke bli stående uten forklaring.
    pub fn beskrivelse(self) -> &'static str {
        use Rett::*;
        match self {
            BilagLes => "Se bilag og vedlegg",
            VedleggSkriv => "Legge vedlegg på et bilag",
            BilagLastOpp => "Sende dokument til innboksen",
            BilagBokfor => "Bokføre fra innboksen",
            PeriodeLaas => "Låse en periode",
            RapportLes => "Se regnskapsrapportene",
            MvaOrdningAdmin => "Endre mva-terminordning",
            FakturaLes => "Se fakturaer",
            FakturaSkriv => "Utstede faktura og kreditnota",
            FakturaSend => "Sende faktura på e-post",
            FakturamalLes => "Se repeterende fakturaer",
            FakturamalSkriv => "Endre repeterende fakturaer",
            TilbudLes => "Se tilbud og ordre",
            TilbudSkriv => "Lage tilbud og ordre",
            PurringLes => "Se purringer og forfalte krav",
            PurringSkriv => "Sende purring og inkassovarsel",
            ReskontroLes => "Se kunder, leverandører og åpne poster",
            ReskontroSkriv => "Endre kontakter og matche åpne poster",
            BankLes => "Se bankavstemming",
            BankAvstem => "Importere kontoutdrag og matche",
            OcrLes => "Se OCR-innbetalinger",
            OcrImport => "Importere OCR-fil",
            BetalingLes => "Se betalingslister",
            BetalingOpprett => "Opprette betalingsliste",
            BetalingGodkjenn => "Godkjenne betalingsliste",
            BetalingOppgjor => "Registrere at betalingene er utført",
            ValutaLes => "Se valutakurser",
            ValutaSkriv => "Legge inn og hente valutakurser",
            ProduktLes => "Se produktregisteret",
            ProduktSkriv => "Endre produktregisteret",
            LagerLes => "Se lagerbeholdning",
            LagerSkriv => "Registrere lagerbevegelser og varetelling",
            AnleggLes => "Se anleggsregisteret",
            AnleggSkriv => "Registrere, avskrive og avhende anleggsmidler",
            TimerLesEgne => "Se sine egne timer",
            TimerLesAlle => "Se alles timer",
            TimerRapportLes => "Se timeoversikt per prosjekt og ufakturert",
            TimerSkrivEgne => "Føre sine egne timer",
            TimerSkrivAlle => "Rette alles timer",
            TimerFakturer => "Fakturere førte timer",
            TimerLaas => "Låse timelisten for en måned",
            UtleggLesEgne => "Se sine egne utlegg",
            UtleggLesAlle => "Se alles utlegg",
            UtleggSkrivEgne => "Sende inn eget utlegg og kjøregodtgjørelse",
            UtleggGodkjenn => "Godkjenne og avvise utlegg",
            UtleggUtbetal => "Registrere utbetaling av utlegg",
            LonnLes => "Se ansattregisteret og lønnskjøringene",
            LonnsslippLesEgen => "Se sin egen lønnsslipp",
            LonnsslippLesAlle => "Se alles lønnsslipper",
            LonnSkriv => "Registrere ansatte",
            LonnKjor => "Kjøre lønn",
            BudsjettLes => "Se budsjett og avviksrapport",
            BudsjettSkriv => "Lage og fastsette budsjett",
            DimensjonLes => "Se avdelinger og prosjekter",
            DimensjonSkriv => "Endre avdelinger og prosjekter",
            AksjebokLes => "Se aksjeeierboken",
            AksjebokSkriv => "Registrere aksjonærer, hendelser og utbytte",
            AttesteringLes => "Se attesteringssporet",
            AttesteringUtfor => "Attestere bilag",
            AttesteringAdmin => "Sette attesteringspolicyen",
            EpostInnLes => "Se e-post inn til innboksen",
            EpostInnAdmin => "Styre mottaksadresse og avsenderliste",
            ForankringLes => "Se forankringen av hovedboken",
            SelskapLes => "Se firmaopplysningene",
            SelskapAdmin => "Endre firmaopplysningene",
            MedlemAdmin => "Gi og fjerne tilgang",
            KontaktSkriv => "Endre kontaktinfo på en part",
            OppdragLes => "Se oppdrag",
            OppdragAdmin => "Inngå og avslutte oppdrag",
            IntegrasjonLes => "Se integrasjoner",
            IntegrasjonAdmin => "Slippe til en integrasjon",
            MigreringAdmin => "Importere regnskap fra et annet system",
        }
    }

    /// Hva denne rettigheten også gir. `_ALLE` gir `_EGNE`: den som ser
    /// alles timer ser selvsagt sine egne, og uten regelen måtte hver
    /// bunt huske begge.
    pub fn medforer(self) -> &'static [Rett] {
        match self {
            Rett::TimerLesAlle => &[Rett::TimerLesEgne],
            Rett::TimerSkrivAlle => &[Rett::TimerSkrivEgne, Rett::TimerLesEgne],
            Rett::UtleggLesAlle => &[Rett::UtleggLesEgne],
            Rett::LonnsslippLesAlle => &[Rett::LonnsslippLesEgen],
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
    /// En rolleverdi denne binæren ikke kjenner. Gir INGEN rettigheter.
    ///
    /// Dette er ikke teoretisk: under en rullerende utrulling kan
    /// databasen ha en rolle den gamle binæren aldri har hørt om. Å
    /// falle tilbake til `Les` ville da gjort en ansatt om til en som
    /// leser hele hovedboken — en oppgradering, ikke en degradering.
    /// Etter at `Ansatt` kom til finnes det heller ingen «svakeste»
    /// rolle å falle til: `Ansatt` og `Les` er ikke sammenlignbare.
    Ukjent,
    /// Selvbetjening: egne timer, egne utlegg, egen lønnsslipp. Ikke
    /// et trinn under `Les` — se [`ANSATT_BUNT`].
    Ansatt,
    Les,
    /// Lesing som `Les`, pluss lønnsopplysningene revisjonen trenger.
    /// Kommer bare fra et oppdrag av typen `revisjon` — den kan ikke
    /// tildeles direkte.
    Revisor,
    Bokforing,
    Admin,
}

/// Den ansattes selvbetjening (#54).
///
/// **Dette er ikke «lesing minus noe».** En ansatt får SKRIVE noen få
/// ting — sine egne timer, sitt eget utlegg, et bilde av en kvittering
/// — og LESE nesten ingenting. Det er nettopp den formen en rangstige
/// ikke kunne uttrykke, og grunnen til at rettighetsmodellen (#59) måtte
/// komme først.
///
/// Bunten er positivt avgrenset: den lister hva en ansatt får, ikke hva
/// hun er nektet. Hovedbok, rapporter, faktura, bank, reskontro,
/// ansattlisten og alles timer, utlegg og lønn er ikke med, og blir det
/// ikke ved et uhell heller — en ny rettighet må skrives inn her for å
/// gjelde.
pub const ANSATT_BUNT: &[Rett] = &[
    // Føre og se sine egne timer. Prosjektregisteret må være lesbart,
    // ellers finnes det ingenting å føre timene på.
    Rett::TimerLesEgne,
    Rett::TimerSkrivEgne,
    Rett::DimensjonLes,
    // Sende inn og følge sitt eget refusjonskrav.
    Rett::UtleggLesEgne,
    Rett::UtleggSkrivEgne,
    // Sin egen lønnsslipp — ikke kollegenes.
    Rett::LonnsslippLesEgen,
    // Kvitteringsfoto fra mobilen (#48). Å laste opp er ikke å bokføre:
    // dokumentet havner i innboksen og venter på noen med BILAG_BOKFOR.
    Rett::BilagLastOpp,
];

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
    Rett::TimerRapportLes,
    Rett::UtleggLesAlle,
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

/// Det revisor legger til lesingen: lønnsopplysningene (#55).
///
/// Lønn er revisjonspliktig — den er en vesentlig kostnad, og
/// forskuddstrekk og arbeidsgiveravgift er lovpålagte størrelser en
/// revisor må kunne kontrollere. Så svaret er «ja, revisor ser lønn»,
/// men nå er det et **uttrykkelig ja** i stedet for en bieffekt av at
/// revisor og en intern leser var samme rolle.
pub const REVISOR_BUNT: &[Rett] = &[Rett::LonnLes, Rett::LonnsslippLesAlle];

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
    Rett::LonnLes,
    Rett::LonnsslippLesAlle,
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
    /// Ukjente verdier gir INGEN rettigheter — se [`Rolle::Ukjent`].
    pub fn fra_db(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            "bokforing" => Self::Bokforing,
            "ansatt" => Self::Ansatt,
            "les" => Self::Les,
            "revisor" => Self::Revisor,
            _ => Self::Ukjent,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Bokforing => "bokforing",
            Self::Les => "les",
            Self::Ansatt => "ansatt",
            Self::Revisor => "revisor",
            Self::Ukjent => "ukjent",
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
            Self::Ukjent => &[],
            // Ansatt er IKKE nøstet under de tre andre — den er sin egen
            // sammensetning, og det er hele poenget med modellen.
            Self::Ansatt => &[ANSATT_BUNT],
            Self::Les => &[LES_BUNT],
            Self::Revisor => &[LES_BUNT, REVISOR_BUNT],
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

/// Alle rettighetene en person har i ett selskap, unionert over hver vei
/// inn.
///
/// **Unionen er ikke pynt.** Så lenge rollene var en stige holdt det å
/// velge den sterkeste, men `ansatt` er ikke et trinn — den kan skrive
/// ting `les` ikke kan. En som er ansatt i selskapet OG kommer inn via
/// et oppdrag ville mistet retten til å føre sine egne timer hvis vi
/// bare valgte én rolle. Når roller blir egendefinerte (#60) er unionen
/// dessuten den eneste regelen som fortsatt gir mening.
#[derive(Debug, Clone)]
pub struct Tilgang {
    roller: Vec<Rolle>,
    /// Rettigheter fra selskapets EGNE roller (#60), allerede filtrert
    /// mot vokabularet og mot [`Rett::kan_delegeres`].
    egendefinerte: Vec<Rett>,
}

impl Tilgang {
    pub fn har(&self, rett: Rett) -> bool {
        self.roller.iter().any(|r| r.har(rett)) || self.egendefinerte.contains(&rett)
    }

    pub fn er_admin(&self) -> bool {
        self.roller.iter().any(|r| r.er_admin())
    }

    /// Rollenavnene, sterkeste først — til visning og logging, aldri
    /// til å avgjøre tilgang.
    pub fn roller(&self) -> Vec<&'static str> {
        self.roller.iter().map(|r| r.slug()).collect()
    }
}

/// Slår opp personens tilgang til selskapet og krever `rett`.
///
/// **Ukjent selskap og manglende tilgang gir begge 404.** En som ikke
/// har tilgang skal ikke få vite om selskapet finnes; det er samme
/// regel som ellers i API-et (docs/auth.md).
pub async fn krev(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    rett: Rett,
) -> Result<Tilgang, ApiError> {
    let navn = regnmed_db::company_roles(&state.pool, person_id, company_id).await?;
    if navn.is_empty() {
        return Err(ApiError::NotFound);
    }
    let roller: Vec<Rolle> = navn.iter().map(|s| Rolle::fra_db(s)).collect();

    // Navn som ikke er innebygde kan være selskapets egne roller. Bare
    // da koster det et oppslag — de aller fleste kall gjør det ikke.
    let ukjente: Vec<String> = navn
        .iter()
        .zip(&roller)
        .filter(|(_, r)| **r == Rolle::Ukjent)
        .map(|(n, _)| n.clone())
        .collect();
    let egendefinerte = if ukjente.is_empty() {
        Vec::new()
    } else {
        regnmed_db::roller::rettigheter_for(&state.pool, company_id, &ukjente)
            .await?
            .iter()
            // Et navn databasen bærer, men koden ikke kjenner, gir
            // ingenting — og en rettighet som ikke kan delegeres blir
            // liggende uvirksom selv om den skulle ha kommet inn i
            // tabellen på et vis.
            .filter_map(|s| Rett::fra_slug(s))
            .filter(|r| r.kan_delegeres())
            .collect()
    };

    let tilgang = Tilgang {
        roller,
        egendefinerte,
    };
    if !tilgang.har(rett) {
        return Err(ApiError::Forbidden(manglende(rett)));
    }

    // Abonnementssperren (#65, docs/abonnement.md) — ETTER
    // tilgangssjekken, så en utenforstående fortsatt får 404 og aldri
    // lærer noe om selskapets abonnement. Bare endrende rettigheter
    // koster oppslaget; lesing og eksport går alltid.
    if rett.endrer() && regnmed_db::abonnement::sperret(&state.pool, company_id).await? {
        return Err(ApiError::Forbidden(
            "abonnementet er utløpt — lesing og eksport virker som før, men endringer er sperret til abonnementet er i orden (docs/abonnement.md)",
        ));
    }
    Ok(tilgang)
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

    /// Hele vokabularet, som fasit for de andre testene.
    ///
    /// Buntene PLUSS det de medfører: `UTLEGG_LES_EGNE` står ikke i noen
    /// bunt, den kommer av `UTLEGG_LES_ALLE`. Uten den utvidelsen ville
    /// «hele vokabularet» utelatt nettopp de rettighetene omfanget
    /// handler om.
    fn alle_rettigheter() -> Vec<Rett> {
        let mut v: Vec<Rett> = LES_BUNT
            .iter()
            .chain(REVISOR_BUNT)
            .chain(BOKFORING_BUNT)
            .chain(ADMIN_BUNT)
            .flat_map(|r| std::iter::once(*r).chain(r.medforer().iter().copied()))
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

    /// ANSATT_BUNT er med vilje IKKE en av de tre nivåbuntene — den
    /// gjenbruker rettigheter fra dem, og skal gjøre det. Kravet her er
    /// et annet: hver rettighet den nevner må finnes i vokabularet, så
    /// bunten ikke kan love noe ingen håndhever.
    #[test]
    fn ansattbunten_bruker_bare_kjente_rettigheter() {
        let kjente: std::collections::BTreeSet<_> = alle_rettigheter().into_iter().collect();
        for r in ANSATT_BUNT {
            assert!(
                kjente.contains(r),
                "{} finnes ikke i noen nivåbunt",
                r.slug()
            );
        }
    }

    /// Det viktigste ved ansattrollen er hva den IKKE gir.
    #[test]
    fn ansatt_kommer_ikke_til_hovedboken() {
        let a = Rolle::Ansatt;
        for nektet in [
            Rett::BilagLes,
            Rett::BilagBokfor,
            Rett::RapportLes,
            Rett::FakturaLes,
            Rett::FakturaSkriv,
            Rett::BankLes,
            Rett::ReskontroLes,
            Rett::BetalingLes,
            Rett::LonnLes,
            Rett::LonnsslippLesAlle,
            Rett::TimerLesAlle,
            Rett::TimerRapportLes,
            Rett::UtleggLesAlle,
            Rett::UtleggGodkjenn,
            Rett::SelskapLes,
            Rett::MedlemAdmin,
        ] {
            assert!(!a.har(nektet), "ansatt skulle ikke hatt {}", nektet.slug());
        }
        // Og det den SKAL ha.
        for gitt in [
            Rett::TimerLesEgne,
            Rett::TimerSkrivEgne,
            Rett::UtleggSkrivEgne,
            Rett::UtleggLesEgne,
            Rett::LonnsslippLesEgen,
            Rett::BilagLastOpp,
            Rett::DimensjonLes,
        ] {
            assert!(a.har(gitt), "ansatt mangler {}", gitt.slug());
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
                assert!(Rolle::Revisor.har(r), "revisor mangler {}", r.slug());
                assert!(Rolle::Bokforing.har(r), "bokforing mangler {}", r.slug());
            }
            if Rolle::Revisor.har(r) {
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
        for slug in ["admin", "bokforing", "les", "ansatt", "revisor"] {
            assert_eq!(Rolle::fra_db(slug).slug(), slug);
        }
    }

    /// En rolleverdi vi ikke kjenner igjen gir INGEN rettigheter.
    ///
    /// Før ansattrollen falt ukjent til `Les`, som da var svakest. Nå
    /// er `Ansatt` og `Les` ikke sammenlignbare, og under en rullerende
    /// utrulling kan en gammel binær møte en rolle den ikke kjenner —
    /// å tolke «ansatt» som «les» ville vært en oppgradering.
    #[test]
    fn ukjent_rolle_gir_ingen_rettigheter() {
        for ukjent in ["superbruker", "", "Ansatt", "LES"] {
            let r = Rolle::fra_db(ukjent);
            assert_eq!(r, Rolle::Ukjent, "«{ukjent}»");
            assert!(r.rettigheter().is_empty(), "«{ukjent}» ga rettigheter");
        }
    }

    /// Revisor ser lønn, en intern leser gjør det ikke (#55). Det er
    /// hele poenget med at de to ble skilt: begge er skrivebeskyttet,
    /// men bare den ene har en revisjonsplikt som krever
    /// lønnsopplysningene.
    #[test]
    fn revisor_ser_lonn_men_en_intern_leser_gjor_det_ikke() {
        for rett in [Rett::LonnLes, Rett::LonnsslippLesAlle] {
            assert!(Rolle::Revisor.har(rett), "revisor mangler {}", rett.slug());
            assert!(
                !Rolle::Les.har(rett),
                "les skulle ikke hatt {}",
                rett.slug()
            );
            assert!(
                !Rolle::Ansatt.har(rett),
                "ansatt skulle ikke hatt {}",
                rett.slug()
            );
            assert!(
                Rolle::Bokforing.har(rett),
                "bokforing mangler {}",
                rett.slug()
            );
        }
        // Revisor er ellers akkurat en leser — og skriver ingenting.
        assert!(Rolle::Revisor.har(Rett::RapportLes));
        assert!(!Rolle::Revisor.har(Rett::BilagBokfor));
        assert!(!Rolle::Revisor.har(Rett::LonnKjor));
        // Sin egen slipp har enhver ansatt, uavhengig av dette.
        assert!(Rolle::Ansatt.har(Rett::LonnsslippLesEgen));
    }

    /// Hver rettighet må ha en forklaring og en gruppe. Uten testen kan
    /// en ny rettighet bli stående i rutenettet uten tekst — og en
    /// avkrysningsboks uten forklaring er verre enn ingen boks.
    #[test]
    fn hver_rettighet_har_forklaring_og_gruppe() {
        for r in Rett::ALLE {
            assert!(
                !r.beskrivelse().is_empty(),
                "{} mangler forklaring",
                r.slug()
            );
            assert!(!r.gruppe().is_empty(), "{} mangler gruppe", r.slug());
            // Forklaringen skal være for et menneske, ikke slug-en igjen.
            assert_ne!(r.beskrivelse(), r.slug(), "{}", r.slug());
        }
    }

    /// Slug-en må kunne leses tilbake — den er det #60 lagrer.
    #[test]
    fn slugen_er_rundtur() {
        for r in Rett::ALLE {
            assert_eq!(Rett::fra_slug(r.slug()), Some(*r), "{}", r.slug());
        }
        assert_eq!(Rett::fra_slug("FAKTURA_ALT"), None);
        assert_eq!(Rett::fra_slug(""), None);
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
