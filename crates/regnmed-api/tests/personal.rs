//! Folk og eiendeler: timer, utlegg, anlegg og aksjonærer.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/aksjonaer.rs"]
mod aksjonaer;
#[path = "grupper/assets.rs"]
mod assets;
#[path = "grupper/expenses.rs"]
mod expenses;
#[path = "grupper/timesheet.rs"]
mod timesheet;
