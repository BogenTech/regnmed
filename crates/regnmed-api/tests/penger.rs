//! Money in and out: bank, payment, OCR, currency and mva terminer.
//!
//! One binary for several test files: each tests/*.rs links separately,
//! and 33 of them held a single test. nextest runs every test in its own
//! process regardless, so the grouping costs no parallelism.

mod common;

#[path = "grupper/bank.rs"]
mod bank;
#[path = "grupper/bank_feed.rs"]
mod bank_feed;
#[path = "grupper/contractor_payments.rs"]
mod contractor_payments;
#[path = "grupper/valuta.rs"]
mod currency;
#[path = "grupper/ocr.rs"]
mod ocr;
#[path = "grupper/payments.rs"]
mod payments;
#[path = "grupper/terminordning.rs"]
mod period_scheme;
#[path = "grupper/reskontro.rs"]
mod subledger;
#[path = "grupper/vat_filing.rs"]
mod vat_filing;
