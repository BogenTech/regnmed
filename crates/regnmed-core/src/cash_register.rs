//! Kassaoppgjør (#89): the daily settlement from a kassasystem, as
//! bookkeeping — bokføringsforskriften §5-3 og §5-4.
//!
//! **regnmed is not a kassasystem.** Kassasystemlova puts the register
//! itself, with its produkterklæring, on its supplier. What belongs here
//! is the other end: turning the day's Z-report into one voucher with
//! the mva split, and doing it so the numbers can be checked against the
//! report afterwards.
//!
//! Pure and deterministic: the caller supplies the rate that applied on
//! the day (looked up in the dated table, like everywhere else), and
//! this module only splits and arranges.

use crate::Ore;
use crate::vat::split_gross;
use crate::voucher::{EntryDraft, VoucherDraft};

/// One line of the day's sales, as the register reports it: gross, on an
/// income account, at one VAT rate.
#[derive(Debug, Clone)]
pub struct SalesLine {
    pub account: String,
    pub vat_code: Option<String>,
    /// The rate that applied on the settlement date, basis points.
    pub rate_bp: i64,
    pub gross_ore: i64,
}

/// What the day was paid with, as the register reports it: cash, bank,
/// or a clearing account for cards and Vipps awaiting settlement.
#[derive(Debug, Clone)]
pub struct PaymentLine {
    pub account: String,
    pub amount_ore: i64,
}

#[derive(Debug, Clone)]
pub struct DailySettlement {
    pub date: chrono::NaiveDate,
    /// The Z-report number. Goes in the voucher text so the bilag can be
    /// tied back to the register's own numbered report — that link is
    /// the point of §5-4, and a settlement without it documents nothing.
    pub z_number: String,
    pub sales: Vec<SalesLine>,
    pub payments: Vec<PaymentLine>,
    pub vat_account: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CashRegisterError {
    NoSales,
    EmptyZNumber,
    /// The register says it sold one amount and took another. This is
    /// refused rather than balanced against a difference account: a
    /// mismatch INSIDE the Z-report is a broken report, not a till
    /// discrepancy, and papering over it would hide the one number the
    /// settlement exists to reconcile.
    Unbalanced {
        sales_ore: i64,
        payments_ore: i64,
    },
}

impl std::fmt::Display for CashRegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CashRegisterError::NoSales => write!(f, "dagsoppgjøret har ingen salgslinjer"),
            CashRegisterError::EmptyZNumber => write!(
                f,
                "dagsoppgjøret må ha Z-nummer fra kassasystemet (bokføringsforskriften §5-4)"
            ),
            CashRegisterError::Unbalanced {
                sales_ore,
                payments_ore,
            } => write!(
                f,
                "kassaoppgjøret går ikke opp: salg {sales_ore} øre mot betalingsmidler \
                 {payments_ore} øre. Differansen mellom TALT kasse og registrert salg \
                 føres som eget kassadifferansebilag — den skjules aldri her"
            ),
        }
    }
}

/// The day's settlement as one voucher: payment means debited, income
/// credited net per rate, mva credited as the sum of the parts.
///
/// The income lines keep their `vat_code`, so the mva-spesifikasjon sees
/// the day's sales the same way it sees an invoice's. The mva line
/// itself is uncoded, exactly as the invoice engine posts it — a code
/// there would count the same grunnlag twice.
pub fn build_settlement(settlement: &DailySettlement) -> Result<VoucherDraft, CashRegisterError> {
    if settlement.sales.is_empty() {
        return Err(CashRegisterError::NoSales);
    }
    if settlement.z_number.trim().is_empty() {
        return Err(CashRegisterError::EmptyZNumber);
    }
    let sales_ore: i64 = settlement.sales.iter().map(|l| l.gross_ore).sum();
    let payments_ore: i64 = settlement.payments.iter().map(|b| b.amount_ore).sum();
    if sales_ore != payments_ore {
        return Err(CashRegisterError::Unbalanced {
            sales_ore,
            payments_ore,
        });
    }

    let mut entries = Vec::with_capacity(settlement.sales.len() + settlement.payments.len() + 1);
    for b in &settlement.payments {
        entries.push(EntryDraft {
            account_number: b.account.clone(),
            amount: Ore(b.amount_ore),
            vat_code: None,
            description: None,
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    let mut vat_sum = 0;
    for l in &settlement.sales {
        let (net, vat) = split_gross(l.gross_ore, l.rate_bp);
        vat_sum += vat;
        entries.push(EntryDraft {
            account_number: l.account.clone(),
            amount: Ore(-net),
            vat_code: l.vat_code.clone(),
            description: None,
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if vat_sum != 0 {
        entries.push(EntryDraft {
            account_number: settlement.vat_account.clone(),
            amount: Ore(-vat_sum),
            vat_code: None,
            description: None,
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    Ok(VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: settlement.date,
        description: format!("Kassaoppgjør Z-{}", settlement.z_number.trim()),
        reverses: None,
        entries,
    })
}

/// The kassadifferanse as its OWN voucher: counted cash against what the
/// register says should be in the till.
///
/// Its own bilag rather than a line on the settlement, because a
/// discrepancy is a finding about the day, not a rounding of it. A
/// shortfall credits the cash account and charges the difference
/// account; a surplus does the reverse. `None` when they agree — we do
/// not post a zero voucher to prove nothing happened.
pub fn build_discrepancy(
    date: chrono::NaiveDate,
    z_number: &str,
    cash_account: &str,
    discrepancy_account: &str,
    registered_cash_ore: i64,
    counted_cash_ore: i64,
) -> Option<VoucherDraft> {
    let discrepancy = counted_cash_ore - registered_cash_ore;
    if discrepancy == 0 {
        return None;
    }
    let line = |account: &str, amount: i64| EntryDraft {
        account_number: account.to_string(),
        amount: Ore(amount),
        vat_code: None,
        description: None,
        party_no: None,
        department: None,
        project: None,
        currency: None,
    };
    Some(VoucherDraft {
        journal_code: "GL".into(),
        voucher_date: date,
        description: format!(
            "Kassadifferanse Z-{} ({} øre)",
            z_number.trim(),
            discrepancy
        ),
        reverses: None,
        entries: vec![
            line(cash_account, discrepancy),
            line(discrepancy_account, -discrepancy),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> DailySettlement {
        DailySettlement {
            date: chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            z_number: "0042".into(),
            sales: vec![
                SalesLine {
                    account: "3000".into(),
                    vat_code: Some("3".into()),
                    rate_bp: 2500,
                    gross_ore: 12_500_00,
                },
                SalesLine {
                    account: "3010".into(),
                    vat_code: Some("31".into()),
                    rate_bp: 1500,
                    gross_ore: 2_300_00,
                },
            ],
            payments: vec![
                PaymentLine {
                    account: "1900".into(),
                    amount_ore: 4_800_00,
                },
                PaymentLine {
                    account: "1571".into(),
                    amount_ore: 10_000_00,
                },
            ],
            vat_account: "2700".into(),
        }
    }

    #[test]
    fn the_settlement_balances_and_splits_vat_per_rate() {
        let v = build_settlement(&day()).unwrap();
        assert_eq!(
            v.entries.iter().map(|e| e.amount.0).sum::<i64>(),
            0,
            "bilaget må gå i null"
        );
        assert!(v.description.contains("Z-0042"), "{}", v.description);

        // 12 500 incl. 25 % = 10 000 + 2 500; 2 300 incl. 15 % = 2 000 + 300.
        let account = |number: &str| {
            v.entries
                .iter()
                .find(|e| e.account_number == number)
                .unwrap_or_else(|| panic!("mangler {number}"))
                .amount
                .0
        };
        assert_eq!(account("3000"), -10_000_00);
        assert_eq!(account("3010"), -2_000_00);
        assert_eq!(
            account("2700"),
            -2_800_00,
            "summen av delene, ikke av totalen"
        );
        assert_eq!(account("1900"), 4_800_00);
        assert_eq!(account("1571"), 10_000_00);

        // Income keeps its code so the mva-spesifikasjon sees the day's
        // sales; the mva line is uncoded like the invoice engine's.
        assert_eq!(
            v.entries
                .iter()
                .find(|e| e.account_number == "3000")
                .unwrap()
                .vat_code
                .as_deref(),
            Some("3")
        );
        assert!(
            v.entries
                .iter()
                .find(|e| e.account_number == "2700")
                .unwrap()
                .vat_code
                .is_none()
        );
    }

    /// A Z-report that does not add up is a broken report. Balancing it
    /// against the difference account would hide the very number the
    /// settlement exists to reconcile.
    #[test]
    fn a_z_report_that_does_not_add_up_is_refused_loudly() {
        let mut d = day();
        d.payments[0].amount_ore = 4_000_00;
        let error = build_settlement(&d).unwrap_err();
        assert!(matches!(error, CashRegisterError::Unbalanced { .. }));
        assert!(
            error.to_string().contains("kassadifferansebilag"),
            "{error}"
        );

        d.z_number = "  ".into();
        assert_eq!(
            build_settlement(&d).unwrap_err(),
            CashRegisterError::EmptyZNumber
        );
    }

    #[test]
    fn the_kassadifferanse_is_its_own_voucher_and_only_when_there_is_one() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap();
        assert!(
            build_discrepancy(date, "0042", "1900", "7830", 4_800_00, 4_800_00).is_none(),
            "ingen differanse, intet bilag"
        );

        // 50 kr missing from the till.
        let v = build_discrepancy(date, "0042", "1900", "7830", 4_800_00, 4_750_00).unwrap();
        assert_eq!(v.entries.iter().map(|e| e.amount.0).sum::<i64>(), 0);
        assert_eq!(v.entries[0].account_number, "1900");
        assert_eq!(v.entries[0].amount.0, -50_00, "kassen skrives ned");
        assert_eq!(v.entries[1].amount.0, 50_00, "differansen kostnadsføres");
        assert!(
            v.description.contains("Kassadifferanse"),
            "{}",
            v.description
        );

        // A surplus goes the other way — it is not "no discrepancy".
        let over = build_discrepancy(date, "0042", "1900", "7830", 4_800_00, 4_820_00).unwrap();
        assert_eq!(over.entries[0].amount.0, 20_00);
    }
}
