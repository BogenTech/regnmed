# The ledger: append-only, hash-chained, independently verifiable

The ledger records vouchers (bilag) with double-entry lines. It is
designed so that **history cannot change without detection** — the
property bokføringsloven's sporbarhet requirements ask for, and the
product's core trust story ("don't trust us — verify").

## Invariants

1. **Double entry**: every voucher has ≥ 2 lines summing to exactly 0 øre.
2. **Gap-free numbering**: voucher numbers per journal + fiscal year have
   no gaps, even across failed/rolled-back postings.
3. **Append-only**: posted vouchers and lines are never updated or
   deleted. Corrections are new reversing vouchers
   (`reverses_voucher_id`).
4. **Tamper-evidence**: every voucher is hash-chained; rewriting any
   historical voucher breaks the chain from that point on.

## The three enforcement layers

| Layer | Mechanism | Where |
| --- | --- | --- |
| Domain | `VoucherDraft::validate` rejects unbalanced/degenerate vouchers before any I/O | `crates/regnmed-core/src/voucher.rs` |
| Database | Deferred constraint trigger re-checks balance at COMMIT; append-only triggers reject UPDATE/DELETE/TRUNCATE on `voucher`/`entry`; the app role (`regnmed_app`) is only GRANTed INSERT/SELECT on ledger tables | `crates/regnmed-db/migrations/0002…0004` |
| Crypto | `hash = SHA-256(prev_hash ‖ canonical(voucher))`, chained from a genesis hash per company | `crates/regnmed-core/src/hash.rs` |

The layers are independent: an attacker must defeat all three, and the
crypto layer is verifiable by parties who don't trust the database at
all.

## The hash chain, precisely

- Canonical serialization: every field of the voucher (company, sequence,
  journal, year, number, date, description, reversal reference, creator,
  timestamp, and every line) is length-prefixed as a netstring
  (`<len>:<bytes>;`), so no field content can masquerade as another
  field. Entry count is included, so lines cannot be appended silently.
- Timestamps are truncated to microseconds before hashing **and** before
  storage, because Postgres `timestamptz` stores microseconds — the
  stored row must re-hash identically forever.
- The serialization is **frozen**. A golden test pins the exact digest
  (`golden_hash_never_changes`); if it fails, the change would break
  verification of every deployed ledger. The format can be versioned,
  never edited.
- Posting locks the company's `chain_head` row FOR UPDATE, which
  serializes postings per company — required by both the chain and
  gap-free numbering. The chain head is a mutable *pointer*, not history;
  verification recomputes the chain from the vouchers and only compares
  the result against it.

## Verification

`regnmed verify-ledger` re-walks every company's chain from genesis:
recomputes each voucher's hash from stored business content, checks the
link to the previous hash, and finally compares against the chain head.
Any edited amount, description, date or deleted voucher is reported with
its sequence number.

## Trust model (vs. blockchain)

Structurally this is a blockchain-style hash chain (git history and
Certificate Transparency are the close relatives). Bitcoin's immutability
comes from decentralized consensus + proof-of-work; ours cannot, because
there is one writer. Instead:

- Everyone below full-database control is stopped by the database layer.
- An adversary with full DB control *could* rewrite the entire chain and
  recompute all hashes — which is why **external anchoring of chain
  heads** (roadmap M6, issue #25) closes the gap: once a chain head is
  published outside the database (timestamping service, or simply sent to
  the revisor), a rewrite is provable because the recomputed head no
  longer matches the anchored one.
- The revisor running `verify-ledger` against an anchored head plays the
  role consensus plays in Bitcoin — third-party verifiability without the
  energy cost, which is the right trade-off for accounting.

## Where it is tested

- `crates/regnmed-core/src/hash.rs` — determinism, tamper sensitivity,
  golden digest, netstring boundary collisions, timestamp stability.
- `crates/regnmed-db/tests/ledger.rs` (against real Postgres, also in
  CI): gap-free numbers across failed postings; UPDATE/DELETE rejected;
  hand-inserted unbalanced voucher rejected at COMMIT; a simulated
  DBA-level adversary (triggers disabled via `session_replication_role`)
  detected by `verify_chain`, and clean verification after repair.
