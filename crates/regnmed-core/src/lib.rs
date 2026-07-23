//! Domain model for the regnmed ledger.
//!
//! This crate has no database or I/O dependencies: it defines money,
//! vouchers, the double-entry invariants, and the canonical hashing that
//! makes the ledger chain tamper-evident. Everything here must stay
//! deterministic — the same voucher content must hash identically forever,
//! on any machine, or chain verification breaks.

pub mod anchor;
pub mod bank;
pub mod bankcsv;
pub mod camt053;
pub mod error;
pub mod hash;
pub mod invoice;
pub mod kid;
pub mod kontoplan;
pub mod money;
pub mod mva;
pub mod mvamelding;
pub mod ocr;
pub mod orgnr;
pub mod regnskap;
pub mod revisjon;
pub mod saft;
pub mod saft_import;
pub mod sats;
pub mod voucher;
pub(crate) mod xml;

pub use error::LedgerError;
pub use money::Ore;
