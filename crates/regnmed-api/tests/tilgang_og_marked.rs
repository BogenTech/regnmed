//! Hvem får gjøre hva: tilgang, medlemmer, oppdrag, marked, abonnement.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/abonnement.rs"]
mod abonnement;
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
#[path = "grupper/tilgang.rs"]
mod tilgang;
