//! The sales side: faktura, tilbud/ordre, purring, EHF, products, dispatch.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/utsendelse.rs"]
mod dispatch;
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
#[path = "grupper/reminders.rs"]
mod reminders;
#[path = "grupper/salgsdokument.rs"]
mod salgsdokument;
