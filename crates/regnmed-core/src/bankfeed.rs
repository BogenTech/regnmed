//! Bank feed (docs/bank.md tier 2): transactions fetched from the bank
//! through an aggregator, turned into the same statement shape the file
//! import produces. Pure: the provider client lives in `regnmed-gov`.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::camt053::{Camt053Transaction, CamtError, parse_amount};

/// One booked transaction as a provider reports it, before any sign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedTx {
    /// The bank's own reference for the entry, when it gives one.
    pub entry_reference: Option<String>,
    pub booking_date: NaiveDate,
    /// Decimal string, e.g. "12500.00". The sign is ignored: the direction
    /// is `credit`.
    pub amount: String,
    /// Money INTO the account.
    pub credit: bool,
    pub description: String,
    /// KID / payment reference when present.
    pub reference: Option<String>,
}

/// A stable identity per transaction, so overlapping fetch windows never
/// import the same transaction twice.
///
/// With a bank reference the key is the reference plus date, amount and
/// direction: the text is left out, because banks enrich descriptions after
/// booking and that must not make a second copy. Without one it is a hash of
/// the content. Either way an ordinal among IDENTICAL transactions that day is
/// appended, so two equal payments on one day are two keys — also when a bank
/// reuses a reference — and they stay the same two keys as long as a fetch
/// covers whole days (it does: the window is given in dates).
pub fn dedupe_keys(txs: &[FeedTx]) -> Vec<String> {
    let mut ordinals: HashMap<String, usize> = HashMap::new();
    txs.iter()
        .map(|t| {
            let amount = t.amount.trim().trim_start_matches('-');
            let base = match t.entry_reference.as_deref().map(str::trim) {
                Some(r) if !r.is_empty() => {
                    format!("ref:{r}:{}:{amount}:{}", t.booking_date, t.credit)
                }
                _ => {
                    let content = format!(
                        "{}|{amount}|{}|{}|{}",
                        t.booking_date,
                        t.credit,
                        t.description,
                        t.reference.as_deref().unwrap_or("")
                    );
                    let digest = crate::hash::sha256(content.as_bytes());
                    format!(
                        "hash:{}",
                        digest
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    )
                }
            };
            let n = ordinals.entry(base.clone()).or_insert(0);
            *n += 1;
            format!("{base}#{n}")
        })
        .collect()
}

/// Ledger sign for the bank account: money in is positive, like camt.053.
pub fn to_transaction(tx: &FeedTx) -> Result<Camt053Transaction, CamtError> {
    let ore = parse_amount(tx.amount.trim().trim_start_matches('-'))?;
    Ok(Camt053Transaction {
        booking_date: tx.booking_date,
        amount_ore: if tx.credit { ore } else { -ore },
        description: tx.description.clone(),
        reference: tx.reference.clone(),
    })
}

/// "NO9386011117947" or "8601 11 17947" -> the 11-digit BBAN, if that is
/// what it is. Used to check that the account a bank returns is the one
/// the user said they were connecting.
pub fn norwegian_bban(text: &str) -> Option<String> {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '.')
        .collect();
    let digits = match compact
        .strip_prefix("NO")
        .or_else(|| compact.strip_prefix("no"))
    {
        Some(rest) if rest.len() == 13 => &rest[2..],
        Some(_) => return None,
        None => compact.as_str(),
    };
    (digits.len() == 11 && digits.chars().all(|c| c.is_ascii_digit())).then(|| digits.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(reference: Option<&str>, amount: &str, credit: bool) -> FeedTx {
        FeedTx {
            entry_reference: reference.map(str::to_string),
            booking_date: NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            amount: amount.into(),
            credit,
            description: "Fjellvik Bygg AS".into(),
            reference: None,
        }
    }

    #[test]
    fn a_reference_keys_the_transaction_whatever_the_text_says() {
        let mut enriched = tx(Some(" A1 "), "100.00", true);
        enriched.description = "Fjellvik Bygg AS, faktura 1042".into();
        let keys = dedupe_keys(&[
            tx(Some("A1"), "100.00", true),
            tx(Some("A2"), "100.00", true),
        ]);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(
            dedupe_keys(&[enriched])[0],
            keys[0],
            "a later, richer description is the same transaction"
        );
    }

    #[test]
    fn a_reused_reference_does_not_merge_two_transactions() {
        // A bank that puts a batch id (or "0") in the reference.
        let day = [
            tx(Some("0"), "100.00", true),
            tx(Some("0"), "100.00", true),
            tx(Some("0"), "55.00", false),
        ];
        let keys = dedupe_keys(&day);
        assert_eq!(
            keys.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
        assert_eq!(keys, dedupe_keys(&day));
        // Another day with the same reference is another transaction.
        let mut next = tx(Some("0"), "100.00", true);
        next.booking_date = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        assert!(!keys.contains(&dedupe_keys(&[next])[0]));
    }

    #[test]
    fn identical_transactions_without_a_reference_stay_distinct_and_stable() {
        let day = [
            tx(None, "100.00", true),
            tx(None, "100.00", true),
            tx(Some(""), "50.00", false),
        ];
        let keys = dedupe_keys(&day);
        assert_ne!(keys[0], keys[1], "two equal payments are two transactions");
        assert_ne!(keys[0], keys[2]);
        assert_eq!(
            keys,
            dedupe_keys(&day),
            "the same fetch gives the same keys"
        );
        // A later fetch of the same day with one more transaction keeps the old keys.
        let mut later = day.to_vec();
        later.push(tx(None, "100.00", true));
        assert_eq!(dedupe_keys(&later)[..3], keys[..]);
    }

    #[test]
    fn the_direction_gives_the_sign_never_the_amount_string() {
        assert_eq!(
            to_transaction(&tx(None, "12500.00", true))
                .unwrap()
                .amount_ore,
            1_250_000
        );
        assert_eq!(
            to_transaction(&tx(None, "12500.00", false))
                .unwrap()
                .amount_ore,
            -1_250_000
        );
        assert_eq!(
            to_transaction(&tx(None, "-99.50", false))
                .unwrap()
                .amount_ore,
            -9_950
        );
        assert!(to_transaction(&tx(None, "abc", true)).is_err());
    }

    #[test]
    fn a_norwegian_iban_and_a_formatted_account_number_are_the_same_account() {
        assert_eq!(
            norwegian_bban("NO9386011117947").as_deref(),
            Some("86011117947")
        );
        assert_eq!(
            norwegian_bban("8601 11 17947").as_deref(),
            Some("86011117947")
        );
        assert_eq!(
            norwegian_bban("8601.11.17947").as_deref(),
            Some("86011117947")
        );
        assert_eq!(norwegian_bban("SE4550000000058398257466"), None);
        assert_eq!(norwegian_bban("12345"), None);
    }
}
