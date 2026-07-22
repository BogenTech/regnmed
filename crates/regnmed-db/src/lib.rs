//! PostgreSQL persistence for the regnmed ledger.
//!
//! Queries use sqlx's runtime API for now so the workspace builds without a
//! live database. Once the dev database is a fixture of CI, migrate hot
//! paths to `sqlx::query!` + `cargo sqlx prepare` for compile-time checking.

pub mod bank;
pub mod ledger;
pub mod mva;
pub mod ocr;
pub mod reskontro;
pub mod saft;
pub mod tenancy;

pub use bank::*;
pub use ledger::*;
pub use mva::*;
pub use ocr::*;
pub use reskontro::*;
pub use saft::*;
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
