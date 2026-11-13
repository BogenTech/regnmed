//! Clients for the Norwegian government rail.
//!
//! - [`maskinporten`]: machine-to-machine OAuth2 tokens via signed JWT
//!   grants — the shared foundation every Skatteetaten/Altinn API rides
//!   on (delegation lets a regnskapsfører act for client orgs).
//! - [`mvamelding`]: Skatteetaten's mva-melding validation API. The
//!   Altinn3 submission (instance) flow follows once real test
//!   credentials exist; see docs/gov.md.
//! - [`amelding`]: Skatteetatens a-melding API — submission with a
//!   systembruker token and the feedback via Dialogporten.
//! - [`altinn_events`]: Altinn Events subscriptions, so Skatteetatens
//!   feedback arrives as an event instead of being asked for.
//! - [`tax_card`]: skattekort til arbeidsgiver -- ordering the cards and
//!   reading what each one means for the employee's withholding.
//! - [`tsa`]: RFC 3161 timestamping — the external witness for anchor
//!   roots (docs/anchoring.md).
//!
//! Nothing here touches the ledger: regnmed-core builds the documents,
//! this crate moves them.

pub mod addresses;
pub mod altinn_events;
pub mod amelding;
pub mod brreg;
pub mod finanstilsynet;
pub mod maskinporten;
pub mod norgesbank;
pub mod regnid;
pub mod routing;
pub mod shareholder_return;
pub mod stripe;
pub mod tax_card;
pub mod tsa;
pub mod vat_return;
