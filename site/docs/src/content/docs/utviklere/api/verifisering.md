---
title: "Forankring og verifisering"
description: "Den åpne forankringsfeeden og verifisering per selskap."
editUrl: false
sidebar:
  order: 10
---

Den åpne forankringsfeeden og verifisering per selskap.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
| --- | --- | --- |
| GET | `/anchors` | åpen |
| GET | `/companies/{company_id}/anchors` | `FORANKRING_LES` |
| GET | `/companies/{company_id}/anchors/verify` | `FORANKRING_LES` |

## Former

`GET /anchors` er åpen (ingen token) og viser de 20 siste snapshotene:

```json
{ "snapshots": [{ "snapshot_id": "…", "created_at": "2026-11-18T01:00:03Z",
                  "root_hash": "79c9…", "leaf_count": 412,
                  "witnesses": [{ "method": "rfc3161", "reference": "…", "witnessed_at": "…" }] }] }
```

`GET /companies/{id}/anchors` gir per snapshot selskapets forankrede kjedehode og Merkle-beviset (`proof`: liste av `{side, sibling}`). `GET /companies/{id}/anchors/verify` kjører hele kontrollen og svarer `{ok, vouchers_checked, attachments_checked, anchors_checked, problems[]}`. Se [Verifiser selv](/utviklere/verifiser-selv/).


## Les mer

- [anchoring](/teknisk/anchoring/)
- [ledger](/teknisk/ledger/)
