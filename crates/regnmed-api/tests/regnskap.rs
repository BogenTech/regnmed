//! The accounts out: reports, revisjon, budget, kontoplan and migration.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

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
