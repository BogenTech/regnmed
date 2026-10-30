//! PostgreSQL persistence for the regnmed ledger.
//!
//! Queries use sqlx's runtime API for now so the workspace builds without a
//! live database. Once the dev database is a fixture of CI, migrate hot
//! paths to `sqlx::query!` + `cargo sqlx prepare` for compile-time checking.

pub mod abonnement;
pub mod abonnement_oppfolging;
pub mod accrual;
pub mod agent_task;
pub mod agentllm;
pub mod aksjebok;
pub mod anchor;
pub mod approval;
pub mod asset;
pub mod attachment;
pub mod balance_documentation;
pub mod bank;
pub mod budget;
pub mod cash_register;
pub mod chart_of_accounts;
pub mod currency;
pub mod dimension;
pub mod ehf;
pub mod email;
pub mod engagement;
pub mod expenses;
pub mod firm_members;
pub mod inbox;
pub mod integration;
pub mod invoice;
pub mod invoice_template;
pub mod ledger;
pub mod lonn;
pub mod marketplace;
pub mod members;
pub mod migration;
pub mod mva;
pub mod ocr;
pub mod opening;
pub mod payments;
pub mod period;
pub mod platform;
pub mod product;
pub mod purring;
pub mod rates;
pub mod regnskap;
pub mod reskontro;
pub mod revisjon;
pub mod roles;
pub mod saft;
pub mod saft_import;
pub mod salgsdokument;
pub mod settings;
pub mod tenancy;
pub mod timesheet;
pub mod utsendelse;
pub mod year_end;

pub use anchor::*;
pub use approval::*;
pub use asset::*;
pub use attachment::*;
pub use bank::*;
pub use budget::*;
pub use chart_of_accounts::*;
pub use currency::*;
pub use dimension::*;
pub use ehf::*;
pub use email::*;
pub use engagement::*;
pub use expenses::*;
pub use inbox::*;
pub use integration::*;
pub use invoice::*;
pub use invoice_template::*;
pub use ledger::*;
pub use marketplace::*;
pub use migration::*;
pub use mva::*;
pub use ocr::*;
pub use opening::*;
pub use payments::*;
pub use period::*;
pub use platform::*;
pub use product::*;
pub use purring::*;
pub use rates::*;
pub use regnskap::*;
pub use reskontro::*;
pub use revisjon::*;
pub use saft::*;
pub use saft_import::*;
pub use salgsdokument::*;
pub use settings::*;
pub use tenancy::*;
pub use timesheet::*;
pub use utsendelse::*;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Migrations embedded from `crates/regnmed-db/migrations`. sqlx records a
/// checksum per applied migration and refuses to run if an already-applied
/// file changed on disk — treat the migrations directory as append-only.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}
