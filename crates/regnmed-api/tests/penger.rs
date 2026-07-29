//! Penger inn og ut: bank, betaling, OCR, valuta og mva-terminer.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/bank.rs"]
mod bank;
#[path = "grupper/ocr.rs"]
mod ocr;
#[path = "grupper/payments.rs"]
mod payments;
#[path = "grupper/reskontro.rs"]
mod reskontro;
#[path = "grupper/terminordning.rs"]
mod terminordning;
#[path = "grupper/valuta.rs"]
mod valuta;
