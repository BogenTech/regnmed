---
title: Verify it yourself
description: Don't trust us, check. How to verify that a general ledger has not been altered, without trusting the API.
sidebar:
  order: 9
---

regnmed does not promise that the general ledger is unaltered. It lets you **check** it. The guarantee rests on three layers, and the last layer is outside our control:

1. **Domain**: corrections are reversing vouchers, never edits.
2. **Database**: triggers reject `UPDATE`, `DELETE` and `TRUNCATE` on vouchers and postings, and the application role has only `INSERT` and `SELECT`.
3. **Cryptography**: every voucher stores `SHA-256(previous_hash ‖ canonical content)`. Every day all companies' chain heads are frozen under one Merkle root that is published openly.

The details are in [The general ledger](/teknisk/ledger/) and [Anchoring](/teknisk/anchoring/). Here is the practical part.

## Quick check: let regnmed run the verification

```sh
curl -s https://api.regnmed.no/companies/$COMPANY/anchors/verify -H "Authorization: Bearer $TOKEN"
```

```json
{ "ok": true, "vouchers_checked": 1523, "attachments_checked": 311, "anchors_checked": 42, "problems": [] }
```

The call requires the right `FORANKRING_LES` (anchoring read) and does three things: it walks the chain from genesis and recomputes every voucher's hash, checks the attachments' content hashes, and compares every anchored chain head with the live chain. You get `ok: false` and a `problems` list with the sequence number at the first deviation. The same check exists in the portal (Verifiser kjeden mot forankringen, "Verify the chain against the anchoring") and as the audit report (`GET …/reports/revisjon`) with eight checks.

This proves something only if you trust the server to run its own code honestly. The next section removes that assumption.

## Independent check: the Merkle proof

The open feed `GET /anchors` requires no login and lists root hashes. Every independent copy of a root (your own cron job, an auditor's notebook) is one more witness that a rewrite of the database cannot reach.

```sh
curl -s https://api.regnmed.no/anchors | jq '.snapshots[0]'
curl -s https://api.regnmed.no/companies/$COMPANY/anchors?limit=1 -H "Authorization: Bearer $TOKEN" | jq '.anchors[0]'
```

The company response has `last_seq`, `last_hash` (the chain head that was anchored), `root_hash` and `proof`, a list of `{side, sibling}`. Everything you need to compute the root yourself:

- **Leaf** (format v1, frozen): `SHA-256(0x00 ‖ netstring("regnmed-anchor-v1") ‖ netstring(company id as 16 raw bytes) ‖ netstring(last_seq as decimal text) ‖ netstring(last_hash as 32 raw bytes))`, where `netstring(x)` is `<length>:<x>;`.
- **Node**: `SHA-256(0x01 ‖ left ‖ right)`.
- **Proof**: for each step in `proof`, the sibling is placed to the left (`side: "left"`) or right (`side: "right"`) of the running hash. The final result must equal `root_hash`, and `root_hash` must equal the root in the open feed for the same `snapshot_id`.

As code (Python 3, no dependencies). The self-test at the bottom computes the root for three leaves and requires it to equal the frozen value from `golden_root_never_changes` in `regnmed-core::anchor`:

```python
import hashlib, uuid

def sha(b): return hashlib.sha256(b).digest()
def field(b): return str(len(b)).encode() + b":" + b + b";"

def leaf_hash(company_id: str, last_seq: int, last_hash_hex: str) -> bytes:
    content = (field(b"regnmed-anchor-v1")
               + field(uuid.UUID(company_id).bytes)
               + field(str(last_seq).encode())
               + field(bytes.fromhex(last_hash_hex)))
    return sha(b"\x00" + content)

def verify_inclusion(company_id, last_seq, last_hash_hex, proof, root_hex) -> bool:
    h = leaf_hash(company_id, last_seq, last_hash_hex)
    for step in proof:                       # {"side": "left"|"right", "sibling": hex}
        sib = bytes.fromhex(step["sibling"])
        h = sha(b"\x01" + (sib + h if step["side"] == "left" else h + sib))
    return h.hex() == root_hex

# Self-test against the frozen test in regnmed-core::anchor (three leaves).
leaves = [(str(uuid.UUID(int=i)), i * 10, f"{i:02x}" * 32) for i in (1, 2, 3)]
l = [leaf_hash(*x) for x in leaves]
root = sha(b"\x01" + sha(b"\x01" + l[0] + l[1]) + l[2])
assert root.hex() == "79c9ecf01bda78fd05002e8f715d08e7d39ba1990adc6601ef79d6ac4fb9eec1"
proof = [{"side": "right", "sibling": l[1].hex()}, {"side": "right", "sibling": l[2].hex()}]
assert verify_inclusion(*leaves[0], proof, root.hex())
print("ok")
```

With a response from the API, the usage is `verify_inclusion(company_id, a["last_seq"], a["last_hash"], a["proof"], a["root_hash"])` for each element `a` in `anchors`.

:::note
The leaves are sorted by company id and an odd node is promoted unchanged (never duplicated), but you don't need to do that yourself: the proof already describes the path. The format is **frozen**. It can be replaced by a v2, never changed.
:::

## Timestamp from a third party

When operations has set `ANCHOR_TSA_URL`, the root is sent to an RFC 3161 timestamp service, and the signed token ("this hash existed no later than time T") is stored. It is verified offline with standard tools:

```sh
openssl ts -reply -in token.tsr -text
openssl ts -verify -digest <root-hex> -in token.tsr -CAfile <tsa-ca.pem>
```

Which witnesses a snapshot has is in `witnesses` in both responses. Without `ANCHOR_TSA_URL` the anchoring still exists and still detects changes, it is just not timestamped by a third party.

## What each check reveals

| Check | Catches |
| --- | --- |
| The chain recomputed from genesis | Any in-place edit without full access to the database |
| Anchored chain head against the live chain | Rewriting or truncation after the snapshot was taken |
| The root recomputed from the leaves | Tampering with the anchoring rows themselves |
| External witness (feed copies, RFC 3161) | Deletion or replacement of all the anchors |

The protection applies to history older than the newest witnessed root. Today's postings are protected by the next snapshot (nightly by default).

## Run it yourself against your own instance

`regnmed verify-ledger` walks the chain for all companies (or one with `--company <uuid>`), checks attachment hashes and the anchors, and **fails** on any deviation:

```sh
regnmed verify-ledger
regnmed verify-ledger --company 0192e8aa-…
```

The voucher hash itself (`SHA-256(previous_hash ‖ canonical content)`, format versions 1 to 4) is described precisely in [The general ledger](/teknisk/ledger/). The frozen digests in `golden_hashes_never_change` in `regnmed-core::hash` are the normative answer key, and the version is on every voucher (`hash_version`).

## Not built yet

- **The attachment set in the anchoring.** The attachments' *content* is hash-checked, but the anchoring does not prove that an attachment existed at snapshot time. Planned as a v2 of the leaf format (v1 roots verify forever).
- **Signed feed.** The feed rests on many independent copies, not a signature.
