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
pub enum Right {
    // Hovedbok, bilag and periods
    VoucherRead,
    AttachmentWrite,
    VoucherUpload,
    VoucherPost,
    PeriodLock,

    // Rapporter
    ReportRead,
    VatSchemeAdmin,

    // Faktura and sales
    InvoiceRead,
    InvoiceWrite,
    InvoiceSend,
    InvoiceTemplateRead,
    InvoiceTemplateWrite,
    QuoteRead,
    QuoteWrite,
    ReminderRead,
    ReminderWrite,

    // Reskontro and contacts
    SubledgerRead,
    SubledgerWrite,

    // Bank, payment and currency
    BankRead,
    BankReconcile,
    OcrRead,
    OcrImport,
    PaymentRead,
    PaymentCreate,
    PaymentApprove,
    PaymentSettle,
    CurrencyRead,
    CurrencyWrite,

    // Products and inventory
    ProductRead,
    ProductWrite,
    InventoryRead,
    InventoryWrite,

    // Fixed assets
    AssetRead,
    AssetWrite,

    // Hours — scope applies here today
    TimesheetReadOwn,
    TimesheetReadAll,
    /// Company-wide hour overviews (per prosjekt, unbilled). Kept apart
    /// from `TimerLesAlle`, which is an admin's right to see and correct
    /// other people's individual hours: a reader should see the totals
    /// without being able to correct anything.
    TimesheetReportRead,
    TimesheetWriteOwn,
    TimesheetWriteAll,
    TimesheetBill,
    /// Setting the dated timesatser on prosjekter (default and per
    /// person) and overriding the rate on a single entry. Without it,
    /// entries get the register's rate — never the caller's number.
    TimesheetRateWrite,
    TimesheetLock,

    // Utlegg
    ExpenseReadOwn,
    ExpenseReadAll,
    ExpenseWriteOwn,
    ExpenseApprove,
    ExpensePay,
    // Lønn
    // Lønn
    PayrollRead,
    PayslipReadOwn,
    PayslipReadAll,
    PayrollWrite,
    PayrollRun,
    // Budget
    // Budsjett
    BudgetRead,
    BudgetWrite,
    // Dimensions
    // Dimensjoner
    DimensionRead,
    DimensionWrite,
    // Aksjeeierbok
    // Aksjeeierbok
    ShareRegisterRead,
    ShareRegisterWrite,
    // Attestering
    // Attestering
    ApprovalRead,
    ApprovalPerform,
    ApprovalAdmin,
    // Innboks by e-mail
    // Innboks per e-post
    InboundMailRead,
    InboundMailAdmin,
    // Anchoring
    // Forankring
    AnchorRead,
    // Company, oppdrag, integrations, migration
    // Selskap, oppdrag, integrasjoner, migrering
    CompanyRead,
    CompanyAdmin,
    MemberAdmin,
    ContactWrite,
    EngagementRead,
    EngagementAdmin,
    IntegrationRead,
    IntegrationAdmin,
    MigrationAdmin,
}

static ALL_RIGHTS: [Right; 73] = [
    Right::VoucherRead,
    Right::AttachmentWrite,
    Right::VoucherUpload,
    Right::VoucherPost,
    Right::PeriodLock,
    Right::ReportRead,
    Right::VatSchemeAdmin,
    Right::InvoiceRead,
    Right::InvoiceWrite,
    Right::InvoiceSend,
    Right::InvoiceTemplateRead,
    Right::InvoiceTemplateWrite,
    Right::QuoteRead,
    Right::QuoteWrite,
    Right::ReminderRead,
    Right::ReminderWrite,
    Right::SubledgerRead,
    Right::SubledgerWrite,
    Right::BankRead,
    Right::BankReconcile,
    Right::OcrRead,
    Right::OcrImport,
    Right::PaymentRead,
    Right::PaymentCreate,
    Right::PaymentApprove,
    Right::PaymentSettle,
    Right::CurrencyRead,
    Right::CurrencyWrite,
    Right::ProductRead,
    Right::ProductWrite,
    Right::InventoryRead,
    Right::InventoryWrite,
    Right::AssetRead,
    Right::AssetWrite,
    Right::TimesheetReadOwn,
    Right::TimesheetReadAll,
    Right::TimesheetReportRead,
    Right::TimesheetWriteOwn,
    Right::TimesheetWriteAll,
    Right::TimesheetBill,
    Right::TimesheetRateWrite,
    Right::TimesheetLock,
    Right::ExpenseReadOwn,
    Right::ExpenseReadAll,
    Right::ExpenseWriteOwn,
    Right::ExpenseApprove,
    Right::ExpensePay,
    Right::PayrollRead,
    Right::PayslipReadOwn,
    Right::PayslipReadAll,
    Right::PayrollWrite,
    Right::PayrollRun,
    Right::BudgetRead,
    Right::BudgetWrite,
    Right::DimensionRead,
    Right::DimensionWrite,
    Right::ShareRegisterRead,
    Right::ShareRegisterWrite,
    Right::ApprovalRead,
    Right::ApprovalPerform,
    Right::ApprovalAdmin,
    Right::InboundMailRead,
    Right::InboundMailAdmin,
    Right::AnchorRead,
    Right::CompanyRead,
    Right::CompanyAdmin,
    Right::MemberAdmin,
    Right::ContactWrite,
    Right::EngagementRead,
    Right::EngagementAdmin,
    Right::IntegrationRead,
    Right::IntegrationAdmin,
    Right::MigrationAdmin,
];

impl Right {
    /// The canonical name. This is the string #60 stores in
    /// `role_right`, so it cannot change without a migration.
    pub fn slug(self) -> &'static str {
        use Right::*;
        match self {
            VoucherRead => "BILAG_LES",
            AttachmentWrite => "VEDLEGG_SKRIV",
            VoucherUpload => "BILAG_LAST_OPP",
            VoucherPost => "BILAG_BOKFOR",
            PeriodLock => "PERIODE_LAAS",
            ReportRead => "RAPPORT_LES",
            VatSchemeAdmin => "MVA_ORDNING_ADMIN",
            InvoiceRead => "FAKTURA_LES",
            InvoiceWrite => "FAKTURA_SKRIV",
            InvoiceSend => "FAKTURA_SEND",
            InvoiceTemplateRead => "FAKTURAMAL_LES",
            InvoiceTemplateWrite => "FAKTURAMAL_SKRIV",
            QuoteRead => "TILBUD_LES",
            QuoteWrite => "TILBUD_SKRIV",
            ReminderRead => "PURRING_LES",
            ReminderWrite => "PURRING_SKRIV",
            SubledgerRead => "RESKONTRO_LES",
            SubledgerWrite => "RESKONTRO_SKRIV",
            BankRead => "BANK_LES",
            BankReconcile => "BANK_AVSTEM",
            OcrRead => "OCR_LES",
            OcrImport => "OCR_IMPORT",
            PaymentRead => "BETALING_LES",
            PaymentCreate => "BETALING_OPPRETT",
            PaymentApprove => "BETALING_GODKJENN",
            PaymentSettle => "BETALING_OPPGJOR",
            CurrencyRead => "VALUTA_LES",
            CurrencyWrite => "VALUTA_SKRIV",
            ProductRead => "PRODUKT_LES",
            ProductWrite => "PRODUKT_SKRIV",
            InventoryRead => "LAGER_LES",
            InventoryWrite => "LAGER_SKRIV",
            AssetRead => "ANLEGG_LES",
            AssetWrite => "ANLEGG_SKRIV",
            TimesheetReadOwn => "TIMER_LES_EGNE",
            TimesheetReadAll => "TIMER_LES_ALLE",
            TimesheetReportRead => "TIMER_RAPPORT_LES",
            TimesheetWriteOwn => "TIMER_SKRIV_EGNE",
            TimesheetWriteAll => "TIMER_SKRIV_ALLE",
            TimesheetBill => "TIMER_FAKTURER",
            TimesheetRateWrite => "TIMER_SATS_SKRIV",
            TimesheetLock => "TIMER_LAAS",
            ExpenseReadOwn => "UTLEGG_LES_EGNE",
            ExpenseReadAll => "UTLEGG_LES_ALLE",
            ExpenseWriteOwn => "UTLEGG_SKRIV_EGNE",
            ExpenseApprove => "UTLEGG_GODKJENN",
            ExpensePay => "UTLEGG_UTBETAL",
            PayrollRead => "LONN_LES",
            PayslipReadOwn => "LONNSSLIPP_LES_EGEN",
            PayslipReadAll => "LONNSSLIPP_LES_ALLE",
            PayrollWrite => "LONN_SKRIV",
            PayrollRun => "LONN_KJOR",
            BudgetRead => "BUDSJETT_LES",
            BudgetWrite => "BUDSJETT_SKRIV",
            DimensionRead => "DIMENSJON_LES",
            DimensionWrite => "DIMENSJON_SKRIV",
            ShareRegisterRead => "AKSJEBOK_LES",
            ShareRegisterWrite => "AKSJEBOK_SKRIV",
            ApprovalRead => "ATTESTERING_LES",
            ApprovalPerform => "ATTESTERING_UTFOR",
            ApprovalAdmin => "ATTESTERING_ADMIN",
            InboundMailRead => "EPOST_INN_LES",
            InboundMailAdmin => "EPOST_INN_ADMIN",
            AnchorRead => "FORANKRING_LES",
            CompanyRead => "SELSKAP_LES",
            CompanyAdmin => "SELSKAP_ADMIN",
            MemberAdmin => "MEDLEM_ADMIN",
            ContactWrite => "KONTAKT_SKRIV",
            EngagementRead => "OPPDRAG_LES",
            EngagementAdmin => "OPPDRAG_ADMIN",
            IntegrationRead => "INTEGRASJON_LES",
            IntegrationAdmin => "INTEGRASJON_ADMIN",
            MigrationAdmin => "MIGRERING_ADMIN",
        }
    }

    /// The whole vocabulary, in the order the enum is written.
    pub const ALLE: &'static [Right] = &ALL_RIGHTS;

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
    pub fn mutates(self) -> bool {
        use Right::*;
        match self {
            // Reading — always open.
            VoucherRead | ReportRead | InvoiceRead | InvoiceTemplateRead | QuoteRead
            | ReminderRead | SubledgerRead | BankRead | OcrRead | PaymentRead | CurrencyRead
            | ProductRead | InventoryRead | AssetRead | TimesheetReadOwn | TimesheetReadAll
            | TimesheetReportRead | ExpenseReadOwn | ExpenseReadAll | PayrollRead
            | PayslipReadOwn | PayslipReadAll | BudgetRead | DimensionRead | ShareRegisterRead
            | ApprovalRead | InboundMailRead | AnchorRead | CompanyRead | EngagementRead
            | IntegrationRead => false,

            // Governance of the company — open even when the abonnement
            // is blocked (see above).
            CompanyAdmin | MemberAdmin | EngagementAdmin | IntegrationAdmin | InboundMailAdmin => {
                false
            }

            // Documenting what is ALREADY booked — open (#85).
            // Bokføringsloven §10 requires booked information to be
            // documented, and documentation legitimately arrives after
            // the posting. Blocking it would let a missed payment stop
            // the bokføringspliktige from meeting a statutory duty on
            // entries that already stand, which is the hovedbok taken
            // hostage — the one thing the abonnement must never do.
            // docs/perioder.md already draws this line for LOCKED
            // periods, where attaching is likewise allowed: an
            // attachment carries nothing new into the accounts.
            //
            // BilagLastOpp stays blocked, deliberately: the innboks is
            // where NEW documents arrive to be booked, not where
            // existing entries get their documentation.
            AttachmentWrite => false,

            // Everything that carries the accounts forward — blocked.
            VoucherUpload | VoucherPost | PeriodLock | VatSchemeAdmin | InvoiceWrite
            | InvoiceSend | InvoiceTemplateWrite | QuoteWrite | ReminderWrite | SubledgerWrite
            | ContactWrite | BankReconcile | OcrImport | PaymentCreate | PaymentApprove
            | PaymentSettle | CurrencyWrite | ProductWrite | InventoryWrite | AssetWrite
            | TimesheetWriteOwn | TimesheetWriteAll | TimesheetBill | TimesheetRateWrite
            | TimesheetLock | ExpenseWriteOwn | ExpenseApprove | ExpensePay | PayrollWrite
            | PayrollRun | BudgetWrite | DimensionWrite | ShareRegisterWrite | ApprovalPerform
            | ApprovalAdmin | MigrationAdmin => true,
        }
    }

    /// Turns a stored name back into a rettighet.
    ///
    /// Unknown names give `None` and are **ignored** where they are used
    /// (#60): the database does not know the vocabulary, so a role cannot
    /// promise a rettighet nobody enforces. If a version is rolled back
    /// that does not know a new rettighet, it disappears — it does not
    /// turn into something else.
    pub fn from_slug(s: &str) -> Option<Right> {
        Right::ALLE.iter().copied().find(|r| r.slug() == s)
    }

    /// Whether the rettighet may be placed in a **custom** role (#60).
    ///
    /// No for anything that governs WHO HAS ACCESS. A role that can
    /// change access can give itself everything else, and then the rest
    /// of the boundary is decoration. Those rettigheter stay with admin,
    /// which is a role a company cannot rewrite.
    pub fn can_be_delegated(self) -> bool {
        !matches!(
            self,
            Right::MemberAdmin
                | Right::EngagementAdmin
                | Right::IntegrationAdmin
                | Right::CompanyAdmin
        )
    }

    /// The area the rettighet belongs to — the same split as the
    /// portal's menu, so a grid of rettigheter can be read by someone who
    /// knows the product rather than the code.
    pub fn group(self) -> &'static str {
        use Right::*;
        match self {
            VoucherRead => "Bilag",
            AttachmentWrite => "Bilag",
            VoucherUpload => "Bilag",
            VoucherPost => "Bilag",
            PeriodLock => "Bilag",
            ReportRead => "Rapporter",
            VatSchemeAdmin => "Rapporter",
            InvoiceRead => "Faktura",
            InvoiceWrite => "Faktura",
            InvoiceSend => "Faktura",
            InvoiceTemplateRead => "Faktura",
            InvoiceTemplateWrite => "Faktura",
            QuoteRead => "Faktura",
            QuoteWrite => "Faktura",
            ReminderRead => "Faktura",
            ReminderWrite => "Faktura",
            SubledgerRead => "Reskontro",
            SubledgerWrite => "Reskontro",
            BankRead => "Bank",
            BankReconcile => "Bank",
            OcrRead => "Bank",
            OcrImport => "Bank",
            PaymentRead => "Betaling",
            PaymentCreate => "Betaling",
            PaymentApprove => "Betaling",
            PaymentSettle => "Betaling",
            CurrencyRead => "Bank",
            CurrencyWrite => "Bank",
            ProductRead => "Produkter",
            ProductWrite => "Produkter",
            InventoryRead => "Produkter",
            InventoryWrite => "Produkter",
            AssetRead => "Anlegg",
            AssetWrite => "Anlegg",
            TimesheetReadOwn => "Timer",
            TimesheetReadAll => "Timer",
            TimesheetReportRead => "Timer",
            TimesheetWriteOwn => "Timer",
            TimesheetWriteAll => "Timer",
            TimesheetBill => "Timer",
            TimesheetRateWrite => "Timer",
            TimesheetLock => "Timer",
            ExpenseReadOwn => "Utlegg",
            ExpenseReadAll => "Utlegg",
            ExpenseWriteOwn => "Utlegg",
            ExpenseApprove => "Utlegg",
            ExpensePay => "Utlegg",
            PayrollRead => "Lønn",
            PayslipReadOwn => "Lønn",
            PayslipReadAll => "Lønn",
            PayrollWrite => "Lønn",
            PayrollRun => "Lønn",
            BudgetRead => "Rapporter",
            BudgetWrite => "Rapporter",
            DimensionRead => "Dimensjoner",
            DimensionWrite => "Dimensjoner",
            ShareRegisterRead => "Aksjonærer",
            ShareRegisterWrite => "Aksjonærer",
            ApprovalRead => "Attestering",
            ApprovalPerform => "Attestering",
            ApprovalAdmin => "Attestering",
            InboundMailRead => "Bilag",
            InboundMailAdmin => "Bilag",
            AnchorRead => "Rapporter",
            CompanyRead => "Selskap",
            CompanyAdmin => "Selskap",
            MemberAdmin => "Selskap",
            ContactWrite => "Reskontro",
            EngagementRead => "Selskap",
            EngagementAdmin => "Selskap",
            IntegrationRead => "Selskap",
            IntegrationAdmin => "Selskap",
            MigrationAdmin => "Selskap",
        }
    }

    /// What the rettighet lets you do, in Norwegian.
    ///
    /// `TIMER_LES_ALLE` is for us; "Se alles timer" is for whoever is
    /// composing a role. The text belongs here and not in the portal:
    /// then there is only one list, and a new rettighet cannot be left
    /// standing without an explanation.
    pub fn description(self) -> &'static str {
        use Right::*;
        match self {
            VoucherRead => "Se bilag og vedlegg",
            AttachmentWrite => "Legge vedlegg på et bilag",
            VoucherUpload => "Sende dokument til innboksen",
            VoucherPost => "Bokføre fra innboksen",
            PeriodLock => "Låse en periode",
            ReportRead => "Se regnskapsrapportene",
            VatSchemeAdmin => "Endre mva-terminordning",
            InvoiceRead => "Se fakturaer",
            InvoiceWrite => "Utstede faktura og kreditnota",
            InvoiceSend => "Sende faktura på e-post",
            InvoiceTemplateRead => "Se repeterende fakturaer",
            InvoiceTemplateWrite => "Endre repeterende fakturaer",
            QuoteRead => "Se tilbud og ordre",
            QuoteWrite => "Lage tilbud og ordre",
            ReminderRead => "Se purringer og forfalte krav",
            ReminderWrite => "Sende purring og inkassovarsel",
            SubledgerRead => "Se kunder, leverandører og åpne poster",
            SubledgerWrite => "Endre kontakter og matche åpne poster",
            BankRead => "Se bankavstemming",
            BankReconcile => "Importere kontoutdrag og matche",
            OcrRead => "Se OCR-innbetalinger",
            OcrImport => "Importere OCR-fil",
            PaymentRead => "Se betalingslister",
            PaymentCreate => "Opprette betalingsliste",
            PaymentApprove => "Godkjenne betalingsliste",
            PaymentSettle => "Registrere at betalingene er utført",
            CurrencyRead => "Se valutakurser",
            CurrencyWrite => "Legge inn og hente valutakurser",
            ProductRead => "Se produktregisteret",
            ProductWrite => "Endre produktregisteret",
            InventoryRead => "Se lagerbeholdning",
            InventoryWrite => "Registrere lagerbevegelser og varetelling",
            AssetRead => "Se anleggsregisteret",
            AssetWrite => "Registrere, avskrive og avhende anleggsmidler",
            TimesheetReadOwn => "Se sine egne timer",
            TimesheetReadAll => "Se alles timer",
            TimesheetReportRead => "Se timeoversikt per prosjekt og ufakturert",
            TimesheetWriteOwn => "Føre sine egne timer",
            TimesheetWriteAll => "Rette alles timer",
            TimesheetBill => "Fakturere førte timer",
            TimesheetRateWrite => {
                "Sette timesatser på prosjekter og overstyre sats på timeføringer"
            }
            TimesheetLock => "Låse timelisten for en måned",
            ExpenseReadOwn => "Se sine egne utlegg",
            ExpenseReadAll => "Se alles utlegg",
            ExpenseWriteOwn => "Sende inn eget utlegg og kjøregodtgjørelse",
            ExpenseApprove => "Godkjenne og avvise utlegg",
            ExpensePay => "Registrere utbetaling av utlegg",
            PayrollRead => "Se ansattregisteret og lønnskjøringene",
            PayslipReadOwn => "Se sin egen lønnsslipp",
            PayslipReadAll => "Se alles lønnsslipper",
            PayrollWrite => "Registrere ansatte",
            PayrollRun => "Kjøre lønn",
            BudgetRead => "Se budsjett og avviksrapport",
            BudgetWrite => "Lage og fastsette budsjett",
            DimensionRead => "Se avdelinger og prosjekter",
            DimensionWrite => "Endre avdelinger og prosjekter",
            ShareRegisterRead => "Se aksjeeierboken",
            ShareRegisterWrite => "Registrere aksjonærer, hendelser og utbytte",
            ApprovalRead => "Se attesteringssporet",
            ApprovalPerform => "Attestere bilag",
            ApprovalAdmin => "Sette attesteringspolicyen",
            InboundMailRead => "Se e-post inn til innboksen",
            InboundMailAdmin => "Styre mottaksadresse og avsenderliste",
            AnchorRead => "Se forankringen av hovedboken",
            CompanyRead => "Se firmaopplysningene",
            CompanyAdmin => "Endre firmaopplysningene",
            MemberAdmin => "Gi og fjerne tilgang",
            ContactWrite => "Endre kontaktinfo på en part",
            EngagementRead => "Se oppdrag",
            EngagementAdmin => "Inngå og avslutte oppdrag",
            IntegrationRead => "Se integrasjoner",
            IntegrationAdmin => "Slippe til en integrasjon",
            MigrationAdmin => "Importere regnskap fra et annet system",
        }
    }

    /// What this rettighet also grants. `_ALLE` grants `_EGNE`: whoever
    /// sees everyone's hours obviously sees their own, and without the
    /// rule every bundle would have to remember both.
    pub fn implies(self) -> &'static [Right] {
        match self {
            Right::TimesheetReadAll => &[Right::TimesheetReadOwn],
            Right::TimesheetWriteAll => &[Right::TimesheetWriteOwn, Right::TimesheetReadOwn],
            Right::ExpenseReadAll => &[Right::ExpenseReadOwn],
            Right::PayslipReadAll => &[Right::PayslipReadOwn],
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
pub enum Role {
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
    Employee,
    Read,
    /// Reading like `Les`, plus the lønn data an audit needs. Comes only
    /// from an oppdrag of kind `revisjon` — it cannot be assigned
    /// directly.
    Auditor,
    Bookkeeping,
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
pub const EMPLOYEE_BUNDLE: &[Right] = &[
    // Logging and seeing one's own hours. The prosjekt register must be
    // readable, or there is nothing to log the hours against.
    Right::TimesheetReadOwn,
    Right::TimesheetWriteOwn,
    Right::DimensionRead,
    // Submitting and following one's own refusjonskrav.
    Right::ExpenseReadOwn,
    Right::ExpenseWriteOwn,
    // Their own payslip — not their colleagues'.
    Right::PayslipReadOwn,
    // Receipt photos from the phone (#48). Uploading is not posting: the
    // document lands in the innboks and waits for someone with
    // BILAG_BOKFOR.
    Right::VoucherUpload,
];

/// What anyone with access to the company gets. Revisor lives here:
/// reading everything, changing nothing.
const LES_BUNT: &[Right] = &[
    Right::VoucherRead,
    Right::ReportRead,
    Right::InvoiceRead,
    Right::InvoiceTemplateRead,
    Right::QuoteRead,
    Right::ReminderRead,
    Right::SubledgerRead,
    Right::BankRead,
    Right::OcrRead,
    Right::PaymentRead,
    Right::CurrencyRead,
    Right::ProductRead,
    Right::InventoryRead,
    Right::AssetRead,
    Right::TimesheetReadOwn,
    Right::TimesheetReportRead,
    Right::ExpenseReadAll,
    Right::BudgetRead,
    Right::DimensionRead,
    Right::ShareRegisterRead,
    Right::ApprovalRead,
    Right::InboundMailRead,
    Right::AnchorRead,
    Right::CompanyRead,
    Right::EngagementRead,
    Right::IntegrationRead,
];

/// What revisor adds to reading: the lønn data (#55).
///
/// Lønn is subject to audit — it is a material cost, and forskuddstrekk
/// and arbeidsgiveravgift are statutory figures a revisor must be able to
/// check. So the answer is "yes, a revisor sees lønn", but now it is an
/// **explicit yes** rather than a side effect of revisor and an internal
/// reader having been the same role.
pub const AUDITOR_BUNDLE: &[Right] = &[
    Right::PayrollRead,
    Right::PayslipReadAll,
    // Hours are billing evidence — the audit reads everyone's, like lønn.
    Right::TimesheetReadAll,
];

/// What bokføring adds: everything that changes the hovedbok or ends there.
const BOKFORING_BUNT: &[Right] = &[
    Right::AttachmentWrite,
    Right::VoucherUpload,
    Right::VoucherPost,
    Right::PeriodLock,
    Right::InvoiceWrite,
    Right::InvoiceSend,
    Right::InvoiceTemplateWrite,
    Right::QuoteWrite,
    Right::ReminderWrite,
    Right::SubledgerWrite,
    Right::BankReconcile,
    Right::OcrImport,
    Right::PaymentCreate,
    Right::PaymentApprove,
    Right::PaymentSettle,
    Right::CurrencyWrite,
    Right::ProductWrite,
    Right::InventoryWrite,
    Right::AssetWrite,
    Right::TimesheetWriteOwn,
    // Whoever bills the hours needs to see whose they are.
    Right::TimesheetReadAll,
    Right::TimesheetBill,
    // The rates live on the project; whoever keeps the books sets them.
    Right::TimesheetRateWrite,
    Right::ExpenseWriteOwn,
    Right::ExpenseApprove,
    Right::ExpensePay,
    Right::PayrollRead,
    Right::PayslipReadAll,
    Right::PayrollWrite,
    Right::PayrollRun,
    Right::BudgetWrite,
    Right::DimensionWrite,
    Right::ShareRegisterWrite,
    Right::ApprovalPerform,
    Right::ContactWrite,
];

/// What admin adds: governing the company and who gets in.
const ADMIN_BUNT: &[Right] = &[
    Right::VatSchemeAdmin,
    Right::TimesheetWriteAll,
    Right::TimesheetLock,
    Right::ApprovalAdmin,
    Right::InboundMailAdmin,
    Right::CompanyAdmin,
    // Governing who gets in. Whoever holds this can give themselves
    // everything else, so it belongs with admin and nobody else.
    Right::MemberAdmin,
    Right::EngagementAdmin,
    Right::IntegrationAdmin,
    Right::MigrationAdmin,
];

impl Role {
    /// Unknown values grant NO rettigheter — see [`Rolle::Ukjent`].
    pub fn from_db(s: &str) -> Self {
        match s {
            "admin" => Self::Admin,
            "bokforing" => Self::Bookkeeping,
            "ansatt" => Self::Employee,
            "les" => Self::Read,
            "revisor" => Self::Auditor,
            _ => Self::Ukjent,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Bookkeeping => "bokforing",
            Self::Read => "les",
            Self::Employee => "ansatt",
            Self::Auditor => "revisor",
            Self::Ukjent => "ukjent",
        }
    }

    pub fn is_admin(self) -> bool {
        self == Self::Admin
    }

    /// What the role is FOR, in the user's language — the invitation
    /// guidance in the portal renders these (#79). One copy, kept next
    /// to the bundles that make the claims true: whoever changes a
    /// bundle below must reread this text.
    pub fn description(self) -> &'static str {
        match self {
            Self::Employee => {
                "Selvbetjening for en lønnsmottaker: fører egne timer og utlegg, \
                 ser egne lønnsslipper og laster opp kvitteringer. Ser ikke hovedboken."
            }
            Self::Read => "Ser hele regnskapet, endrer ingenting. Lønn er unntatt.",
            Self::Auditor => {
                "Lesetilgang pluss lønn og kjedeverifikasjon. Tildeles ikke direkte — \
                 rollen følger av et revisjonsoppdrag."
            }
            Self::Bookkeeping => {
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
    fn bunter(self) -> &'static [&'static [Right]] {
        match self {
            Self::Ukjent => &[],
            // Ansatt is NOT nested under the other three — it is its own
            // composition, and that is the whole point of the model.
            Self::Employee => &[EMPLOYEE_BUNDLE],
            Self::Read => &[LES_BUNT],
            Self::Auditor => &[LES_BUNT, AUDITOR_BUNDLE],
            Self::Bookkeeping => &[LES_BUNT, BOKFORING_BUNT],
            Self::Admin => &[LES_BUNT, BOKFORING_BUNT, ADMIN_BUNT],
        }
    }

    /// Whether the role has the rettighet, directly or because another
    /// rettighet implies it.
    pub fn has(self, right: Right) -> bool {
        self.bunter()
            .iter()
            .flat_map(|b| b.iter())
            .any(|r| *r == right || r.implies().contains(&right))
    }

    /// Every rettighet the role actually grants, implications included.
    /// The portal uses this to show what a role means (#61).
    pub fn rights(self) -> Vec<Right> {
        let mut ut: Vec<Right> = self
            .bunter()
            .iter()
            .flat_map(|b| b.iter())
            .flat_map(|r| std::iter::once(*r).chain(r.implies().iter().copied()))
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
pub struct Access {
    roles: Vec<Role>,
    /// Rettigheter from the company's OWN roles (#60), already filtered
    /// against the vocabulary and against [`Rett::kan_delegeres`].
    egendefinerte: Vec<Right>,
    /// The caller got here as a machine (an `integration_grant`, #45).
    ///
    /// Since #101 an integration holds a REAL role — `admin` included,
    /// when the owner decides it — so the ceiling cannot live at grant
    /// time any more. It lives here instead: whatever the role says, a
    /// machine never holds a rettighet that governs WHO HAS ACCESS.
    /// One seam, so it covers roles added later and rows written before
    /// the rule existed.
    maskin: bool,
}

impl Access {
    pub fn has(&self, right: Right) -> bool {
        if self.maskin && !right.can_be_delegated() {
            return false;
        }
        // The implication (`_ALLE` medfører `_EGNE`) is a property of the
        // rettighet, not of the role kind — a custom role holding
        // TIMER_SKRIV_ALLE can correct everyone's hours and therefore
        // also its own, whether or not the admin ticked both boxes.
        self.roles.iter().any(|r| r.has(right))
            || self
                .egendefinerte
                .iter()
                .any(|r| *r == right || r.implies().contains(&right))
    }

    /// Whether this is a machine caller — the attestering policy and the
    /// portal both want to know.
    pub fn is_machine(&self) -> bool {
        self.maskin
    }

    pub fn is_admin(&self) -> bool {
        self.roles.iter().any(|r| r.is_admin())
    }

    /// The role names, strongest first — for display and logging, never
    /// for deciding access.
    pub fn roles(&self) -> Vec<&'static str> {
        self.roles.iter().map(|r| r.slug()).collect()
    }

    /// Every rettighet the person holds, as slugs — implications
    /// included, sorted and deduplicated. `/me` carries this so the
    /// portal can show only controls that will succeed; DISPLAY ONLY,
    /// the guard below remains the enforcement.
    pub fn right_slugs(&self) -> Vec<&'static str> {
        let mut ut: Vec<&'static str> = self
            .roles
            .iter()
            .flat_map(|r| r.rights())
            .chain(
                self.egendefinerte
                    .iter()
                    .flat_map(|r| std::iter::once(*r).chain(r.implies().iter().copied())),
            )
            // Same filter as `har`, or `/me` would promise a machine
            // controls that are going to fail.
            .filter(|r| !self.maskin || r.can_be_delegated())
            .map(|r| r.slug())
            .collect();
        ut.sort_unstable();
        ut.dedup();
        ut
    }
}

/// Builds the person's [`Tilgang`] for one company without requiring any
/// particular rettighet — `None` when they have no access at all. `/me`
/// uses this; every guarded endpoint goes through [`krev`].
pub async fn lookup(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
) -> Result<Option<Access>, ApiError> {
    // `company_access_for_person` rather than `company_roles`, because
    // the route in (`via`) decides the machine ceiling below and comes
    // out of the same single query — no extra cost on the hot path.
    let tilganger: Vec<regnmed_db::CompanyAccess> =
        regnmed_db::company_access_for_person(&state.pool, person_id)
            .await?
            .into_iter()
            .filter(|a| a.company_id == company_id)
            .collect();
    if tilganger.is_empty() {
        return Ok(None);
    }
    let maskin = tilganger.iter().any(|a| a.via == "integrasjon");
    let mut name: Vec<String> = tilganger.into_iter().map(|a| a.access).collect();
    name.sort();
    name.dedup();
    let roles: Vec<Role> = name.iter().map(|s| Role::from_db(s)).collect();
    let ukjente: Vec<String> = name
        .iter()
        .zip(&roles)
        .filter(|(_, r)| **r == Role::Ukjent)
        .map(|(n, _)| n.clone())
        .collect();
    let egendefinerte = if ukjente.is_empty() {
        Vec::new()
    } else {
        regnmed_db::roles::rights_for(&state.pool, company_id, &ukjente)
            .await?
            .iter()
            .filter_map(|s| Right::from_slug(s))
            .filter(|r| r.can_be_delegated())
            .collect()
    };
    Ok(Some(Access {
        roles,
        egendefinerte,
        maskin,
    }))
}

/// Looks up the person's access to the company and requires `rett`.
///
/// **An unknown company and missing access both give 404.** Someone
/// without access must not learn that the company exists; that is the
/// same rule as everywhere else in the API (docs/auth.md).
pub async fn require(
    state: &AppState,
    person_id: Uuid,
    company_id: Uuid,
    right: Right,
) -> Result<Access, ApiError> {
    // Names that are not built-in may be the company's own roles (the
    // lookup in `slaa_opp` only costs an extra query then). A name the
    // database carries but the code does not know grants nothing — and a
    // rettighet that cannot be delegated stays inert even if it had
    // somehow found its way into the table.
    let Some(access) = lookup(state, person_id, company_id).await? else {
        return Err(ApiError::NotFound);
    };
    if !access.has(right) {
        return Err(ApiError::Forbidden(manglende(right)));
    }

    // The abonnement block (#65, docs/abonnement.md) — AFTER the access
    // check, so an outsider still gets 404 and never learns anything
    // about the company's abonnement. Only changing rettigheter
    // costs the lookup; reading and export always go through.
    if right.mutates() && regnmed_db::subscription::blocked(&state.pool, company_id).await? {
        return Err(ApiError::Forbidden(
            "abonnementet er utløpt — lesing og eksport virker som før, men endringer er sperret til abonnementet er i orden (docs/abonnement.md)",
        ));
    }
    Ok(access)
}

/// The error message names the missing rettighet. Whoever gets a 403
/// should be able to tell their admin what they need, without us having
/// to dig through the log.
fn manglende(right: Right) -> &'static str {
    // ApiError::Forbidden carries &'static str, so the slug is used directly.
    right.slug()
}

/// Unit tests for the vocabulary.
///
/// **These cannot catch a rettighet sitting in the WRONG bundle.** They
/// derive their expectations from the bundles, so moving `PRODUKT_SKRIV`
/// from bokføring to reading still passes — tried, deliberately.
/// `tests/grupper/access.rs` is the guard there: it asks a real server
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
    fn every_right() -> Vec<Right> {
        let mut v: Vec<Right> = LES_BUNT
            .iter()
            .chain(AUDITOR_BUNDLE)
            .chain(BOKFORING_BUNT)
            .chain(ADMIN_BUNT)
            .flat_map(|r| std::iter::once(*r).chain(r.implies().iter().copied()))
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
    fn the_employee_bundle_uses_only_known_rights() {
        let kjente: std::collections::BTreeSet<_> = every_right().into_iter().collect();
        for r in EMPLOYEE_BUNDLE {
            assert!(
                kjente.contains(r),
                "{} finnes ikke i noen nivåbunt",
                r.slug()
            );
        }
    }

    /// The most important thing about the ansatt role is what it does NOT give.
    #[test]
    fn an_employee_cannot_reach_the_hovedbok() {
        let a = Role::Employee;
        for nektet in [
            Right::VoucherRead,
            Right::VoucherPost,
            Right::ReportRead,
            Right::InvoiceRead,
            Right::InvoiceWrite,
            Right::BankRead,
            Right::SubledgerRead,
            Right::PaymentRead,
            Right::PayrollRead,
            Right::PayslipReadAll,
            Right::TimesheetReadAll,
            Right::TimesheetReportRead,
            Right::ExpenseReadAll,
            Right::ExpenseApprove,
            Right::CompanyRead,
            Right::MemberAdmin,
        ] {
            assert!(!a.has(nektet), "ansatt skulle ikke hatt {}", nektet.slug());
        }
        // And what it SHALL have.
        for gitt in [
            Right::TimesheetReadOwn,
            Right::TimesheetWriteOwn,
            Right::ExpenseWriteOwn,
            Right::ExpenseReadOwn,
            Right::PayslipReadOwn,
            Right::VoucherUpload,
            Right::DimensionRead,
        ] {
            assert!(a.has(gitt), "ansatt mangler {}", gitt.slug());
        }
    }

    /// The slugs are what #60 stores in the database. Two rettigheter with
    /// the same name would make a stored role ambiguous.
    #[test]
    fn the_slugs_are_unique() {
        let mut sett = std::collections::BTreeSet::new();
        for r in every_right() {
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
        for r in every_right() {
            if Role::Read.has(r) {
                assert!(Role::Auditor.has(r), "revisor mangler {}", r.slug());
                assert!(Role::Bookkeeping.has(r), "bokforing mangler {}", r.slug());
            }
            if Role::Auditor.has(r) {
                assert!(Role::Bookkeeping.has(r), "bokforing mangler {}", r.slug());
            }
            if Role::Bookkeeping.has(r) {
                assert!(Role::Admin.has(r), "admin mangler {}", r.slug());
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
            assert!(Role::Read.has(*r), "les mangler {}", r.slug());
        }
        for r in BOKFORING_BUNT {
            assert!(!Role::Read.has(*r), "les skulle ikke hatt {}", r.slug());
            assert!(Role::Bookkeeping.has(*r), "bokforing mangler {}", r.slug());
        }
        for r in ADMIN_BUNT {
            assert!(!Role::Read.has(*r), "les skulle ikke hatt {}", r.slug());
            assert!(
                !Role::Bookkeeping.has(*r),
                "bokforing skulle ikke hatt {}",
                r.slug()
            );
            assert!(Role::Admin.has(*r), "admin mangler {}", r.slug());
        }
    }

    /// `_ALLE` implies `_EGNE`. Without the rule an admin holding
    /// TIMER_SKRIV_ALLE could not log their own hours unless the bundle
    /// remembered both — and that kind of "remember both" is exactly what
    /// slips.
    #[test]
    fn alle_implies_egne() {
        assert!(
            Right::TimesheetReadAll
                .implies()
                .contains(&Right::TimesheetReadOwn)
        );
        assert!(
            Right::TimesheetWriteAll
                .implies()
                .contains(&Right::TimesheetWriteOwn)
        );
        // Admin has _ALLE and therefore gets _EGNE without it being in the bundle.
        assert!(!ADMIN_BUNT.contains(&Right::TimesheetWriteOwn));
        assert!(Role::Admin.has(Right::TimesheetWriteOwn));
        // And it does not hold the other way: own does not give everyone's.
        assert!(!Role::Bookkeeping.has(Right::TimesheetWriteAll));
        assert!(Role::Bookkeeping.has(Right::TimesheetWriteOwn));
    }

    #[test]
    fn the_role_round_trips_against_the_database_values() {
        for slug in ["admin", "bokforing", "les", "ansatt", "revisor"] {
            assert_eq!(Role::from_db(slug).slug(), slug);
        }
    }

    /// A role value we do not recognise grants NO rettigheter.
    ///
    /// Before the ansatt role, unknown fell back to `Les`, which was then
    /// the weakest. Now `Ansatt` and `Les` are not comparable, and during
    /// a rolling deploy an old binary can meet a role it does not know —
    /// reading "ansatt" as "les" would have been an upgrade.
    #[test]
    fn an_unknown_role_grants_no_rights() {
        for ukjent in ["superbruker", "", "Ansatt", "LES"] {
            let r = Role::from_db(ukjent);
            assert_eq!(r, Role::Ukjent, "«{ukjent}»");
            assert!(r.rights().is_empty(), "«{ukjent}» ga rettigheter");
        }
    }

    /// A revisor sees lønn, an internal reader does not (#55). That is
    /// deliberate: both read everything else the same way, but only one
    /// has an audit duty that requires the lønn data.
    #[test]
    fn a_revisor_sees_lonn_but_an_internal_reader_does_not() {
        for right in [Right::PayrollRead, Right::PayslipReadAll] {
            assert!(Role::Auditor.has(right), "revisor mangler {}", right.slug());
            assert!(
                !Role::Read.has(right),
                "les skulle ikke hatt {}",
                right.slug()
            );
            assert!(
                !Role::Employee.has(right),
                "ansatt skulle ikke hatt {}",
                right.slug()
            );
            assert!(
                Role::Bookkeeping.has(right),
                "bokforing mangler {}",
                right.slug()
            );
        }
        // A revisor is otherwise exactly a reader — and writes nothing.
        assert!(Role::Auditor.has(Right::ReportRead));
        assert!(!Role::Auditor.has(Right::VoucherPost));
        assert!(!Role::Auditor.has(Right::PayrollRun));
        // Their own slip belongs to every employee, independently of this.
        assert!(Role::Employee.has(Right::PayslipReadOwn));
    }

    /// Every rettighet must have an explanation and a group. Without the
    /// test a new rettighet could be left standing in the grid without
    /// text — and a checkbox without an explanation is worse than no box.
    #[test]
    fn every_right_has_a_description_and_a_group() {
        for r in Right::ALLE {
            assert!(
                !r.description().is_empty(),
                "{} mangler forklaring",
                r.slug()
            );
            assert!(!r.group().is_empty(), "{} mangler gruppe", r.slug());
            // The description is for a human, not the slug again.
            assert_ne!(r.description(), r.slug(), "{}", r.slug());
        }
    }

    /// The slug must read back — it is what #60 stores.
    #[test]
    fn the_slug_round_trips() {
        for r in Right::ALLE {
            assert_eq!(Right::from_slug(r.slug()), Some(*r), "{}", r.slug());
        }
        assert_eq!(Right::from_slug("FAKTURA_ALT"), None);
        assert_eq!(Right::from_slug(""), None);
    }

    /// The rettighet list the portal shows must be complete and sorted
    /// without duplicates, including what is implied.
    #[test]
    fn the_right_list_is_complete() {
        let admin = Role::Admin.rights();
        assert_eq!(admin.len(), every_right().len());
        let les = Role::Read.rights();
        assert!(les.contains(&Right::TimesheetReadOwn));
        assert!(!les.contains(&Right::TimesheetReadAll));
    }
}
