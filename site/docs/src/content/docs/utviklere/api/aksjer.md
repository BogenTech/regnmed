---
title: "Aksjeeierbok og anlegg"
description: "Aksjonærer, aksjehendelser, utbytte, RF-1086 og anleggsregisteret."
editUrl: false
sidebar:
  order: 8
---

Aksjonærer, aksjehendelser, utbytte, RF-1086 og anleggsregisteret.

Tabellen er generert fra rutetabellen i `crates/regnmed-api/src/lib.rs` (`scripts/gen-api-reference.py`). **Rett** er rettigheten handleren krever (se [Tilgang og roller](/utviklere/tilgang-og-roller/)); «åpen» betyr uten token, «—» at kontrollen ligger i en hjelpefunksjon eller at bare innlogging kreves. Der flere retter står, avhenger kravet av parametrene.

| Metode | Sti | Rett |
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

## Les mer

- [aksjonaer](/teknisk/aksjonaer/)
- [anlegg](/teknisk/anlegg/)
