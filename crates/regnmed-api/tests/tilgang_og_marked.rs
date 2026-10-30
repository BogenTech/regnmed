//! Who may do what: access, members, oppdrag, marketplace, subscription.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/access.rs"]
mod access;
#[path = "grupper/agent.rs"]
mod agent;
#[path = "grupper/byramedlemmer.rs"]
mod byramedlemmer;
#[path = "grupper/engagement.rs"]
mod engagement;
#[path = "grupper/integrasjon.rs"]
mod integrasjon;
#[path = "grupper/marketplace.rs"]
mod marketplace;
#[path = "grupper/matrix.rs"]
mod matrix;
#[path = "grupper/me_endpoint.rs"]
mod me_endpoint;
#[path = "grupper/medlemmer.rs"]
mod members;
#[path = "grupper/plattform.rs"]
mod plattform;
#[path = "grupper/subscription.rs"]
mod subscription;
