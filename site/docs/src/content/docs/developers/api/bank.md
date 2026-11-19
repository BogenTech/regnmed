---
title: "Bank og betaling"
description: "Bankavstemming, OCR-giro, betalingslister, kort og valuta."
editUrl: false
sidebar:
  order: 5
---

Bankavstemming, OCR-giro, betalingslister, kort og valuta.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/developers/access-and-roles/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
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

## Les mer

- [bank](/teknisk/bank/)
- [betaling](/teknisk/betaling/)
- [valuta](/teknisk/valuta/)
