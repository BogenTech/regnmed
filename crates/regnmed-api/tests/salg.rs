//! Salgssiden: faktura, tilbud/ordre, purring, EHF, produkter, utsendelse.
//!
//! Én binær for flere testfiler: hver tests/*.rs lenkes for seg, og
//! 33 av dem hadde én eneste test. nextest kjører hver test i sin
//! egen prosess uansett, så grupperingen koster ingen parallellitet.

mod common;

#[path = "grupper/ehf.rs"]
mod ehf;
#[path = "grupper/faktura_pdf.rs"]
mod faktura_pdf;
#[path = "grupper/invoice.rs"]
mod invoice;
#[path = "grupper/invoice_template.rs"]
mod invoice_template;
#[path = "grupper/products.rs"]
mod products;
#[path = "grupper/purring.rs"]
mod purring;
#[path = "grupper/salgsdokument.rs"]
mod salgsdokument;
#[path = "grupper/utsendelse.rs"]
mod utsendelse;
