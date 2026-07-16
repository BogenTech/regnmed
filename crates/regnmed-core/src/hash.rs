//! Canonical hashing for the tamper-evident voucher chain.
//!
//! Every posted voucher stores `hash = SHA-256(prev_hash || content)`,
//! where `content` is the canonical serialization defined here and
//! `prev_hash` is the previous voucher's hash in the same company
//! (the first voucher chains from [`GENESIS_HASH`]).
//!
//! Rewriting any historical voucher therefore changes its hash, which
//! breaks the link of every later voucher — detectable by re-walking the
//! chain (`regnmed verify-ledger`). Anchoring the chain head outside the
//! database extends that protection to adversaries with full DB access.

use chrono::{DateTime, NaiveDate, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::Ore;

/// The hash a company's chain starts from (all zeroes).
pub const GENESIS_HASH: [u8; 32] = [0u8; 32];

/// The full business content of a voucher, as covered by its chain hash.
#[derive(Debug, Clone)]
pub struct VoucherHashInput {
    pub company_id: Uuid,
    pub chain_seq: i64,
    pub journal_code: String,
    pub fiscal_year: i32,
    pub voucher_number: i64,
    pub voucher_date: NaiveDate,
    pub description: String,
    pub reverses: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub entries: Vec<EntryHashInput>,
}

#[derive(Debug, Clone)]
pub struct EntryHashInput {
    pub line_no: i32,
    pub account_number: String,
    pub amount: Ore,
    /// Must never be `Some("")` — an empty code would hash identically to
    /// `None`. [`crate::voucher::VoucherDraft::validate`] rejects it.
    pub vat_code: Option<String>,
    /// Empty strings are normalized to `None` before hashing and storing.
    pub description: Option<String>,
}

/// `hash = SHA-256(prev_hash || canonical(voucher))`.
pub fn chain_hash(prev_hash: &[u8; 32], v: &VoucherHashInput) -> [u8; 32] {
    let mut buf = Vec::with_capacity(512);
    push_field(&mut buf, prev_hash);
    push_field(&mut buf, v.company_id.as_bytes());
    push_field(&mut buf, v.chain_seq.to_string().as_bytes());
    push_field(&mut buf, v.journal_code.as_bytes());
    push_field(&mut buf, v.fiscal_year.to_string().as_bytes());
    push_field(&mut buf, v.voucher_number.to_string().as_bytes());
    push_field(&mut buf, v.voucher_date.to_string().as_bytes());
    push_field(&mut buf, v.description.as_bytes());
    match &v.reverses {
        Some(id) => push_field(&mut buf, id.as_bytes()),
        None => push_field(&mut buf, b""),
    }
    push_field(&mut buf, v.created_by.as_bytes());
    push_field(&mut buf, canonical_timestamp(&v.created_at).as_bytes());
    push_field(&mut buf, v.entries.len().to_string().as_bytes());
    for e in &v.entries {
        push_field(&mut buf, e.line_no.to_string().as_bytes());
        push_field(&mut buf, e.account_number.as_bytes());
        push_field(&mut buf, e.amount.0.to_string().as_bytes());
        push_field(&mut buf, e.vat_code.as_deref().unwrap_or("").as_bytes());
        push_field(&mut buf, e.description.as_deref().unwrap_or("").as_bytes());
    }
    Sha256::digest(&buf).into()
}

/// Appends one field as `<len>:<bytes>;` (a netstring). Length-prefixing
/// makes the serialization unambiguous regardless of field content — no
/// delimiter inside a description can collide with another field.
fn push_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(bytes.len().to_string().as_bytes());
    buf.push(b':');
    buf.extend_from_slice(bytes);
    buf.push(b';');
}

/// Timestamps are hashed at microsecond precision because that is what
/// Postgres `timestamptz` stores. Always pass timestamps through
/// [`truncate_to_micros`] before both hashing and inserting, so the stored
/// value re-hashes identically during verification.
pub fn canonical_timestamp(ts: &DateTime<Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

pub fn truncate_to_micros(ts: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(ts.timestamp_micros())
        .expect("timestamp within representable range")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VoucherHashInput {
        VoucherHashInput {
            company_id: Uuid::from_u128(1),
            chain_seq: 1,
            journal_code: "GL".into(),
            fiscal_year: 2026,
            voucher_number: 1,
            voucher_date: NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            description: "Salg".into(),
            reverses: None,
            created_by: "test".into(),
            created_at: truncate_to_micros(
                DateTime::from_timestamp(1_800_000_000, 123_456_789).unwrap(),
            ),
            entries: vec![
                EntryHashInput {
                    line_no: 1,
                    account_number: "1920".into(),
                    amount: Ore(12_500_00),
                    vat_code: None,
                    description: None,
                },
                EntryHashInput {
                    line_no: 2,
                    account_number: "3000".into(),
                    amount: Ore(-12_500_00),
                    vat_code: Some("3".into()),
                    description: None,
                },
            ],
        }
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(
            chain_hash(&GENESIS_HASH, &sample()),
            chain_hash(&GENESIS_HASH, &sample())
        );
    }

    #[test]
    fn tampering_with_an_amount_changes_the_hash() {
        let original = chain_hash(&GENESIS_HASH, &sample());
        let mut tampered = sample();
        tampered.entries[0].amount = Ore(12_500_01);
        assert_ne!(original, chain_hash(&GENESIS_HASH, &tampered));
    }

    #[test]
    fn hash_depends_on_previous_hash() {
        let a = chain_hash(&GENESIS_HASH, &sample());
        let b = chain_hash(&[1u8; 32], &sample());
        assert_ne!(a, b);
    }

    #[test]
    fn timestamp_truncation_is_stable() {
        let ts = DateTime::from_timestamp(1_800_000_000, 123_456_789).unwrap();
        let truncated = truncate_to_micros(ts);
        assert_eq!(truncated, truncate_to_micros(truncated));
        assert_eq!(
            canonical_timestamp(&truncated),
            "2027-01-15T08:00:00.123456Z"
        );
    }
}
