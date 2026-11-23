//! People and assets: hours, utlegg, fixed assets and shareholders.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/altinn_system_user.rs"]
mod altinn_system_user;
#[path = "grupper/amelding.rs"]
mod amelding;
#[path = "grupper/ansattkobling.rs"]
mod ansattkobling;
#[path = "grupper/assets.rs"]
mod assets;
#[path = "grupper/expenses.rs"]
mod expenses;
#[path = "grupper/payroll_prefill.rs"]
mod payroll_prefill;
#[path = "grupper/share_register.rs"]
mod share_register;
#[path = "grupper/tax_card.rs"]
mod tax_card;
#[path = "grupper/terms.rs"]
mod terms;
#[path = "grupper/timesheet.rs"]
mod timesheet;
