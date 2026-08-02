//! Rettigheter, roles and the one access guard (#56, #59 —
//! docs/auth.md).
//!
//! Before #56 every module had its own `require_access` — 22 copies in
//! three different shapes. They were merged into one guard with three
//! levels. #59 replaced the levels with a **vocabulary of rettigheter**,
//! because a ladder of ranks cannot express "someone who only invoices"
//! or "a controller who sees everything except lønn".
//!
//! The model:
//!
//! - An endpoint states which **rettighet** the action requires. That
//!   belongs to the action, not to the person, and does not change when
//!   we add a role.
//! - A **role is a SET of rettigheter**, not a step. Today the sets are
//!   fixed bundles in this file; #60 moves them to the database so a
//!   company can compose its own.
//! - The vocabulary is an **enum in the code**, not free text. An
//!   endpoint cannot require a rettighet that does not exist, and the
//!   compiler finds every site when one is renamed.
//!
//! The names are Norwegian, like the rest of the domain (CLAUDE.md):
//! `FAKTURA_LES`, not `INVOICE_READ`.
//!
//! **Rettigheter are additive, never subtractive.** There is no
//! "everything except X" — that rule is impossible to reason about when
//! roles are composed.
//!
//! The authorization lookup still lives in regnmed-db (`company_access`)
//! — the token proves identity, the database decides access.

use uuid::Uuid;

use crate::AppState;
use crate::auth::ApiError;

/// What an action requires of whoever performs it.
///
/// ## Scope: one's own data versus everyone's
///
/// Some rettigheter come in `_EGNE`/`_ALLE` pairs. An employee should log
/// their own hours without seeing their colleagues'; a manager should see
/// both. The convention is:
///
/// - `_ALLE` **implies** `_EGNE` (see [`Rett::medforer`]). Otherwise
///   every bundle would have to carry both, and a bundle that forgot
///   `_EGNE` would shut people out of their own data.
/// - An endpoint that already filters by the person requires `_EGNE`; one
///   that shows or changes other people's requires `_ALLE`.
///
/// This dimension is decided now rather than later precisely because it
/// is expensive to add afterwards: every "own" variant would be a new
/// name, and stored roles would have to be migrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rett {
    // Hovedbok, bilag and periods
    BilagLes,
    VedleggSkriv,
    BilagLastOpp,
    BilagBokfor,
    PeriodeLaas,

    // Rapporter
    RapportLes,
    MvaOrdningAdmin,

    // Faktura and sales
    FakturaLes,
    FakturaSkriv,
    FakturaSend,
    FakturamalLes,
    FakturamalSkriv,
    TilbudLes,
    TilbudSkriv,
    PurringLes,
    PurringSkriv,

    // Reskontro and contacts
    ReskontroLes,
    ReskontroSkriv,

    // Bank, payment and currency
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

    // Products and inventory
    ProduktLes,
    ProduktSkriv,
    LagerLes,
    LagerSkriv,

    // Fixed assets
    AnleggLes,
    AnleggSkriv,

    // Hours — scope applies here today
    TimerLesEgne,
    TimerLesAlle,
    /// Company-wide hour overviews (per prosjekt, unbilled). Kept apart
    /// from `TimerLesAlle`, which is an admin's right to see and correct
    /// other people's individual hours: a reader should see the totals
    /// without being able to correct anything.
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
    // Lønn
    LonnLes,
    LonnsslippLesEgen,
    LonnsslippLesAlle,
    LonnSkriv,
    LonnKjor,
    // Budget
    // Budsjett
    BudsjettLes,
    BudsjettSkriv,
    // Dimensions
    // Dimensjoner
    DimensjonLes,
    DimensjonSkriv,
    // Aksjeeierbok
    // Aksjeeierbok
    AksjebokLes,
    AksjebokSkriv,
    // Attestering
    // Attestering
    AttesteringLes,
    AttesteringUtfor,
    AttesteringAdmin,
    // Innboks by e-mail
    // Innboks per e-post
    EpostInnLes,
    EpostInnAdmin,
    // Anchoring
    // Forankring
    ForankringLes,
    // Company, oppdrag, integrations, migration
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
    /// The canonical name. This is the string #60 stores in
    /// `role_right`, so it cannot change without a migration.
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

    /// The whole vocabulary, in the order the enum is written.
    pub const ALLE: &'static [Rett] = &ALLE_RETTIGHETER;

    /// Does the action carry the accounts (or the data around them)
    /// forward?
    ///
    /// Used by the abonnement block (#65, docs/abonnement.md): a blocked
    /// abonnement stops everything that CHANGES — like a locked period —
    /// but never reading or export, and never the governance of the
    /// company either: access, oppdrag, integrations and company details
    /// can always be tidied, otherwise a blocked company could neither
    /// end an oppdrag nor let in whoever is meant to sort it out.
    ///
    /// The match is exhaustive on purpose: a NEW rettighet is forced to
    /// pick a side here, it cannot fall outside the block by accident.
    pub fn endrer(self) -> bool {
        use Rett::*;
        match self {
            // Reading — always open.
            BilagLes | RapportLes | FakturaLes | FakturamalLes | TilbudLes | PurringLes
            | ReskontroLes | BankLes | OcrLes | BetalingLes | ValutaLes | ProduktLes | LagerLes
            | AnleggLes | TimerLesEgne | TimerLesAlle | TimerRapportLes | UtleggLesEgne
            | UtleggLesAlle | LonnLes | LonnsslippLesEgen | LonnsslippLesAlle | BudsjettLes
            | DimensjonLes | AksjebokLes | AttesteringLes | EpostInnLes | ForankringLes
            | SelskapLes | OppdragLes | IntegrasjonLes => false,

            // Governance of the company — open even when the abonnement
            // is blocked (see above).
            SelskapAdmin | MedlemAdmin | OppdragAdmin | IntegrasjonAdmin | EpostInnAdmin => false,

            // Everything that carries the accounts forward — blocked.
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

    /// Turns a stored name back into a rettighet.
    ///
    /// Unknown names give `None` and are **ignored** where they are used
    /// (#60): the database does not know the vocabulary, so a role cannot
    /// promise a rettighet nobody enforces. If a version is rolled back
    /// that does not know a new rettighet, it disappears — it does not
    /// turn into something else.
    pub fn fra_slug(s: &str) -> Option<Rett> {
        Rett::ALLE.iter().copied().find(|r| r.slug() == s)
    }

    /// Whether the rettighet may be placed in a **custom** role (#60).
    ///
    /// No for anything that governs WHO HAS ACCESS. A role that can
    /// change access can give itself everything else, and then the rest
    /// of the boundary is decoration. Those rettigheter stay with admin,
    /// which is a role a company cannot rewrite.
    pub fn kan_delegeres(self) -> bool {
        !matches!(
            self,
            Rett::MedlemAdmin | Rett::OppdragAdmin | Rett::IntegrasjonAdmin | Rett::SelskapAdmin
        )
    }

    /// The area the rettighet belongs to — the same split as the
    /// portal's menu, so a grid of rettigheter can be read by someone who
    /// knows the product rather than the code.
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

    /// What the rettighet lets you do, in Norwegian.
    ///
    /// `TIMER_LES_ALLE` is for us; "Se alles timer" is for whoever is
    /// composing a role. The text belongs here and not in the portal:
    /// then there is only one list, and a new rettighet cannot be left
    /// standing without an explanation.
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

    /// What this rettighet also grants. `_ALLE` grants `_EGNE`: whoever
    /// sees everyone's hours obviously sees their own, and without the
    /// rule every bundle would have to remember both.
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

/// A person's role in one company, as `company_access` resolves it.
///
/// The roles are fixed bundles for now. #60 turns them into rows in the
/// database so a company can make its own — but the vocabulary and the
/// guard are the same, so that change does not touch the endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rolle {
    /// A role value this binary does not know. Grants NO rettigheter.
    ///
    /// This is not theoretical: during a rolling deploy the database can
    /// hold a role the old binary has never heard of. Falling back to
    /// `Les` would then turn an employee into someone who reads the whole
    /// hovedbok — an upgrade, not a downgrade. And since `Ansatt`
    /// arrived there is no "weakest" role to fall back to either:
    /// `Ansatt` and `Les` are not comparable.
    Ukjent,
    /// Self-service: own hours, own utlegg, own payslip. Not a step
    /// below `Les` — see [`ANSATT_BUNT`].
    Ansatt,
    Les,
    /// Reading like `Les`, plus the lønn data an audit needs. Comes only
    /// from an oppdrag of kind `revisjon` — it cannot be assigned
    /// directly.
    Revisor,
    Bokforing,
    Admin,
}

/// The employee's self-service (#54).
///
/// **This is not "reading minus something".** An employee may WRITE a few
/// things — their own hours, their own utlegg, a photo of a receipt — and
/// READ almost nothing. That is precisely the shape a ladder of ranks
/// could not express, and the reason the rettighet model (#59) had to
/// come first.
///
/// The bundle is positively bounded: it lists what an employee gets, not
/// what they are denied. Hovedbok, reports, faktura, bank, reskontro, the
/// employee list and everyone's hours, utlegg and lønn are not in it, and
/// will not get in by accident either — a new rettighet must be written
/// in here to apply.
pub const ANSATT_BUNT: &[Rett] = &[
    // Logging and seeing one's own hours. The prosjekt register must be
    // readable, or there is nothing to log the hours against.
    Rett::TimerLesEgne,
    Rett::TimerSkrivEgne,
    Rett::DimensjonLes,
    // Submitting and following one's own refusjonskrav.
    Rett::UtleggLesEgne,
    Rett::UtleggSkrivEgne,
    // Their own payslip — not their colleagues'.
    Rett::LonnsslippLesEgen,
    // Receipt photos from the phone (#48). Uploading is not posting: the
    // document lands in the innboks and waits for someone with
    // BILAG_BOKFOR.
    Rett::BilagLastOpp,
];

/// What anyone with access to the company gets. Revisor lives here:
/// reading everything, changing nothing.
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

/// What revisor adds to reading: the lønn data (#55).
///
/// Lønn is subject to audit — it is a material cost, and forskuddstrekk
/// and arbeidsgiveravgift are statutory figures a revisor must be able to
/// check. So the answer is "yes, a revisor sees lønn", but now it is an
/// **explicit yes** rather than a side effect of revisor and an internal
/// reader having been the same role.
pub const REVISOR_BUNT: &[Rett] = &[Rett::LonnLes, Rett::LonnsslippLesAlle];

/// What bokføring adds: everything that changes the hovedbok or ends there.
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

/// What admin adds: governing the company and who gets in.
const ADMIN_BUNT: &[Rett] = &[
    Rett::MvaOrdningAdmin,
    Rett::TimerLesAlle,
    Rett::TimerSkrivAlle,
    Rett::TimerLaas,
    Rett::AttesteringAdmin,
    Rett::EpostInnAdmin,
    Rett::SelskapAdmin,
    // Governing who gets in. Whoever holds this can give themselves
    // everything else, so it belongs with admin and nobody else.
    Rett::MedlemAdmin,
    Rett::OppdragAdmin,
    Rett::IntegrasjonAdmin,
    Rett::MigreringAdmin,
];

impl Rolle {
    /// Unknown values grant NO rettigheter — see [`Rolle::Ukjent`].
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

    /// What the role is FOR, in the user's language — the invitation
    /// guidance in the portal renders these (#79). One copy, kept next
    /// to the bundles that make the claims true: whoever changes a
    /// bundle below must reread this text.
    pub fn beskrivelse(self) -> &'static str {
        match self {
            Self::Ansatt => {
                "Selvbetjening for en lønnsmottaker: fører egne timer og utlegg, \
                 ser egne lønnsslipper og laster opp kvitteringer. Ser ikke hovedboken."
            }
            Self::Les => "Ser hele regnskapet, endrer ingenting. Lønn er unntatt.",
            Self::Revisor => {
                "Lesetilgang pluss lønn og kjedeverifikasjon. Tildeles ikke direkte — \
                 rollen følger av et revisjonsoppdrag."
            }
            Self::Bokforing => {
                "Den daglige økonomien: bilag, faktura og purring, bank og betaling, \
                 timer, lønn og rapporter. Ikke tilgangsstyring."
            }
            Self::Admin => {
                "Alt — også hvem som har tilgang: medlemmer og roller, oppdrag, \
                 integrasjoner og abonnement."
            }
            Self::Ukjent => "",
        }
    }

    /// The bundles the role is composed of. They are nested today —
    /// bokføring is reading plus something, admin is bokføring plus
    /// something — but that is a property of these three bundles, not of
    /// the model. A custom role (#60) need not be nested at all.
    fn bunter(self) -> &'static [&'static [Rett]] {
        match self {
            Self::Ukjent => &[],
            // Ansatt is NOT nested under the other three — it is its own
            // composition, and that is the whole point of the model.
            Self::Ansatt => &[ANSATT_BUNT],
            Self::Les => &[LES_BUNT],
            Self::Revisor => &[LES_BUNT, REVISOR_BUNT],
            Self::Bokforing => &[LES_BUNT, BOKFORING_BUNT],
            Self::Admin => &[LES_BUNT, BOKFORING_BUNT, ADMIN_BUNT],
        }
    }

    /// Whether the role has the rettighet, directly or because another
    /// rettighet implies it.
    pub fn har(self, rett: Rett) -> bool {
        self.bunter()
            .iter()
            .flat_map(|b| b.iter())
            .any(|r| *r == rett || r.medforer().contains(&rett))
    }

    /// Every rettighet the role actually grants, implications included.
    /// The portal uses this to show what a role means (#61).
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

/// Every rettighet a person has in one company, unioned across each route
/// in.
///
/// **The union is not decoration.** While the roles were a ladder it was
/// enough to pick the strongest, but `ansatt` is not a step — it can
/// write things `les` cannot. Someone who is an employee of the company
/// AND comes in through an oppdrag would lose the right to log their own
/// hours if we picked just one role. Once roles become custom (#60) the
/// union is moreover the only rule that still makes sense.
#[derive(Debug, Clone)]
pub struct Tilgang {
    roller: Vec<Rolle>,
    /// Rettigheter from the company's OWN roles (#60), already filtered
    /// against the vocabulary and against [`Rett::kan_delegeres`].
    egendefinerte: Vec<Rett>,
}

impl Tilgang {
    pub fn har(&self, rett: Rett) -> bool {
        self.roller.iter().any(|r| r.har(rett)) || self.egendefinerte.contains(&rett)
    }

    pub fn er_admin(&self) -> bool {
        self.roller.iter().any(|r| r.er_admin())
    }

    /// The role names, strongest first — for display and logging, never
    /// for deciding access.
    pub fn roller(&self) -> Vec<&'static str> {
        self.roller.iter().map(|r| r.slug()).collect()
    }
}

/// Looks up the person's access to the company and requires `rett`.
///
/// **An unknown company and missing access both give 404.** Someone
/// without access must not learn that the company exists; that is the
/// same rule as everywhere else in the API (docs/auth.md).
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

    // Names that are not built-in may be the company's own roles. Only
    // then does it cost a lookup — the vast majority of calls do not.
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
            // A name the database carries but the code does not know
            // grants nothing — and a rettighet that cannot be delegated
            // stays inert even if it had somehow found its way into the
            // table.
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

    // The abonnement block (#65, docs/abonnement.md) — AFTER the access
    // check, so an outsider still gets 404 and never learns anything
    // about the company's abonnement. Only changing rettigheter
    // costs the lookup; reading and export always go through.
    if rett.endrer() && regnmed_db::abonnement::sperret(&state.pool, company_id).await? {
        return Err(ApiError::Forbidden(
            "abonnementet er utløpt — lesing og eksport virker som før, men endringer er sperret til abonnementet er i orden (docs/abonnement.md)",
        ));
    }
    Ok(tilgang)
}

/// The error message names the missing rettighet. Whoever gets a 403
/// should be able to tell their admin what they need, without us having
/// to dig through the log.
fn manglende(rett: Rett) -> &'static str {
    // ApiError::Forbidden carries &'static str, so the slug is used directly.
    rett.slug()
}

/// Unit tests for the vocabulary.
///
/// **These cannot catch a rettighet sitting in the WRONG bundle.** They
/// derive their expectations from the bundles, so moving `PRODUKT_SKRIV`
/// from bokføring to reading still passes — tried, deliberately.
/// `tests/grupper/tilgang.rs` is the guard there: it asks a real server
/// with a real role and sees what gets through.
///
/// What these tests do catch is the rest: a rettighet in two bundles, a
/// duplicated slug, a bundle that forgot something, an implication that
/// does not work.
#[cfg(test)]
mod tests {
    use super::*;

    /// The whole vocabulary, as the expected set for the other tests.
    ///
    /// The bundles PLUS what they imply: `UTLEGG_LES_EGNE` is in no
    /// bundle, it comes from `UTLEGG_LES_ALLE`. Without that expansion
    /// the completeness test would fail on rettigheter no bundle names
    /// directly.
    fn every_rettighet() -> Vec<Rett> {
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

    /// The bundles must SHARE the vocabulary, not overlap. A rettighet in
    /// two bundles is either a typo or a rettighet that does not mean
    /// what it looks like.
    #[test]
    fn the_bundles_do_not_overlap() {
        let mut sett = std::collections::BTreeSet::new();
        for bunt in [LES_BUNT, BOKFORING_BUNT, ADMIN_BUNT] {
            for r in bunt {
                assert!(sett.insert(*r), "{} er i mer enn én bunt", r.slug());
            }
        }
    }

    /// ANSATT_BUNT is deliberately NOT one of the three level bundles —
    /// it reuses rettigheter from them, and is meant to. The requirement
    /// here is different: every rettighet it names must exist in the
    /// vocabulary, so the bundle cannot promise something nobody
    /// enforces.
    #[test]
    fn the_ansatt_bundle_uses_only_known_rettigheter() {
        let kjente: std::collections::BTreeSet<_> = every_rettighet().into_iter().collect();
        for r in ANSATT_BUNT {
            assert!(
                kjente.contains(r),
                "{} finnes ikke i noen nivåbunt",
                r.slug()
            );
        }
    }

    /// The most important thing about the ansatt role is what it does NOT give.
    #[test]
    fn an_ansatt_cannot_reach_the_hovedbok() {
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
        // And what it SHALL have.
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

    /// The slugs are what #60 stores in the database. Two rettigheter with
    /// the same name would make a stored role ambiguous.
    #[test]
    fn the_slugs_are_unique() {
        let mut sett = std::collections::BTreeSet::new();
        for r in every_rettighet() {
            assert!(sett.insert(r.slug()), "duplisert slug {}", r.slug());
        }
    }

    /// The rank ladder from before #59 must still hold for the built-in
    /// roles: admin has everything bokføring has, which has everything
    /// reading has. This is not a requirement of the model — a custom
    /// role need not be nested — but it is a requirement of THESE three,
    /// and it is what makes #59 behavior-preserving.
    #[test]
    fn the_built_in_roles_are_nested() {
        for r in every_rettighet() {
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

    /// The translation from the old levels: everything in the reading
    /// bundle all three must have, everything in the bokføring bundle
    /// `les` must NOT have, and everything in the admin bundle only admin
    /// has. If a rettighet moves to the wrong bundle, this is where it
    /// shows.
    #[test]
    fn the_bundles_reproduce_the_old_levels_exactly() {
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

    /// `_ALLE` implies `_EGNE`. Without the rule an admin holding
    /// TIMER_SKRIV_ALLE could not log their own hours unless the bundle
    /// remembered both — and that kind of "remember both" is exactly what
    /// slips.
    #[test]
    fn alle_implies_egne() {
        assert!(Rett::TimerLesAlle.medforer().contains(&Rett::TimerLesEgne));
        assert!(
            Rett::TimerSkrivAlle
                .medforer()
                .contains(&Rett::TimerSkrivEgne)
        );
        // Admin has _ALLE and therefore gets _EGNE without it being in the bundle.
        assert!(!ADMIN_BUNT.contains(&Rett::TimerSkrivEgne));
        assert!(Rolle::Admin.har(Rett::TimerSkrivEgne));
        // And it does not hold the other way: own does not give everyone's.
        assert!(!Rolle::Bokforing.har(Rett::TimerSkrivAlle));
        assert!(Rolle::Bokforing.har(Rett::TimerSkrivEgne));
    }

    #[test]
    fn the_role_round_trips_against_the_database_values() {
        for slug in ["admin", "bokforing", "les", "ansatt", "revisor"] {
            assert_eq!(Rolle::fra_db(slug).slug(), slug);
        }
    }

    /// A role value we do not recognise grants NO rettigheter.
    ///
    /// Before the ansatt role, unknown fell back to `Les`, which was then
    /// the weakest. Now `Ansatt` and `Les` are not comparable, and during
    /// a rolling deploy an old binary can meet a role it does not know —
    /// reading "ansatt" as "les" would have been an upgrade.
    #[test]
    fn an_unknown_role_grants_no_rettigheter() {
        for ukjent in ["superbruker", "", "Ansatt", "LES"] {
            let r = Rolle::fra_db(ukjent);
            assert_eq!(r, Rolle::Ukjent, "«{ukjent}»");
            assert!(r.rettigheter().is_empty(), "«{ukjent}» ga rettigheter");
        }
    }

    /// A revisor sees lønn, an internal reader does not (#55). That is
    /// deliberate: both read everything else the same way, but only one
    /// has an audit duty that requires the lønn data.
    #[test]
    fn a_revisor_sees_lonn_but_an_internal_reader_does_not() {
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
        // A revisor is otherwise exactly a reader — and writes nothing.
        assert!(Rolle::Revisor.har(Rett::RapportLes));
        assert!(!Rolle::Revisor.har(Rett::BilagBokfor));
        assert!(!Rolle::Revisor.har(Rett::LonnKjor));
        // Their own slip belongs to every employee, independently of this.
        assert!(Rolle::Ansatt.har(Rett::LonnsslippLesEgen));
    }

    /// Every rettighet must have an explanation and a group. Without the
    /// test a new rettighet could be left standing in the grid without
    /// text — and a checkbox without an explanation is worse than no box.
    #[test]
    fn every_rettighet_has_a_description_and_a_group() {
        for r in Rett::ALLE {
            assert!(
                !r.beskrivelse().is_empty(),
                "{} mangler forklaring",
                r.slug()
            );
            assert!(!r.gruppe().is_empty(), "{} mangler gruppe", r.slug());
            // The description is for a human, not the slug again.
            assert_ne!(r.beskrivelse(), r.slug(), "{}", r.slug());
        }
    }

    /// The slug must read back — it is what #60 stores.
    #[test]
    fn the_slug_round_trips() {
        for r in Rett::ALLE {
            assert_eq!(Rett::fra_slug(r.slug()), Some(*r), "{}", r.slug());
        }
        assert_eq!(Rett::fra_slug("FAKTURA_ALT"), None);
        assert_eq!(Rett::fra_slug(""), None);
    }

    /// The rettighet list the portal shows must be complete and sorted
    /// without duplicates, including what is implied.
    #[test]
    fn the_rettighet_list_is_complete() {
        let admin = Rolle::Admin.rettigheter();
        assert_eq!(admin.len(), every_rettighet().len());
        let les = Rolle::Les.rettigheter();
        assert!(les.contains(&Rett::TimerLesEgne));
        assert!(!les.contains(&Rett::TimerLesAlle));
    }
}
