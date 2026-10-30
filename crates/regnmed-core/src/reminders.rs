//! Payment follow-up, pure side: late-payment interest, dunning rules
//! and deterministic document rendering (docs/purring.md).
//!
//! All regelverk is pulled as data from the satsregister
//! ([`crate::rates`]) — this module knows *how* the rules are applied
//! (forsinkelsesrenteloven, inkassoforskriften, inkassoloven §9), never
//! which numbers are in force. Interest is computed in integer øre, and
//! the document renders deterministically so an archived claim can be
//! reproduced byte for byte.

use std::fmt;

use chrono::NaiveDate;

use crate::rates::{RatePeriod, value_on};
use crate::voucher::{EntryDraft, VoucherDraft};
use crate::{LedgerError, Ore};

/// The dunning ladder. The steps are one-way: a claim can never go back to a
/// milder step, and inkasso itself is outside regnmed (it requires a
/// licence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Reminder,
    Demand,
    CollectionNotice,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Reminder => "paminnelse",
            Step::Demand => "purring",
            Step::CollectionNotice => "inkassovarsel",
        }
    }

    pub fn parse(s: &str) -> Option<Step> {
        match s {
            "paminnelse" => Some(Step::Reminder),
            "purring" => Some(Step::Demand),
            "inkassovarsel" => Some(Step::CollectionNotice),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Reminder => "Betalingspåminnelse",
            Step::Demand => "Purring",
            Step::CollectionNotice => "Inkassovarsel",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReminderError {
    /// The send date is not after the due date — the claim is not yet due.
    NotDue {
        due_date: NaiveDate,
    },
    /// No late-payment interest rate covers this day (before the earliest
    /// verified period) — we never guess a rate.
    MissingInterestRate {
        date: NaiveDate,
    },
    /// The dunning ladder is one-way.
    StepBackwards {
        last: Step,
        attempted: Step,
    },
    /// A payment reminder carries no fee; with one it is a demand.
    FeeOnReminder,
    /// Inkassoforskriften §1-2: no fee earlier than 14 days after the due date.
    FeeTooEarly {
        earliest: NaiveDate,
    },
    FeeAboveMax {
        max_ore: i64,
    },
    /// At most two fee-bearing steps per claim (inkassoforskriften §1-2).
    TooManyFees,
    NegativeAmount,
    DeadlineBeforeSendDate,
    /// Inkassoloven §9: a payment deadline of at least 14 days.
    GraceTooShort {
        at_least: NaiveDate,
    },
    InterestOverflow,
}

impl fmt::Display for ReminderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReminderError::NotDue { due_date } => {
                write!(f, "fakturaen forfaller {due_date} og er ikke forfalt")
            }
            ReminderError::MissingInterestRate { date } => {
                write!(f, "ingen forsinkelsesrentesats dekker {date}")
            }
            ReminderError::StepBackwards { last, attempted } => write!(
                f,
                "purretrappen er enveis: {} er allerede sendt, kan ikke sende {}",
                last.title(),
                attempted.title()
            ),
            ReminderError::FeeOnReminder => {
                write!(
                    f,
                    "en betalingspåminnelse er gebyrfri — bruk steget purring"
                )
            }
            ReminderError::FeeTooEarly { earliest } => write!(
                f,
                "purregebyr tidligst 14 dager etter forfall, dvs. {earliest} (inkassoforskriften §1-2)"
            ),
            ReminderError::FeeAboveMax { max_ore } => {
                write!(f, "gebyret overstiger maksimalsatsen {} kr", Ore(*max_ore))
            }
            ReminderError::TooManyFees => write!(
                f,
                "maksimalt to gebyrbelagte purringer/varsler per krav (inkassoforskriften §1-2)"
            ),
            ReminderError::NegativeAmount => write!(f, "beløp kan ikke være negative"),
            ReminderError::DeadlineBeforeSendDate => {
                write!(f, "betalingsfristen må være etter sendedatoen")
            }
            ReminderError::GraceTooShort { at_least } => write!(
                f,
                "et inkassovarsel krever minst 14 dagers betalingsfrist, tidligst {at_least} (inkassoloven §9)"
            ),
            ReminderError::InterestOverflow => write!(f, "rentebeløpet gikk over i64"),
        }
    }
}

impl std::error::Error for ReminderError {}

/// One rate period of the interest run, as shown in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestPeriod {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub rate_bp: i64,
    pub days: i64,
    pub interest_ore: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestCalculation {
    pub periods: Vec<InterestPeriod>,
    pub sum_ore: i64,
}

/// `amount × rate × days / (10000 × 365)`, rounded half away from zero.
fn interest_for_segment(amount_ore: i64, rate_bp: i64, days: i64) -> Option<i64> {
    let numerator = i128::from(amount_ore) * i128::from(rate_bp) * i128::from(days);
    let denominator: i128 = 10_000 * 365;
    let rounded = (2 * numerator.abs() + denominator) / (2 * denominator);
    i64::try_from(rounded * numerator.signum()).ok()
}

/// Late-payment interest under forsinkelsesrenteloven §2: interest runs
/// from the day after the due date through `to`, at whichever rate was in force on
/// each single day (actual days / 365). Every rate period is rounded on
/// its own, so the spesifikasjon in the document sums exactly to the
/// total.
pub fn late_payment_interest(
    amount_ore: i64,
    due_date: NaiveDate,
    to: NaiveDate,
    rates: &[RatePeriod],
) -> Result<InterestCalculation, ReminderError> {
    if amount_ore < 0 {
        return Err(ReminderError::NegativeAmount);
    }
    let mut periods = Vec::new();
    let mut sum: i64 = 0;
    let mut day = due_date.succ_opt().expect("date in range");
    while day <= to {
        let rate = value_on(rates, "forsinkelsesrente", day)
            .ok_or(ReminderError::MissingInterestRate { date: day })?;
        let next_change = rates
            .iter()
            .filter(|s| s.domain == "forsinkelsesrente" && s.valid_from > day)
            .map(|s| s.valid_from)
            .min();
        let end = match next_change {
            Some(change) if change <= to => change.pred_opt().expect("date in range"),
            _ => to,
        };
        let days = (end - day).num_days() + 1;
        let interest =
            interest_for_segment(amount_ore, rate, days).ok_or(ReminderError::InterestOverflow)?;
        sum = sum
            .checked_add(interest)
            .ok_or(ReminderError::InterestOverflow)?;
        periods.push(InterestPeriod {
            from: day,
            to: end,
            rate_bp: rate,
            days,
            interest_ore: interest,
        });
        day = end.succ_opt().expect("date in range");
    }
    Ok(InterestCalculation {
        periods,
        sum_ore: sum,
    })
}

/// A previously sent step, in the shape the rule checks need.
#[derive(Debug, Clone, Copy)]
pub struct PreviousStep {
    pub step: Step,
    pub fee_ore: i64,
}

/// The machine's next move on an overdue invoice (#75) — the cadence
/// the automatic follow-up walks. A SUGGESTION shape like everything
/// else in this module: `valider_steg` still judges the actual step.
#[derive(Debug, PartialEq, Eq)]
pub struct NextStep {
    pub step: Step,
    /// Charge the fee the rate register allows on the send date.
    pub with_fee: bool,
    /// Add late-payment interest to the claim.
    pub with_interest: bool,
    /// Payment deadline, days from the send date.
    pub grace_days: i64,
}

/// Days past the due date before the first (free) reminder — a bank
/// transfer needs a few days to land before we nag.
pub const REMINDER_AFTER_DAYS: i64 = 3;
/// Days between steps — matches the deadline each step gives the debtor.
pub const STEP_INTERVAL_DAYS: i64 = 14;

/// What the automatic follow-up should do today, or None when nothing
/// is due: the ladder is reminder (free) → demand (fee) → collection
/// notice (interest, 14-day deadline as the law requires) → stop.
/// Debt collection itself needs a licence (docs/purring.md), so
/// after the inkassovarsel the machine's job is blocking, not nagging.
pub fn next_step(
    last: Option<(Step, NaiveDate)>,
    due_date: NaiveDate,
    today: NaiveDate,
) -> Option<NextStep> {
    match last {
        None if today >= due_date + chrono::Days::new(REMINDER_AFTER_DAYS as u64) => {
            Some(NextStep {
                step: Step::Reminder,
                with_fee: false,
                with_interest: false,
                grace_days: STEP_INTERVAL_DAYS,
            })
        }
        Some((Step::Reminder, sent))
            if today >= sent + chrono::Days::new(STEP_INTERVAL_DAYS as u64) =>
        {
            Some(NextStep {
                step: Step::Demand,
                with_fee: true,
                with_interest: false,
                grace_days: STEP_INTERVAL_DAYS,
            })
        }
        Some((Step::Demand, sent))
            if today >= sent + chrono::Days::new(STEP_INTERVAL_DAYS as u64) =>
        {
            Some(NextStep {
                step: Step::CollectionNotice,
                with_fee: false,
                with_interest: true,
                grace_days: STEP_INTERVAL_DAYS,
            })
        }
        _ => None,
    }
}

/// The rule check before a new step is recorded. `max_fee_ore` is the
/// rate valid on the send date (purregebyr_maks or, for
/// næringsdrivende debtors, standardkompensasjon) — the caller does the
/// lookup, the rules are applied here.
pub fn validate_step(
    step: Step,
    sent_date: NaiveDate,
    grace_date: NaiveDate,
    due_date: NaiveDate,
    fee_ore: i64,
    max_fee_ore: i64,
    history: &[PreviousStep],
) -> Result<(), ReminderError> {
    if fee_ore < 0 {
        return Err(ReminderError::NegativeAmount);
    }
    if sent_date <= due_date {
        return Err(ReminderError::NotDue { due_date });
    }
    if let Some(last) = history.iter().map(|s| s.step).max()
        && step < last
    {
        return Err(ReminderError::StepBackwards {
            last,
            attempted: step,
        });
    }
    if grace_date <= sent_date {
        return Err(ReminderError::DeadlineBeforeSendDate);
    }
    if step == Step::CollectionNotice {
        let at_least = sent_date + chrono::Days::new(14);
        if grace_date < at_least {
            return Err(ReminderError::GraceTooShort { at_least });
        }
    }
    if fee_ore > 0 {
        if step == Step::Reminder {
            return Err(ReminderError::FeeOnReminder);
        }
        let earliest = due_date + chrono::Days::new(14);
        if sent_date < earliest {
            return Err(ReminderError::FeeTooEarly { earliest });
        }
        if fee_ore > max_fee_ore {
            return Err(ReminderError::FeeAboveMax {
                max_ore: max_fee_ore,
            });
        }
        if history.iter().filter(|s| s.fee_ore > 0).count() >= 2 {
            return Err(ReminderError::TooManyFees);
        }
    }
    Ok(())
}

/// The voucher when a fee and/or interest is claimed: debit the reskontro
/// receivable (with the customer — the claim becomes an open item on the
/// same reskontro as the faktura), credit the revenue accounts. Called
/// only when the sum is positive; a step with neither fee nor interest
/// posts nothing.
#[allow(clippy::too_many_arguments)]
pub fn build_claim_voucher(
    journal_code: &str,
    sent_date: NaiveDate,
    step: Step,
    invoice_no: i64,
    party_no: &str,
    receivable_account: &str,
    fee_account: &str,
    fee_ore: i64,
    interest_account: &str,
    interest_ore: i64,
) -> Result<VoucherDraft, LedgerError> {
    let mut entries = vec![EntryDraft {
        account_number: receivable_account.to_string(),
        amount: Ore(fee_ore + interest_ore),
        vat_code: None,
        description: None,
        party_no: Some(party_no.to_string()),
        department: None,
        project: None,
        currency: None,
    }];
    if fee_ore > 0 {
        entries.push(EntryDraft {
            account_number: fee_account.to_string(),
            amount: Ore(-fee_ore),
            vat_code: None,
            description: Some("Purregebyr".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    if interest_ore > 0 {
        entries.push(EntryDraft {
            account_number: interest_account.to_string(),
            amount: Ore(-interest_ore),
            vat_code: None,
            description: Some("Forsinkelsesrente".into()),
            party_no: None,
            department: None,
            project: None,
            currency: None,
        });
    }
    let draft = VoucherDraft {
        journal_code: journal_code.to_string(),
        voucher_date: sent_date,
        description: format!("{} faktura {}", step.title(), invoice_no),
        reverses: None,
        entries,
    };
    draft.validate()?;
    Ok(draft)
}

/// Everything the document needs, gathered by the persistence layer. No
/// clock and
/// ingen oppslag her — samme input gir samme bytes for alltid.
#[derive(Debug, Clone)]
pub struct ReminderDocument {
    pub step: Step,
    pub company: String,
    pub orgnumber: String,
    pub customer_name: String,
    pub customer_no: String,
    pub invoice_no: i64,
    pub invoice_date: NaiveDate,
    pub due_date: NaiveDate,
    pub sent_date: NaiveDate,
    pub grace_date: NaiveDate,
    pub remaining_amount_ore: i64,
    /// The interest spesifikasjon when interest is claimed; empty otherwise.
    pub interest: InterestCalculation,
    pub fee_ore: i64,
    /// None when the seller has no KID/OCR agreement (#104).
    pub kid: Option<String>,
}

fn percent(bp: i64) -> String {
    format!("{},{:02} %", bp / 100, bp % 100)
}

/// Deterministic plain-text rendering of the claim, stored when the step
/// is recorded and reissuable forever (`?format=tekst`).
pub fn render_document(doc: &ReminderDocument) -> String {
    let mut out = String::with_capacity(1024);
    let mut line = |s: &str| {
        out.push_str(s);
        out.push('\n');
    };
    let title = doc.step.title().to_uppercase();
    line(&title);
    line(&"=".repeat(title.chars().count()));
    line("");
    line(&format!("Fra:    {} ({})", doc.company, doc.orgnumber));
    line(&format!(
        "Til:    {} (kunde {})",
        doc.customer_name, doc.customer_no
    ));
    line(&format!("Sendt:  {}", doc.sent_date));
    line("");
    line(&format!(
        "Faktura {} av {} forfalt {}.",
        doc.invoice_no, doc.invoice_date, doc.due_date
    ));
    line("");
    let mut total = doc.remaining_amount_ore;
    line(&format!(
        "Utestående beløp:                        {:>12} kr",
        Ore(doc.remaining_amount_ore).to_string()
    ));
    if doc.interest.sum_ore > 0 {
        line("Forsinkelsesrente (forsinkelsesrenteloven §2):");
        for p in &doc.interest.periods {
            line(&format!(
                "  {} – {}  {:>8}  {:>3} dager  {:>10} kr",
                p.from,
                p.to,
                percent(p.rate_bp),
                p.days,
                Ore(p.interest_ore).to_string()
            ));
        }
        total += doc.interest.sum_ore;
    }
    if doc.fee_ore > 0 {
        line(&format!(
            "Purregebyr (inkassoforskriften §1-2):    {:>12} kr",
            Ore(doc.fee_ore).to_string()
        ));
        total += doc.fee_ore;
    }
    line("");
    line(&format!(
        "Å betale:                                {:>12} kr",
        Ore(total).to_string()
    ));
    line(&format!("Betales innen:  {}", doc.grace_date));
    if let Some(kid) = &doc.kid {
        line(&format!("KID:            {kid}"));
    }
    if doc.step == Step::CollectionNotice {
        line("");
        line("Dette er et inkassovarsel etter inkassoloven §9. Betales ikke");
        line("kravet innen fristen, kan det uten ytterligere varsel sendes");
        line("til inkasso. Videre inndriving skjer hos foretak med");
        line("inkassobevilling — regnmed utfører ikke inkasso.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_ladder_walks_reminder_demand_collection_notice_then_stops() {
        let due_date = date(2026, 3, 1);
        // Too early: the transfer may be in flight.
        assert_eq!(next_step(None, due_date, date(2026, 3, 3)), None);
        // Day 3: the free reminder.
        let p = next_step(None, due_date, date(2026, 3, 4)).unwrap();
        assert_eq!(p.step, Step::Reminder);
        assert!(!p.with_fee && !p.with_interest);
        // 14 days after the reminder: a demand, now with a fee.
        let sent = date(2026, 3, 4);
        assert_eq!(
            next_step(Some((Step::Reminder, sent)), due_date, date(2026, 3, 17)),
            None,
            "not before the frist has run out"
        );
        let p = next_step(Some((Step::Reminder, sent)), due_date, date(2026, 3, 18)).unwrap();
        assert_eq!(p.step, Step::Demand);
        assert!(p.with_fee);
        // 14 more: collection notice with interest and the statutory deadline.
        let p = next_step(
            Some((Step::Demand, date(2026, 3, 18))),
            due_date,
            date(2026, 4, 1),
        )
        .unwrap();
        assert_eq!(p.step, Step::CollectionNotice);
        assert!(p.with_interest && !p.with_fee);
        assert_eq!(p.grace_days, 14);
        // After the inkassovarsel the machine stops nagging — no matter
        // how long it waits.
        assert_eq!(
            next_step(
                Some((Step::CollectionNotice, date(2026, 4, 1))),
                due_date,
                date(2026, 12, 31)
            ),
            None
        );
    }

    fn rates() -> Vec<RatePeriod> {
        [
            (date(2025, 1, 1), 1250),
            (date(2025, 7, 1), 1225),
            (date(2026, 1, 1), 1200),
        ]
        .into_iter()
        .map(|(valid_from, verdi)| RatePeriod {
            domain: "forsinkelsesrente".into(),
            valid_from,
            value: verdi,
        })
        .collect()
    }

    #[test]
    fn interest_is_segmented_across_a_rate_change_and_is_pinned() {
        // 10 000 kr, due 2025-06-15, claimed as of 2025-07-15: 15 days at
        // 12.50 % + 15 days at 12.25 %, actual days / 365.
        let beregning =
            late_payment_interest(1_000_000, date(2025, 6, 15), date(2025, 7, 15), &rates())
                .unwrap();
        assert_eq!(beregning.periods.len(), 2);
        assert_eq!(
            beregning.periods[0].from,
            date(2025, 6, 16),
            "fra dagen etter forfall"
        );
        assert_eq!(beregning.periods[0].to, date(2025, 6, 30));
        assert_eq!(beregning.periods[0].days, 15);
        assert_eq!(
            beregning.periods[0].interest_ore, 5137,
            "10000 × 12,5 % × 15/365"
        );
        assert_eq!(beregning.periods[1].from, date(2025, 7, 1));
        assert_eq!(beregning.periods[1].days, 15);
        assert_eq!(beregning.periods[1].interest_ore, 5034);
        assert_eq!(beregning.sum_ore, 10_171, "periodene summerer til totalen");
    }

    #[test]
    fn no_interest_through_the_forfall_day_itself() {
        let beregning =
            late_payment_interest(1_000_000, date(2025, 6, 15), date(2025, 6, 15), &rates())
                .unwrap();
        assert_eq!(beregning.sum_ore, 0);
        assert!(beregning.periods.is_empty());
    }

    #[test]
    fn a_missing_rate_fails_loudly_never_guesses() {
        let err = late_payment_interest(1_000_000, date(2024, 12, 15), date(2025, 1, 15), &rates())
            .unwrap_err();
        assert_eq!(
            err,
            ReminderError::MissingInterestRate {
                date: date(2024, 12, 16)
            }
        );
    }

    #[test]
    fn a_negative_amount_is_rejected() {
        assert_eq!(
            late_payment_interest(-1, date(2025, 6, 15), date(2025, 7, 15), &rates()).unwrap_err(),
            ReminderError::NegativeAmount
        );
    }

    fn ok_steps(
        step: Step,
        sent: NaiveDate,
        deadline: NaiveDate,
        fee: i64,
        history: &[PreviousStep],
    ) -> Result<(), ReminderError> {
        validate_step(step, sent, deadline, date(2026, 1, 10), fee, 3800, history)
    }

    #[test]
    fn the_step_rules_enforce_the_statutory_requirements() {
        let ingen = &[][..];
        // A reminder the day after the due date, without a fee: fine.
        assert_eq!(
            ok_steps(
                Step::Reminder,
                date(2026, 1, 11),
                date(2026, 1, 25),
                0,
                ingen
            ),
            Ok(())
        );
        // Not yet due.
        assert!(matches!(
            ok_steps(Step::Demand, date(2026, 1, 10), date(2026, 1, 30), 0, ingen),
            Err(ReminderError::NotDue { .. })
        ));
        // A fee on a reminder is not a thing.
        assert_eq!(
            ok_steps(
                Step::Reminder,
                date(2026, 1, 25),
                date(2026, 2, 10),
                3500,
                ingen
            ),
            Err(ReminderError::FeeOnReminder)
        );
        // A fee before 14 days after the due date (inkassoforskriften §1-2).
        assert_eq!(
            ok_steps(
                Step::Demand,
                date(2026, 1, 20),
                date(2026, 2, 5),
                3500,
                ingen
            ),
            Err(ReminderError::FeeTooEarly {
                earliest: date(2026, 1, 24)
            })
        );
        // Gebyr over maksimalsatsen.
        assert_eq!(
            ok_steps(
                Step::Demand,
                date(2026, 1, 24),
                date(2026, 2, 10),
                3900,
                ingen
            ),
            Err(ReminderError::FeeAboveMax { max_ore: 3800 })
        );
        // The dunning ladder is one-way.
        let etter_varsel = &[PreviousStep {
            step: Step::CollectionNotice,
            fee_ore: 0,
        }][..];
        assert!(matches!(
            ok_steps(
                Step::Demand,
                date(2026, 2, 1),
                date(2026, 2, 15),
                0,
                etter_varsel
            ),
            Err(ReminderError::StepBackwards { .. })
        ));
        // At most two fee-bearing steps.
        let to_gebyr = &[
            PreviousStep {
                step: Step::Demand,
                fee_ore: 3500,
            },
            PreviousStep {
                step: Step::Demand,
                fee_ore: 3500,
            },
        ][..];
        assert_eq!(
            ok_steps(
                Step::CollectionNotice,
                date(2026, 3, 1),
                date(2026, 3, 16),
                3800,
                to_gebyr
            ),
            Err(ReminderError::TooManyFees)
        );
        // A third step without a fee is still allowed.
        assert_eq!(
            ok_steps(
                Step::CollectionNotice,
                date(2026, 3, 1),
                date(2026, 3, 16),
                0,
                to_gebyr
            ),
            Ok(())
        );
    }

    #[test]
    fn an_inkassovarsel_requires_at_least_a_14_day_deadline() {
        assert_eq!(
            ok_steps(
                Step::CollectionNotice,
                date(2026, 2, 1),
                date(2026, 2, 14),
                0,
                &[]
            ),
            Err(ReminderError::GraceTooShort {
                at_least: date(2026, 2, 15)
            })
        );
        assert_eq!(
            ok_steps(
                Step::CollectionNotice,
                date(2026, 2, 1),
                date(2026, 2, 15),
                0,
                &[]
            ),
            Ok(())
        );
    }

    #[test]
    fn a_deadline_before_the_send_date_is_rejected() {
        assert_eq!(
            ok_steps(Step::Demand, date(2026, 2, 1), date(2026, 2, 1), 0, &[]),
            Err(ReminderError::DeadlineBeforeSendDate)
        );
    }

    #[test]
    fn the_claim_bilag_balances_and_carries_the_customer() {
        let draft = build_claim_voucher(
            "GL",
            date(2026, 2, 1),
            Step::Demand,
            7,
            "10000",
            "1500",
            "3950",
            3800,
            "8050",
            10_171,
        )
        .unwrap();
        assert_eq!(draft.description, "Purring faktura 7");
        assert_eq!(draft.entries.len(), 3);
        assert_eq!(draft.entries[0].amount, Ore(13_971));
        assert_eq!(draft.entries[0].party_no.as_deref(), Some("10000"));
        assert_eq!(draft.entries.iter().map(|e| e.amount.0).sum::<i64>(), 0);
        // Interest only, no fee: two lines.
        let bare_rente = build_claim_voucher(
            "GL",
            date(2026, 2, 1),
            Step::CollectionNotice,
            7,
            "10000",
            "1500",
            "3950",
            0,
            "8050",
            500,
        )
        .unwrap();
        assert_eq!(bare_rente.entries.len(), 2);
    }

    fn document(step: Step) -> ReminderDocument {
        ReminderDocument {
            step,
            company: "Demo AS".into(),
            orgnumber: "999888777".into(),
            customer_name: "Kari Kunde".into(),
            customer_no: "10000".into(),
            invoice_no: 7,
            invoice_date: date(2025, 6, 1),
            due_date: date(2025, 6, 15),
            sent_date: date(2025, 7, 15),
            grace_date: date(2025, 7, 29),
            remaining_amount_ore: 1_000_000,
            interest: late_payment_interest(
                1_000_000,
                date(2025, 6, 15),
                date(2025, 7, 15),
                &rates(),
            )
            .unwrap(),
            fee_ore: 3500,
            kid: Some("000000071".into()),
        }
    }

    #[test]
    fn the_document_renders_deterministically_with_its_spesifikasjon() {
        let a = render_document(&document(Step::Demand));
        assert_eq!(a, render_document(&document(Step::Demand)));
        assert!(a.starts_with("PURRING\n=======\n"));
        assert!(a.contains("Faktura 7 av 2025-06-01 forfalt 2025-06-15."));
        assert!(a.contains("12,50 %"));
        assert!(a.contains("15 dager"));
        assert!(a.contains("Purregebyr (inkassoforskriften §1-2)"));
        // 10 000,00 + 101,71 interest + 35,00 fee.
        assert!(a.contains("10136,71"));
        assert!(a.contains("KID:            000000071"));
        assert!(
            !a.contains("inkassoloven"),
            "purring er ikke et inkassovarsel"
        );
    }

    #[test]
    fn the_inkassovarsel_carries_the_statutory_text_and_stops_there() {
        let text = render_document(&document(Step::CollectionNotice));
        assert!(text.starts_with("INKASSOVARSEL\n=============\n"));
        assert!(text.contains("inkassoloven §9"));
        assert!(text.contains("regnmed utfører ikke inkasso"));
    }
}
