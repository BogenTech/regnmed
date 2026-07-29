//! The portal as served, the PWA shell and the panic guarantee.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/panikk.rs"]
mod panikk;
#[path = "grupper/portal.rs"]
mod portal;
#[path = "grupper/pwa.rs"]
mod pwa;
