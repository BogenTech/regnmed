//! Bank reconciliation matching: propose pairings between imported bank
//! transactions and ledger entries on the bank account.
//!
//! Pure and deterministic. Rules, in order of confidence:
//! 1. Equal amount, same booking date.
//! 2. Equal amount, dates within `max_days`.
//!
//! Each entry and each bank transaction is used at most once. When two
//! candidates are equally close, **nothing** is auto-matched — ambiguity
//! is for the accountant to resolve manually, never for a heuristic to
//! guess. (Reference/KID matching arrives with reskontro, where ledger
//! entries gain payment references.)

use chrono::NaiveDate;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BankTx {
    pub id: Uuid,
    pub booking_date: NaiveDate,
    /// Ledger sign for the bank account (money in positive).
    pub amount_ore: i64,
}

#[derive(Debug, Clone)]
pub struct OpenEntry {
    pub entry_id: Uuid,
    pub date: NaiveDate,
    pub amount_ore: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Proposal {
    pub bank_tx_id: Uuid,
    pub entry_id: Uuid,
    pub same_day: bool,
}

pub fn propose_matches(
    transactions: &[BankTx],
    entries: &[OpenEntry],
    max_days: i64,
) -> Vec<Proposal> {
    let mut used_entries: Vec<bool> = vec![false; entries.len()];
    let mut proposals = Vec::new();

    // Deterministic processing order regardless of input order.
    let mut order: Vec<&BankTx> = transactions.iter().collect();
    order.sort_by_key(|tx| (tx.booking_date, tx.amount_ore, tx.id));

    for tx in order {
        let mut best: Option<(i64, usize)> = None; // (distance, index)
        let mut ambiguous = false;
        for (index, entry) in entries.iter().enumerate() {
            if used_entries[index] || entry.amount_ore != tx.amount_ore {
                continue;
            }
            let distance = (entry.date - tx.booking_date).num_days().abs();
            if distance > max_days {
                continue;
            }
            match best {
                Some((best_distance, _)) if distance > best_distance => {}
                Some((best_distance, _)) if distance == best_distance => ambiguous = true,
                _ => {
                    best = Some((distance, index));
                    ambiguous = false;
                }
            }
        }
        if let Some((distance, index)) = best
            && !ambiguous
        {
            used_entries[index] = true;
            proposals.push(Proposal {
                bank_tx_id: tx.id,
                entry_id: entries[index].entry_id,
                same_day: distance == 0,
            });
        }
    }
    proposals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, day).unwrap()
    }

    fn tx(id: u128, day: u32, ore: i64) -> BankTx {
        BankTx {
            id: Uuid::from_u128(id),
            booking_date: date(day),
            amount_ore: ore,
        }
    }

    fn entry(id: u128, day: u32, ore: i64) -> OpenEntry {
        OpenEntry {
            entry_id: Uuid::from_u128(id),
            date: date(day),
            amount_ore: ore,
        }
    }

    #[test]
    fn matches_equal_amounts_preferring_same_day() {
        let txs = [tx(1, 20, 1_250_000), tx(2, 25, -15_000)];
        let entries = [
            entry(10, 20, 1_250_000),
            entry(11, 24, -15_000), // one day off — still within window
        ];
        let proposals = propose_matches(&txs, &entries, 5);
        assert_eq!(proposals.len(), 2);
        assert!(proposals[0].same_day);
        assert_eq!(proposals[0].entry_id, Uuid::from_u128(10));
        assert!(!proposals[1].same_day);
    }

    #[test]
    fn never_reuses_an_entry_and_respects_the_window() {
        // Two identical bank txs, one entry: only one match.
        let txs = [tx(1, 20, 500_00), tx(2, 21, 500_00)];
        let entries = [entry(10, 20, 500_00)];
        assert_eq!(propose_matches(&txs, &entries, 5).len(), 1);

        // Outside the window: no match.
        let far = [entry(11, 28, 500_00)];
        assert!(propose_matches(&[tx(3, 20, 500_00)], &far, 5).is_empty());
    }

    #[test]
    fn equal_distance_candidates_are_ambiguous_not_guessed() {
        // Entry on day 19 and day 21 — both one day from the tx on day 20.
        let entries = [entry(10, 19, 500_00), entry(11, 21, 500_00)];
        assert!(
            propose_matches(&[tx(1, 20, 500_00)], &entries, 5).is_empty(),
            "a tie must fall to manual review"
        );
    }

    #[test]
    fn amounts_must_match_exactly_including_sign() {
        let entries = [entry(10, 20, -500_00)];
        assert!(propose_matches(&[tx(1, 20, 500_00)], &entries, 5).is_empty());
    }
}
