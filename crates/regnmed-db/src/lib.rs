//! PostgreSQL persistence for the regnmed ledger.
//!
//! Queries use sqlx's runtime API for now so the workspace builds without a
//! live database. Once the dev database is a fixture of CI, migrate hot
//! paths to `sqlx::query!` + `cargo sqlx prepare` for compile-time checking.

pub mod anchor;
pub mod attachment;
pub mod bank;
pub mod engagement;
pub mod invoice;
pub mod ledger;
pub mod marketplace;
pub mod mva;
pub mod ocr;
pub mod period;
pub mod regnskap;
pub mod reskontro;
pub mod revisjon;
pub mod saft;
pub mod saft_import;
pub mod tenancy;

pub use anchor::*;
pub use attachment::*;
pub use bank::*;
pub use engagement::*;
pub use invoice::*;
pub use ledger::*;
pub use marketplace::*;
pub use mva::*;
pub use ocr::*;
pub use period::*;
pub use regnskap::*;
pub use reskontro::*;
pub use revisjon::*;
pub use saft::*;
pub use saft_import::*;
pub use tenancy::*;

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
