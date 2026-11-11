//! PostgreSQL persistence for the regnmed ledger.
//!
//! Queries use sqlx's runtime API for now so the workspace builds without a
//! live database. Once the dev database is a fixture of CI, migrate hot
//! paths to `sqlx::query!` + `cargo sqlx prepare` for compile-time checking.

pub mod accrual;
pub mod agent_task;
pub mod agentllm;
pub mod amelding_submission;
pub mod anchor;
pub mod approval;
pub mod asset;
pub mod attachment;
pub mod audit;
pub mod balance_documentation;
pub mod bank;
pub mod blob;
pub mod budget;
pub mod cash_register;
pub mod chart_of_accounts;
pub mod currency;
pub mod dimension;
pub mod dispatch;
pub mod ehf;
pub mod email;
pub mod engagement;
pub mod expenses;
pub mod firm_members;
pub mod inbox;
pub mod integration;
pub mod invoice;
pub mod invoice_template;
pub mod job;
pub mod ledger;
pub mod ledger_totals;
pub mod marketplace;
pub mod members;
pub mod migration;
pub mod ocr;
pub mod opening;
pub mod payments;
pub mod payroll;
pub mod period;
pub mod platform;
pub mod product;
pub mod rates;
pub mod reminders;
pub mod roles;
pub mod saft;
pub mod saft_import;
pub mod sales_document;
pub mod settings;
pub mod share_register;
pub mod shareholder_return_submission;
pub mod statements;
pub mod subledger;
pub mod subscription;
pub mod subscription_follow_up;
pub mod tenancy;
pub mod timesheet;
pub mod vat;
pub mod year_end;

pub use anchor::*;
pub use approval::*;
pub use asset::*;
pub use attachment::*;
pub use audit::*;
pub use bank::*;
pub use blob::Blobs;
pub use budget::*;
pub use chart_of_accounts::*;
pub use currency::*;
pub use dimension::*;
pub use dispatch::*;
pub use ehf::*;
pub use email::*;
pub use engagement::*;
pub use expenses::*;
pub use inbox::*;
pub use integration::*;
pub use invoice::*;
pub use invoice_template::*;
pub use ledger::*;
pub use ledger_totals::*;
pub use marketplace::*;
pub use migration::*;
pub use ocr::*;
pub use opening::*;
pub use payments::*;
pub use period::*;
pub use platform::*;
pub use product::*;
pub use rates::*;
pub use reminders::*;
pub use saft::*;
pub use saft_import::*;
pub use sales_document::*;
pub use settings::*;
pub use statements::*;
pub use subledger::*;
pub use tenancy::*;
pub use timesheet::*;
pub use vat::*;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Migrations embedded from `crates/regnmed-db/migrations`. sqlx records a
/// checksum per applied migration and refuses to run if an already-applied
/// file changed on disk — treat the migrations directory as append-only.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    // The portal fires ~20 requests on load and each holds a connection for
    // its queries; at 5 per process they queued (2026-11-08). Connections
    // open lazily, so 20 is a ceiling, not a cost: two API replicas plus jobs
    // stay well under Postgres's max_connections of 100.
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}
