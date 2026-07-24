//! PostgreSQL persistence for the regnmed ledger.
//!
//! Queries use sqlx's runtime API for now so the workspace builds without a
//! live database. Once the dev database is a fixture of CI, migrate hot
//! paths to `sqlx::query!` + `cargo sqlx prepare` for compile-time checking.

pub mod anchor;
pub mod attachment;
pub mod bank;
pub mod dimension;
pub mod engagement;
pub mod innboks;
pub mod invoice;
pub mod invoice_template;
pub mod ledger;
pub mod marketplace;
pub mod mva;
pub mod ocr;
pub mod opening;
pub mod period;
pub mod purring;
pub mod regnskap;
pub mod reskontro;
pub mod revisjon;
pub mod saft;
pub mod saft_import;
pub mod salgsdokument;
pub mod sats;
pub mod settings;
pub mod tenancy;
pub mod timesheet;
pub mod utsendelse;

pub use anchor::*;
pub use attachment::*;
pub use bank::*;
pub use dimension::*;
pub use engagement::*;
pub use innboks::*;
pub use invoice::*;
pub use invoice_template::*;
pub use ledger::*;
pub use marketplace::*;
pub use mva::*;
pub use ocr::*;
pub use opening::*;
pub use period::*;
pub use purring::*;
pub use regnskap::*;
pub use reskontro::*;
pub use revisjon::*;
pub use saft::*;
pub use saft_import::*;
pub use salgsdokument::*;
pub use sats::*;
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
