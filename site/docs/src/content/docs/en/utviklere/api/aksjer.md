---
title: "Share register and assets"
description: "Shareholders, share events, dividends, RF-1086 and the fixed-asset register."
editUrl: false
sidebar:
  order: 8
---

Shareholders, share events, dividends, RF-1086 and the fixed-asset register.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/utviklere/tilgang-og-roller/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/companies/{company_id}/assets` | `ANLEGG_LES` |
| POST | `/companies/{company_id}/assets` | `ANLEGG_SKRIV` |
| POST | `/companies/{company_id}/assets/depreciate` | `ANLEGG_SKRIV` |
| GET | `/companies/{company_id}/assets/saldo` | `ANLEGG_LES` |
| POST | `/companies/{company_id}/assets/{asset_id}/dispose` | `ANLEGG_SKRIV` |
| GET | `/companies/{company_id}/assets/{asset_id}/runs` | `ANLEGG_LES` |
| GET | `/companies/{company_id}/dividends` | `AKSJEBOK_LES` |
| POST | `/companies/{company_id}/dividends` | `AKSJEBOK_SKRIV` |
| GET | `/companies/{company_id}/reports/aksjonaeroppgave` | `AKSJEBOK_LES` |
| GET | `/companies/{company_id}/reports/aksjonaeroppgave/submissions` | `AKSJEBOK_LES` |
| POST | `/companies/{company_id}/reports/aksjonaeroppgave/submissions` | `AKSJEBOK_SKRIV` |
| POST | `/companies/{company_id}/reports/aksjonaeroppgave/submissions/{submission_id}/outcome` | `AKSJEBOK_LES` |
| GET | `/companies/{company_id}/share-events` | `AKSJEBOK_LES` |
| POST | `/companies/{company_id}/share-events` | `AKSJEBOK_SKRIV` |
| GET | `/companies/{company_id}/shareholders` | `AKSJEBOK_LES` |
| POST | `/companies/{company_id}/shareholders` | `AKSJEBOK_SKRIV` |
| GET | `/companies/{company_id}/shareholders/transaction-types` | `AKSJEBOK_LES` |
| PUT | `/companies/{company_id}/shareholders/{shareholder_id}/contact` | `AKSJEBOK_SKRIV` |

## Further reading (Norwegian)

- [aksjonaer](/teknisk/aksjonaer/)
- [anlegg](/teknisk/anlegg/)
