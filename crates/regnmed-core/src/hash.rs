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
//!
//! # Format versions
//!
//! The serialization is frozen per version; every voucher stores which
//! version hashed it, and mixed-version chains verify fine:
//!
//! - **v1** (original): no party field. History posted before reskontro.
//! - **v2**: starts with a `"v2"` marker field and adds the entry's
//!   party number (kundenummer/leverandørnummer, empty when none) after
//!   the description — so reassigning a receivable to another customer
//!   breaks the chain like any other tampering.
//!
//! A version is never edited, only superseded; the golden tests pin one
//! digest per version.

use chrono::{DateTime, NaiveDate, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::Ore;

/// SHA-256 of arbitrary bytes — used for attachment content hashes.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The hash a company's chain starts from (all zeroes).
pub const GENESIS_HASH: [u8; 32] = [0u8; 32];

/// Current format for new postings.
pub const HASH_VERSION_CURRENT: i16 = 2;

/// The full business content of a voucher, as covered by its chain hash.
#[derive(Debug, Clone)]
pub struct VoucherHashInput {
    /// Which frozen serialization hashed this voucher (1 or 2).
    pub hash_version: i16,
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
    /// Reskontro party number — v2 only; v1 vouchers have none.
    pub party_no: Option<String>,
}

/// `hash = SHA-256(prev_hash || canonical(voucher))`, per the voucher's
/// stored format version.
pub fn chain_hash(prev_hash: &[u8; 32], v: &VoucherHashInput) -> [u8; 32] {
    let mut buf = Vec::with_capacity(512);
    push_field(&mut buf, prev_hash);
    if v.hash_version >= 2 {
        // Version marker: v2 streams can never collide with v1 streams,
        // whose first field is always a 32-byte prev-hash... which this
        // marker field's length prefix ("2:") already differs from.
        push_field(&mut buf, b"v2");
    }
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
        if v.hash_version >= 2 {
            push_field(&mut buf, e.party_no.as_deref().unwrap_or("").as_bytes());
        }
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
            hash_version: 1,
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
                    party_no: None,
                },
                EntryHashInput {
                    line_no: 2,
                    account_number: "3000".into(),
                    amount: Ore(-12_500_00),
                    vat_code: Some("3".into()),
                    description: None,
                    party_no: None,
                },
            ],
        }
    }

    fn sample_v2() -> VoucherHashInput {
        let mut v = sample();
        v.hash_version = 2;
        v.entries[0].account_number = "1500".into();
        v.entries[0].party_no = Some("10001".into());
        v
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

    /// Locks each frozen serialization forever. If either digest changes,
    /// the change breaks chain verification of ledgers already in
    /// production — a format cannot be "improved", only superseded by the
    /// next version.
    #[test]
    fn golden_hashes_never_change() {
        let v1 = chain_hash(&GENESIS_HASH, &sample());
        assert_eq!(
            v1.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "1001ebbe2aad6c76a9978f972056ad5d7be922828acb8e5ef55c35d6a16ebc8b"
        );
        let v2 = chain_hash(&GENESIS_HASH, &sample_v2());
        assert_eq!(
            v2.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "ff271302ffd635d1d229e461d8200001781fcf67287407fa03487190b3e664d7"
        );
    }

    /// The party binding is inside the v2 hash: moving a receivable to a
    /// different customer is tampering like any other.
    #[test]
    fn v2_hashes_the_party_and_differs_from_v1() {
        let original = chain_hash(&GENESIS_HASH, &sample_v2());
        let mut reassigned = sample_v2();
        reassigned.entries[0].party_no = Some("10002".into());
        assert_ne!(original, chain_hash(&GENESIS_HASH, &reassigned));

        // Same content hashed as v1 vs v2 must differ (version marker).
        let mut as_v1 = sample_v2();
        as_v1.hash_version = 1;
        as_v1.entries[0].party_no = None;
        let mut as_v2_no_party = sample_v2();
        as_v2_no_party.entries[0].party_no = None;
        assert_ne!(
            chain_hash(&GENESIS_HASH, &as_v1),
            chain_hash(&GENESIS_HASH, &as_v2_no_party)
        );
    }

    /// The netstring framing must keep field boundaries unambiguous:
    /// moving a character between adjacent fields must change the hash.
    #[test]
    fn field_boundaries_cannot_collide() {
        let a = {
            let mut v = sample();
            v.journal_code = "GLX".into();
            v.description = "Salg".into();
            v
        };
        let b = {
            let mut v = sample();
            v.journal_code = "GL".into();
            v.description = "XSalg".into();
            v
        };
        assert_ne!(chain_hash(&GENESIS_HASH, &a), chain_hash(&GENESIS_HASH, &b));

        // None and Some("") on an entry field must also differ via the
        // count/normalization contract upstream; here the serialization at
        // least distinguishes an empty description from a missing entry.
        let c = {
            let mut v = sample();
            v.entries[0].description = Some("x".into());
            v
        };
        assert_ne!(
            chain_hash(&GENESIS_HASH, &sample()),
            chain_hash(&GENESIS_HASH, &c)
        );
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
