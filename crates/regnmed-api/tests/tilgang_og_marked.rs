//! Who may do what: access, members, oppdrag, marketplace, abonnement.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/abonnement.rs"]
mod abonnement;
#[path = "grupper/byramedlemmer.rs"]
mod byramedlemmer;
#[path = "grupper/engagement.rs"]
mod engagement;
#[path = "grupper/integrasjon.rs"]
mod integrasjon;
#[path = "grupper/marketplace.rs"]
mod marketplace;
#[path = "grupper/matrise.rs"]
mod matrise;
#[path = "grupper/me_endpoint.rs"]
mod me_endpoint;
#[path = "grupper/medlemmer.rs"]
mod medlemmer;
#[path = "grupper/plattform.rs"]
mod plattform;
#[path = "grupper/tilgang.rs"]
mod tilgang;
