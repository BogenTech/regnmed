//! The accounts out: reports, audit, budget, chart of accounts and migration.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/audit.rs"]
mod audit;
#[path = "grupper/budsjett.rs"]
mod budsjett;
#[path = "grupper/kontoplan.rs"]
mod chart_of_accounts;
#[path = "grupper/hovedbok.rs"]
mod hovedbok;
#[path = "grupper/migrering.rs"]
mod migrering;
#[path = "grupper/nokkeltall.rs"]
mod nokkeltall;
#[path = "grupper/regnskap.rs"]
mod regnskap;
#[path = "grupper/reports.rs"]
mod reports;
#[path = "grupper/saft_migration.rs"]
mod saft_migration;
