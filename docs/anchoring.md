# External anchoring of chain heads

The hash chain ([ledger.md](ledger.md)) makes tampering *detectable
within the database*. Anchoring makes it detectable — and provable —
even against an adversary with **full database control**, who could
otherwise rewrite the entire chain from genesis and recompute every
hash. The principle: periodically freeze every company's chain head
under a single Merkle root, and get that root out of the database and
out of the adversary's reach. Any later rewrite of anchored history
contradicts evidence the adversary cannot recall.

This is the mechanism behind the product promise: *don't trust us —
verify.*

## What is anchored

An **anchor snapshot** is taken periodically (nightly CronJob in the
cluster; `regnmed anchor` is the ops entry point). It reads every
company's chain head (`last_seq`, `last_hash`) in one MVCC-consistent
statement and builds a Merkle tree:

- **Leaf content** (format v1, frozen like the voucher hash formats):
  netstring fields `"regnmed-anchor-v1"`, company id (16 raw bytes),
  chain seq (decimal), chain head hash (32 raw bytes).
- **Leaf hash** `SHA-256(0x00 ‖ content)`, **node hash**
  `SHA-256(0x01 ‖ left ‖ right)` — the domain-separation prefixes make
  leaves and interior nodes unforgeable as each other.
- Leaves are **sorted by company id**, so the root is independent of
  query order; an odd node is promoted unchanged (Certificate
  Transparency style, never duplicated).

One root covers every tenant and reveals nothing about any of them.
Implementation: `regnmed-core::anchor` (pure — golden test pins the v1
root digest; the format can only be superseded, never edited).

## Where the root goes

1. **The public transparency feed** — `GET /anchors`, deliberately
   unauthenticated: snapshot ids, timestamps, root hashes, leaf counts,
   witness metadata. Nothing else. Every independent copy of a root — a
   revisor's notebook, a monitoring job, a customer's cron — is one more
   witness a database rewrite cannot reach. This is the "many eyes"
   substitute for blockchain consensus.
2. **RFC 3161 timestamp tokens** — when `ANCHOR_TSA_URL` is set (e.g. a
   free public TSA, or a qualified Norwegian/EU TSA for production), the
   root is sent to the Time-Stamp Authority, whose signed token ("this
   hash existed no later than time T") is stored in `anchor_witness`.
   The DER structures are hand-rolled in `regnmed-gov::tsa` (a SHA-256
   `TimeStampReq` is 59 fixed bytes — no ASN.1 stack needed; a golden
   test pins the request bytes). No nonce is sent: every root is unique,
   so token replay is meaningless.

Future witness methods slot into the same `anchor_witness` table
(method, reference, proof): OpenTimestamps/Bitcoin, a public git repo,
publication in the enterprise's own systems.

## Storage is itself evidence

`anchor_snapshot`, `anchor_leaf` and `anchor_witness` (migration 0014)
are **append-only** exactly like the ledger: the same trigger function
rejects UPDATE/DELETE/TRUNCATE, and `regnmed_app` holds only
INSERT/SELECT. An adversary rewriting chains must also erase anchors —
and the externally published roots still convict them.

## Verification

- `GET /companies/{id}/anchors` (any access level; 404 for outsiders)
  returns, per snapshot: the anchored head and the **Merkle inclusion
  proof** connecting it to the published root. A revisor can verify the
  proof offline with `regnmed_core::anchor::verify_inclusion` — no
  trust in the API required beyond the public root.
- `GET /companies/{id}/anchors/verify` runs the full independent check:
  chain from genesis, attachment content hashes, and every anchored
  head against the live chain (the voucher at the anchored seq must
  still carry the anchored hash, and each stored root must recompute
  from its leaves). The portal's oversikt page exposes this as
  "Verifiser kjeden mot forankringen".
- `regnmed verify-ledger` performs the same anchor checks and **fails**
  on any mismatch.
- An RFC 3161 token verifies offline with standard tooling:
  `openssl ts -reply -in token.tsr -text` shows the timestamped digest
  (the root) and time; `openssl ts -verify -digest <root-hex> -in
  token.tsr -CAfile <tsa-ca.pem>` proves it cryptographically.

What each check proves:

| Check | Defeats |
| --- | --- |
| Chain re-walk from genesis | Any in-place edit below full-DB control |
| Anchored head vs live chain | Rewrite/truncation *after* the snapshot |
| Root recomputes from leaves | Tampering with the anchor rows themselves |
| External witness (feed copies, RFC 3161) | Deleting/replacing anchors wholesale |

The detection window is the anchoring cadence: history older than the
newest witnessed root is protected; the current day's postings are
protected by the next snapshot. Nightly is the default; cadence is an
ops decision, not a code change.

## Deliberately not yet

- **Attachment-set binding.** Attachment *content* is already
  hash-verified (perioder.md), but the anchor does not yet prove an
  attachment existed at snapshot time (a DBA deleting a whole attachment
  row is caught by `verify-ledger` only while the row's absence is
  noticed, not proven). Doing this honestly needs a per-company
  attachment sequence so the anchored prefix is recomputable — a small
  migration + posting change, planned as a follow-up on the same leaf
  format (v2 adds an `attachments` field; v1 roots verify forever).
- **Signed anchor feed.** The feed relies on plurality of copies, not a
  signature. A signing key would itself live with the operator, adding
  little against the DBA threat model; external witnesses add more.

## Where it is tested

- `crates/regnmed-core/src/anchor.rs` — golden v1 root, order
  independence, proofs for every leaf at sizes 1–8, tampered leaves and
  foreign proofs rejected.
- `crates/regnmed-gov/src/tsa.rs` — golden `TimeStampReq` DER bytes,
  PKIStatus parsing (granted/rejected/long-form/malformed).
- `crates/regnmed-api/tests/anchor.rs` (real Postgres, also in CI) —
  public feed carries the root; per-company proof from the API verifies
  against the public root using only regnmed-core; clean chain passes;
  a planted anchor claiming a different head is reported as a rewrite
  *and* as a non-recomputing root; anchor rows reject UPDATE/DELETE.
