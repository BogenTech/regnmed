//! Utgående faktura: pure computation — line amounts, VAT, KID, and the
//! ledger voucher an invoice posts.
//!
//! All in integer øre with half-away-from-zero rounding. A kreditnota is
//! the same computation over negated quantities, so signs flow through
//! naturally.

use chrono::NaiveDate;

use crate::kid::check_digit_mod10;
use crate::mva::vat_of_base;
use crate::voucher::{EntryDraft, VoucherDraft};
use crate::{LedgerError, Ore};

#[derive(Debug, Clone)]
pub struct InvoiceLineInput {
    pub description: String,
    /// Revenue account the line posts to (e.g. 3000).
    pub account_number: String,
    /// Thousandths: 1 stk = 1000. Negative on kreditnotaer.
    pub quantity_milli: i64,
    pub unit_price_ore: i64,
    pub vat_code: Option<String>,
    /// Rate valid on the invoice date, resolved by the caller from the
    /// dated rate table. 0 for zero-rate/no-VAT codes.
    pub rate_bp: i64,
    /// Dimensions for the revenue line (docs/dimensjoner.md).
    pub avdeling: Option<String>,
    pub prosjekt: Option<String>,
}

#[derive(Debug)]
pub struct ComputedLine {
    pub net_ore: i64,
    pub vat_ore: i64,
}

#[derive(Debug)]
pub struct ComputedInvoice {
    pub lines: Vec<ComputedLine>,
    pub net_ore: i64,
    pub vat_ore: i64,
    pub gross_ore: i64,
}

/// `quantity × unit price`, rounded half away from zero.
pub fn line_net_ore(quantity_milli: i64, unit_price_ore: i64) -> i64 {
    let product = i128::from(quantity_milli) * i128::from(unit_price_ore);
    let rounded = (product.abs() + 500) / 1_000;
    i64::try_from(rounded).expect("line amount fits in i64") * product.signum() as i64
}

pub fn compute(lines: &[InvoiceLineInput]) -> ComputedInvoice {
    let computed: Vec<ComputedLine> = lines
        .iter()
        .map(|line| {
            let net_ore = line_net_ore(line.quantity_milli, line.unit_price_ore);
            ComputedLine {
                net_ore,
                vat_ore: vat_of_base(net_ore, line.rate_bp),
            }
        })
        .collect();
    let net_ore = computed.iter().map(|l| l.net_ore).sum::<i64>();
    let vat_ore = computed.iter().map(|l| l.vat_ore).sum::<i64>();
    ComputedInvoice {
        lines: computed,
        net_ore,
        vat_ore,
        gross_ore: net_ore + vat_ore,
    }
}

/// KID for an invoice: the number zero-padded to 8 digits plus a MOD10
/// check digit — 9 digits, unique per company since invoice numbers are.
pub fn invoice_kid(invoice_no: i64) -> String {
    let base = format!("{invoice_no:08}");
    let check = check_digit_mod10(&base).expect("base is digits");
    format!("{base}{check}")
}

/// The ledger posting for an invoice: debit the receivable (with the
/// customer), credit each revenue line (with its VAT code), credit the
/// summed VAT. Kreditnotaer negate every amount, flipping debit/credit.
#[allow(clippy::too_many_arguments)]
pub fn build_voucher(
    journal_code: &str,
    invoice_date: NaiveDate,
    invoice_no: i64,
    credit_note: bool,
    party_no: &str,
    receivable_account: &str,
    vat_account: &str,
    lines: &[InvoiceLineInput],
    computed: &ComputedInvoice,
) -> Result<VoucherDraft, LedgerError> {
    let label = if credit_note { "Kreditnota" } else { "Faktura" };
    let mut entries = vec![EntryDraft {
        account_number: receivable_account.to_string(),
        amount: Ore(computed.gross_ore),
        vat_code: None,
        description: None,
        party_no: Some(party_no.to_string()),
        avdeling: None,
        prosjekt: None,
    }];
    for (line, amounts) in lines.iter().zip(&computed.lines) {
        entries.push(EntryDraft {
            account_number: line.account_number.clone(),
            amount: Ore(-amounts.net_ore),
            vat_code: line.vat_code.clone(),
            description: Some(line.description.clone()),
            party_no: None,
            avdeling: line.avdeling.clone(),
            prosjekt: line.prosjekt.clone(),
        });
    }
    if computed.vat_ore != 0 {
        entries.push(EntryDraft {
            account_number: vat_account.to_string(),
            amount: Ore(-computed.vat_ore),
            vat_code: None,
            description: None,
            party_no: None,
            avdeling: None,
            prosjekt: None,
        });
    }
    let draft = VoucherDraft {
        journal_code: journal_code.to_string(),
        voucher_date: invoice_date,
        description: format!("{label} {invoice_no}"),
        reverses: None,
        entries,
    };
    draft.validate()?;
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<InvoiceLineInput> {
        vec![
            InvoiceLineInput {
                description: "Konsulentbistand".into(),
                account_number: "3000".into(),
                quantity_milli: 2_500, // 2,5 timer
                unit_price_ore: 4_000_00,
                vat_code: Some("3".into()),
                rate_bp: 2500,
                avdeling: Some("100".into()),
                prosjekt: Some("P42".into()),
            },
            InvoiceLineInput {
                description: "Bøker".into(),
                account_number: "3100".into(),
                quantity_milli: 1_000,
                unit_price_ore: 500_00,
                vat_code: Some("5".into()),
                rate_bp: 0,
                avdeling: None,
                prosjekt: None,
            },
        ]
    }

    #[test]
    fn computes_lines_vat_and_totals() {
        let computed = compute(&lines());
        assert_eq!(computed.lines[0].net_ore, 10_000_00, "2,5 × 4000 kr");
        assert_eq!(computed.lines[0].vat_ore, 2_500_00);
        assert_eq!(computed.lines[1].net_ore, 500_00);
        assert_eq!(computed.lines[1].vat_ore, 0, "zero-rate code");
        assert_eq!(computed.gross_ore, 13_000_00);
    }

    #[test]
    fn quantity_rounding_is_half_away_from_zero() {
        assert_eq!(line_net_ore(1_000, 100), 100);
        assert_eq!(line_net_ore(333, 100), 33, "0,333 × 1 kr → 33 øre");
        assert_eq!(line_net_ore(335, 100), 34, "33,5 øre rounds up");
        assert_eq!(line_net_ore(-335, 100), -34);
    }

    #[test]
    fn kid_is_mod10_valid_and_deterministic() {
        let kid = invoice_kid(1);
        assert_eq!(kid.len(), 9);
        assert!(crate::kid::is_valid_mod10(&kid));
        assert_eq!(kid, invoice_kid(1));
        assert_ne!(invoice_kid(1), invoice_kid(2));
    }

    #[test]
    fn voucher_balances_and_carries_the_party() {
        let computed = compute(&lines());
        let draft = build_voucher(
            "GL",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            1,
            false,
            "10000",
            "1500",
            "2700",
            &lines(),
            &computed,
        )
        .unwrap();
        assert_eq!(draft.description, "Faktura 1");
        assert_eq!(draft.entries.len(), 4);
        assert_eq!(draft.entries[0].amount, Ore(13_000_00));
        assert_eq!(draft.entries[0].party_no.as_deref(), Some("10000"));
        let sum: i64 = draft.entries.iter().map(|e| e.amount.0).sum();
        assert_eq!(sum, 0, "double entry holds");
    }

    #[test]
    fn credit_note_flips_all_signs() {
        let negated: Vec<InvoiceLineInput> = lines()
            .into_iter()
            .map(|mut line| {
                line.quantity_milli = -line.quantity_milli;
                line
            })
            .collect();
        let computed = compute(&negated);
        assert_eq!(computed.gross_ore, -13_000_00);
        let draft = build_voucher(
            "GL",
            NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            2,
            true,
            "10000",
            "1500",
            "2700",
            &negated,
            &computed,
        )
        .unwrap();
        assert_eq!(draft.description, "Kreditnota 2");
        assert_eq!(
            draft.entries[0].amount,
            Ore(-13_000_00),
            "credit the customer"
        );
    }
}
