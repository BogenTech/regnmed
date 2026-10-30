//! Domain model for the regnmed ledger.
//!
//! This crate has no database or I/O dependencies: it defines money,
//! vouchers, the double-entry invariants, and the canonical hashing that
//! makes the ledger chain tamper-evident. Everything here must stay
//! deterministic — the same voucher content must hash identically forever,
//! on any machine, or chain verification breaks.

pub mod accrual;
pub mod aksjebok;
pub mod aksjonaeroppgave;
pub mod anchor;
pub mod assets;
pub mod bank;
pub mod bankcsv;
pub mod budget;
pub mod camt053;
pub mod cash_register;
pub mod chart_of_accounts;
pub(crate) mod csvutil;
pub mod currency;
pub mod ehf;
pub mod ehf_import;
pub mod email;
pub mod error;
pub mod expenses;
pub mod fakturapdf;
pub mod fiscal_year;
pub mod hash;
pub mod inventory;
pub mod invoice;
pub mod kid;
pub mod lonn;
pub mod lonnsslipp;
pub mod migration_csv;
pub mod money;
pub mod mva;
pub mod mvamelding;
pub mod nin;
pub mod ocr;
pub mod orgnumber;
pub mod pain001;
pub mod pdf;
pub mod pdftext;
pub mod purring;
pub mod rates;
pub mod regnskap;
pub mod revisjon;
pub mod saft;
pub mod saft_import;
pub mod secret;
pub mod subscription;
pub mod voucher;
pub mod voucher_hints;
pub(crate) mod xml;

pub use error::LedgerError;
pub use money::Ore;
