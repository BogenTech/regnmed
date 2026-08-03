//! People and assets: hours, utlegg, fixed assets and shareholders.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/aksjonaer.rs"]
mod aksjonaer;
#[path = "grupper/ansattkobling.rs"]
mod ansattkobling;
#[path = "grupper/assets.rs"]
mod assets;
#[path = "grupper/expenses.rs"]
mod expenses;
#[path = "grupper/timesheet.rs"]
mod timesheet;
