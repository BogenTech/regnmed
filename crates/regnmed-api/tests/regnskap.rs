//! Regnskapet ut: rapporter, revisjon, budsjett, kontoplan og migrering.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/budsjett.rs"]
mod budsjett;
#[path = "grupper/kontoplan.rs"]
mod kontoplan;
#[path = "grupper/migrering.rs"]
mod migrering;
#[path = "grupper/nokkeltall.rs"]
mod nokkeltall;
#[path = "grupper/regnskap.rs"]
mod regnskap;
#[path = "grupper/reports.rs"]
mod reports;
#[path = "grupper/revisjon.rs"]
mod revisjon;
#[path = "grupper/saft_migration.rs"]
mod saft_migration;
