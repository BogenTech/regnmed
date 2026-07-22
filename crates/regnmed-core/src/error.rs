use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("a voucher needs at least two entry lines, got {0}")]
    TooFewEntries(usize),

    #[error("entry line {0} has a zero amount")]
    ZeroAmount(usize),

    #[error("entry line {0} has an empty VAT code — use None for no code")]
    EmptyVatCode(usize),

    #[error("entry line {0} has an empty party number — use None for no party")]
    EmptyPartyNo(usize),

    #[error("voucher does not balance: entries sum to {0} øre, expected 0")]
    Unbalanced(i64),

    #[error("amount overflow while summing entries")]
    AmountOverflow,
}
