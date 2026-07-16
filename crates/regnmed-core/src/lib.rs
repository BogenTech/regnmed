//! Domain model for the regnmed ledger.
//!
//! This crate has no database or I/O dependencies: it defines money,
//! vouchers, the double-entry invariants, and the canonical hashing that
//! makes the ledger chain tamper-evident. Everything here must stay
//! deterministic — the same voucher content must hash identically forever,
//! on any machine, or chain verification breaks.

pub mod error;
pub mod hash;
pub mod money;
pub mod voucher;

pub use error::LedgerError;
pub use money::Ore;
