use chrono::NaiveDate;
use uuid::Uuid;

use crate::{LedgerError, Ore};

/// One line of a voucher. Positive amounts are debits, negative are credits.
#[derive(Debug, Clone)]
pub struct EntryDraft {
    /// Four-digit NS 4102 account number, e.g. "1920".
    pub account_number: String,
    pub amount: Ore,
    /// SAF-T standard VAT code, e.g. "3" for utgående mva alminnelig sats.
    pub vat_code: Option<String>,
    pub description: Option<String>,
}

/// A voucher (bilag) as submitted for posting, before it is assigned a
/// voucher number and chained into the ledger.
#[derive(Debug, Clone)]
pub struct VoucherDraft {
    pub journal_code: String,
    pub voucher_date: NaiveDate,
    pub description: String,
    /// The voucher being reversed, when this posting is a correction.
    /// The ledger never edits history — a mistake is corrected by posting
    /// a reversing voucher plus a new correct one.
    pub reverses: Option<Uuid>,
    pub entries: Vec<EntryDraft>,
}

impl VoucherDraft {
    /// Double-entry invariants: at least two lines, no zero-amount lines,
    /// and all lines must sum to exactly zero.
    pub fn validate(&self) -> Result<(), LedgerError> {
        if self.entries.len() < 2 {
            return Err(LedgerError::TooFewEntries(self.entries.len()));
        }
        let mut sum = Ore::ZERO;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.amount.is_zero() {
                return Err(LedgerError::ZeroAmount(i + 1));
            }
            if entry.vat_code.as_deref() == Some("") {
                return Err(LedgerError::EmptyVatCode(i + 1));
            }
            sum = sum
                .checked_add(entry.amount)
                .ok_or(LedgerError::AmountOverflow)?;
        }
        if !sum.is_zero() {
            return Err(LedgerError::Unbalanced(sum.0));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(account: &str, amount: i64) -> EntryDraft {
        EntryDraft {
            account_number: account.into(),
            amount: Ore(amount),
            vat_code: None,
            description: None,
        }
    }

    fn draft(entries: Vec<EntryDraft>) -> VoucherDraft {
        VoucherDraft {
            journal_code: "GL".into(),
            voucher_date: NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            description: "test".into(),
            reverses: None,
            entries,
        }
    }

    #[test]
    fn balanced_voucher_validates() {
        let v = draft(vec![entry("1920", 12_500_00), entry("3000", -12_500_00)]);
        assert_eq!(v.validate(), Ok(()));
    }

    #[test]
    fn unbalanced_voucher_is_rejected() {
        let v = draft(vec![entry("1920", 100), entry("3000", -99)]);
        assert_eq!(v.validate(), Err(LedgerError::Unbalanced(1)));
    }

    #[test]
    fn single_line_is_rejected() {
        let v = draft(vec![entry("1920", 0)]);
        assert_eq!(v.validate(), Err(LedgerError::TooFewEntries(1)));
    }

    #[test]
    fn zero_amount_line_is_rejected() {
        let v = draft(vec![entry("1920", 100), entry("3000", 0)]);
        assert_eq!(v.validate(), Err(LedgerError::ZeroAmount(2)));
    }
}
