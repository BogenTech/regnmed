//! Clients for the Norwegian government rail.
//!
//! - [`maskinporten`]: machine-to-machine OAuth2 tokens via signed JWT
//!   grants — the shared foundation every Skatteetaten/Altinn API rides
//!   on (delegation lets a regnskapsfører act for client orgs).
//! - [`mvamelding`]: Skatteetaten's mva-melding validation API. The
//!   Altinn3 submission (instance) flow follows once real test
//!   credentials exist; see docs/gov.md.
//!
//! Nothing here touches the ledger: regnmed-core builds the documents,
//! this crate moves them.

pub mod maskinporten;
pub mod mvamelding;
