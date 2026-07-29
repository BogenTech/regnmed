//! Portalen som serveres, PWA-skallet og panikk-garantien.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/panikk.rs"]
mod panikk;
#[path = "grupper/portal.rs"]
mod portal;
#[path = "grupper/pwa.rs"]
mod pwa;
