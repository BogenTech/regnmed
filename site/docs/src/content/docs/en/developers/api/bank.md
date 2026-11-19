---
title: "Bank and payments"
description: "Bank reconciliation, OCR giro, payment lists, cards and currency."
editUrl: false
sidebar:
  order: 5
---

Bank reconciliation, OCR giro, payment lists, cards and currency.

The table is generated from the route table in `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Right** is the right the handler requires (see [Access and roles](/en/developers/access-and-roles/)); "public" means no token, "—" means the check lives in a helper function or that sign-in alone is required. Where several rights are listed, the requirement depends on the parameters.

| Method | Path | Right |
| --- | --- | --- |
| GET | `/companies/{company_id}/bank/accounts` | `BANK_LES` |
| GET | `/companies/{company_id}/bank/entries` | `BANK_LES` |
| GET | `/companies/{company_id}/bank/feed` | `BANK_LES` |
| GET | `/companies/{company_id}/bank/feed/banks` | `BANK_LES` |
| POST | `/companies/{company_id}/bank/feed/complete` | `BANK_AVSTEM` |
| POST | `/companies/{company_id}/bank/feed/connect` | `BANK_AVSTEM` |
| POST | `/companies/{company_id}/bank/feed/{connection_id}/end` | `BANK_AVSTEM` |
| POST | `/companies/{company_id}/bank/feed/{connection_id}/sync` | `BANK_AVSTEM` |
| POST | `/companies/{company_id}/bank/matches` | `BANK_AVSTEM` |
| DELETE | `/companies/{company_id}/bank/matches/{bank_transaction_id}` | `BANK_AVSTEM` |
| GET | `/companies/{company_id}/bank/reconciliation` | `BANK_LES` |
| POST | `/companies/{company_id}/bank/statements` | `BANK_AVSTEM` |
| POST | `/companies/{company_id}/bank/transactions/{bank_transaction_id}/post` | `BANK_AVSTEM`, `BILAG_BOKFOR` |
| GET | `/companies/{company_id}/cards` | `UTLEGG_SKRIV_EGNE`, `BANK_AVSTEM`, `UTLEGG_GODKJENN` |
| POST | `/companies/{company_id}/cards` | `BANK_AVSTEM` |
| PUT | `/companies/{company_id}/cards/{card_id}` | `BANK_AVSTEM` |
| GET | `/companies/{company_id}/currency/rates` | `VALUTA_LES` |
| POST | `/companies/{company_id}/currency/rates` | `VALUTA_SKRIV` |
| POST | `/companies/{company_id}/currency/rates/fetch` | `VALUTA_SKRIV` |
| POST | `/companies/{company_id}/currency/regulate` | `VALUTA_SKRIV` |
| POST | `/companies/{company_id}/ocr/files` | `OCR_IMPORT` |
| GET | `/companies/{company_id}/ocr/payments` | `OCR_LES` |
| GET | `/companies/{company_id}/payments/payable` | `BETALING_LES` |
| GET | `/companies/{company_id}/payments/runs` | `BETALING_LES` |
| POST | `/companies/{company_id}/payments/runs` | `BETALING_OPPRETT` |
| POST | `/companies/{company_id}/payments/runs/{run_id}/approve` | `BETALING_GODKJENN` |
| POST | `/companies/{company_id}/payments/runs/{run_id}/cancel` | `BETALING_OPPRETT` |
| GET | `/companies/{company_id}/payments/runs/{run_id}/file` | `BETALING_LES` |
| POST | `/companies/{company_id}/payments/runs/{run_id}/settle` | `BETALING_OPPGJOR` |

## Further reading (Norwegian)

- [bank](/teknisk/bank/)
- [betaling](/teknisk/betaling/)
- [valuta](/teknisk/valuta/)
