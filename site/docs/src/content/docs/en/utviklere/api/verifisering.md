---
title: "Anchoring and verification"
description: "The public anchoring feed and per-company verification."
editUrl: false
sidebar:
  order: 10
---

The public anchoring feed and per-company verification.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/utviklere/tilgang-og-roller/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/anchors` | public |
| GET | `/companies/{company_id}/anchors` | `FORANKRING_LES` |
| GET | `/companies/{company_id}/anchors/verify` | `FORANKRING_LES` |

## Shapes

`GET /anchors` is public (no token) and lists the 20 latest snapshots:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` returns, per snapshot, the company's anchored chain head and the Merkle proof (`proof`: a list of `{side, sibling}`). `GET /companies/{id}/anchors/verify` runs the full check and answers `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. See [Verify it yourself](/en/utviklere/verifiser-selv/).


## Further reading (Norwegian)

- [anchoring](/teknisk/anchoring/)
- [ledger](/teknisk/ledger/)
