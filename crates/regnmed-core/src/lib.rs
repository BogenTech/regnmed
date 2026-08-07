//! Domain model for the regnmed ledger.
//!
//! This crate has no database or I/O dependencies: it defines money,
//! vouchers, the double-entry invariants, and the canonical hashing that
//! makes the ledger chain tamper-evident. Everything here must stay
//! deterministic — the same voucher content must hash identically forever,
//! on any machine, or chain verification breaks.

pub mod abonnement;
pub mod aksjebok;
pub mod aksjonaeroppgave;
pub mod anchor;
pub mod anlegg;
pub mod bank;
pub mod bankcsv;
pub mod bilagstolk;
pub mod budsjett;
pub mod camt053;
pub(crate) mod csvutil;
pub mod ehf;
pub mod ehf_import;
pub mod epost;
pub mod error;
pub mod fakturapdf;
pub mod fnr;
pub mod hash;
pub mod invoice;
pub mod kid;
pub mod kontoplan;
pub mod lager;
pub mod lonn;
pub mod lonnsslipp;
pub mod migreringcsv;
pub mod money;
pub mod mva;
pub mod mvamelding;
pub mod ocr;
pub mod orgnr;
pub mod pain001;
pub mod pdf;
pub mod pdftekst;
pub mod periodisering;
pub mod purring;
pub mod regnskap;
pub mod regnskapsar;
pub mod revisjon;
pub mod saft;
pub mod saft_import;
pub mod sats;
pub mod utlegg;
pub mod valuta;
pub mod voucher;
pub(crate) mod xml;

pub use error::LedgerError;
pub use money::Ore;
