//! Bilagsflyten: innboks, tolkning, e-post inn, vedlegg, dimensjoner, forankring.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

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
