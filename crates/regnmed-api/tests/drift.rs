//! The bilag flow: innboks, interpretation, inbound e-mail, attachments, dimensions, anchoring.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/anchor.rs"]
mod anchor;
#[path = "grupper/attestering.rs"]
mod attestering;
#[path = "grupper/bilagstolkning.rs"]
mod bilagstolkning;
#[path = "grupper/dimensions.rs"]
mod dimensions;
#[path = "grupper/epost_inn.rs"]
mod epost_inn;
#[path = "grupper/innboks.rs"]
mod innboks;
#[path = "grupper/period_attachments.rs"]
mod period_attachments;
