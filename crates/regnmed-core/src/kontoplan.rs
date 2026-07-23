//! Kontoplan mapping: from a foreign chart of accounts to NS 4102.
//!
//! SAF-T import only accepts 4-digit NS 4102 accounts — that rule keeps
//! grouping codes, reports and mva logic sound. Files from systems with
//! other numbering (5-digit charts, custom ranges) go through this
//! wizard instead: we *suggest* a mapping, the administrator reviews
//! and completes it, and only then does the import run with the mapping
//! applied. Suggestions are heuristics; the human decision is what gets
//! recorded.
//!
//! Heuristics, in order:
//! 1. Already a 4-digit number → itself.
//! 2. Longer digit string whose first four digits form a plausible
//!    account (1000–8999) → truncated ("15000" → "1500").
//! 3. Shorter digit string → zero-padded to four ("150" → "1500").
//! 4. Name match against the standard account names from the vendored
//!    næringsspesifikasjon list (normalized equality, then containment).
//! 5. No suggestion — the wizard shows an empty field the administrator
//!    must fill.

use std::collections::HashMap;

use crate::saft::standard_accounts;
use crate::saft_import::SaftFile;

#[derive(Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub account_id: String,
    pub name: String,
    /// Suggested NS 4102 account, when a heuristic applies.
    pub suggested: Option<String>,
    /// The standard list's name for the suggested account, when listed.
    pub standard_name: Option<String>,
    /// Which heuristic produced it (Norwegian, shown in the wizard).
    pub reason: &'static str,
}

fn is_four_digit(id: &str) -> bool {
    id.len() == 4 && id.chars().all(|c| c.is_ascii_digit())
}

fn normalize(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn standard_name_of(code: &str) -> Option<String> {
    let number: u32 = code.parse().ok()?;
    standard_accounts()
        .iter()
        .find(|(c, _)| *c == number)
        .map(|(_, name)| (*name).to_string())
}

fn by_name(name: &str) -> Option<u32> {
    let needle = normalize(name);
    if needle.len() < 4 {
        return None;
    }
    let table = standard_accounts();
    if let Some((code, _)) = table.iter().find(|(_, n)| normalize(n) == needle) {
        return Some(*code);
    }
    table
        .iter()
        .find(|(_, n)| {
            let hay = normalize(n);
            hay.contains(&needle) || needle.contains(&hay)
        })
        .map(|(code, _)| *code)
}

pub fn suggest_one(account_id: &str, name: &str) -> Suggestion {
    let (suggested, reason) = if is_four_digit(account_id) {
        (Some(account_id.to_string()), "allerede NS 4102")
    } else if account_id.len() > 4 && account_id.chars().all(|c| c.is_ascii_digit()) {
        let head = &account_id[..4];
        match head.parse::<u32>() {
            Ok(n) if (1000..=8999).contains(&n) => (Some(head.to_string()), "avkortet"),
            _ => (None, "ingen forslag"),
        }
    } else if !account_id.is_empty()
        && account_id.len() < 4
        && account_id.chars().all(|c| c.is_ascii_digit())
    {
        (Some(format!("{account_id:0<4}")), "utvidet med nuller")
    } else if let Some(code) = by_name(name) {
        (Some(code.to_string()), "navnetreff")
    } else {
        (None, "ingen forslag")
    };
    let standard_name = suggested.as_deref().and_then(standard_name_of);
    Suggestion {
        account_id: account_id.to_string(),
        name: name.to_string(),
        suggested,
        standard_name,
        reason,
    }
}

pub fn suggest(accounts: &[(String, String)]) -> Vec<Suggestion> {
    accounts
        .iter()
        .map(|(id, name)| suggest_one(id, name))
        .collect()
}

/// Rewrites a parsed SAF-T file through the reviewed mapping: account
/// ids in accounts and transaction lines are replaced; accounts that
/// end up on the same NS 4102 number are merged (openings summed, first
/// name kept). Ids without a mapping entry pass through unchanged — the
/// import's own 4-digit validation still guards the result, so a
/// half-finished mapping fails loudly instead of importing garbage.
pub fn apply_mapping(file: &mut SaftFile, mapping: &HashMap<String, String>) -> Result<(), String> {
    for (from, to) in mapping {
        if !is_four_digit(to) {
            return Err(format!(
                "mapping '{from}' → '{to}': målet må være et firesifret NS 4102-kontonummer"
            ));
        }
    }
    let target =
        |id: &str| -> String { mapping.get(id).cloned().unwrap_or_else(|| id.to_string()) };

    let mut merged: Vec<crate::saft_import::ImportAccount> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for account in file.accounts.drain(..) {
        let id = target(&account.account_id);
        match index.get(&id) {
            Some(&i) => merged[i].opening_ore += account.opening_ore,
            None => {
                index.insert(id.clone(), merged.len());
                merged.push(crate::saft_import::ImportAccount {
                    account_id: id,
                    name: account.name,
                    opening_ore: account.opening_ore,
                });
            }
        }
    }
    file.accounts = merged;

    for transaction in &mut file.transactions {
        for line in &mut transaction.lines {
            line.account_id = target(&line.account_id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_digit_accounts_map_to_themselves() {
        let s = suggest_one("1920", "Bank");
        assert_eq!(s.suggested.as_deref(), Some("1920"));
        assert_eq!(s.reason, "allerede NS 4102");
        assert!(s.standard_name.is_some(), "1920 is a standard account");
    }

    #[test]
    fn longer_numbers_truncate_and_shorter_pad() {
        assert_eq!(
            suggest_one("15000", "Kundefordringer").suggested.as_deref(),
            Some("1500")
        );
        assert_eq!(
            suggest_one("192001", "Driftskonto").suggested.as_deref(),
            Some("1920")
        );
        let padded = suggest_one("150", "Kundefordringer");
        assert_eq!(padded.suggested.as_deref(), Some("1500"));
        assert_eq!(padded.reason, "utvidet med nuller");
        // A truncation outside 1000–8999 is not a plausible account.
        assert_eq!(suggest_one("990001", "Diverse").suggested, None);
    }

    #[test]
    fn name_matching_catches_non_numeric_charts() {
        let s = suggest_one("GOODW", "Goodwill");
        assert_eq!(s.suggested.as_deref(), Some("1080"), "{s:?}");
        assert_eq!(s.reason, "navnetreff");
        assert_eq!(suggest_one("X1", "Zq").suggested, None);
    }

    #[test]
    fn apply_mapping_rewrites_merges_and_validates() {
        use crate::saft_import::{ImportAccount, ImportLine, ImportTransaction, SaftFile};
        let mut file = SaftFile {
            selection_start: None,
            accounts: vec![
                ImportAccount {
                    account_id: "15000".into(),
                    name: "Kunder NO".into(),
                    opening_ore: 100_00,
                },
                ImportAccount {
                    account_id: "15001".into(),
                    name: "Kunder SE".into(),
                    opening_ore: 50_00,
                },
                ImportAccount {
                    account_id: "1920".into(),
                    name: "Bank".into(),
                    opening_ore: -150_00,
                },
            ],
            customers: vec![],
            suppliers: vec![],
            transactions: vec![ImportTransaction {
                source_id: "1".into(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
                description: "Salg".into(),
                lines: vec![
                    ImportLine {
                        account_id: "15000".into(),
                        amount_ore: 10_00,
                        description: None,
                        customer_id: None,
                        supplier_id: None,
                        tax_code: None,
                    },
                    ImportLine {
                        account_id: "1920".into(),
                        amount_ore: -10_00,
                        description: None,
                        customer_id: None,
                        supplier_id: None,
                        tax_code: None,
                    },
                ],
            }],
        };
        let mapping: HashMap<String, String> = [
            ("15000".to_string(), "1500".to_string()),
            ("15001".to_string(), "1500".to_string()),
        ]
        .into();
        apply_mapping(&mut file, &mapping).unwrap();
        assert_eq!(file.accounts.len(), 2, "two foreign accounts merged");
        let kunder = file
            .accounts
            .iter()
            .find(|a| a.account_id == "1500")
            .unwrap();
        assert_eq!(kunder.opening_ore, 150_00, "openings summed");
        assert_eq!(file.transactions[0].lines[0].account_id, "1500");
        // Openings still balance after the merge.
        assert_eq!(file.accounts.iter().map(|a| a.opening_ore).sum::<i64>(), 0);

        let bad: HashMap<String, String> = [("1920".to_string(), "19200".to_string())].into();
        assert!(apply_mapping(&mut file, &bad).is_err());
    }
}
